use js_sys::{Float32Array, Uint8Array};
use sen_core::{
    cartridge::Cartridge,
    cheat::GameGenieCode,
    controller::ControllerButtons,
    frame,
    nes::{InputFrame, Nes},
};
use wasm_bindgen::{JsValue, prelude::wasm_bindgen};

/// A self-contained NES emulator.
///
/// Create an emulator from an iNES ROM, call `runFrame()` to advance it, then
/// read the video and audio produced by that frame. Call `free()` when the
/// instance is no longer needed.
#[wasm_bindgen]
pub struct Emulator {
    nes: Nes,
    rgba_frame: Vec<u8>,
    audio: Vec<f32>,
    controller1: ControllerButtons,
    controller2: ControllerButtons,
}

#[wasm_bindgen]
impl Emulator {
    /// Creates an emulator loaded with an iNES ROM.
    ///
    /// `sample_rate` is the output audio sample rate in hertz, usually
    /// `AudioContext.sampleRate`. Throws an `Error` if `rom` is invalid or uses
    /// an unsupported mapper.
    #[wasm_bindgen(constructor)]
    pub fn new(rom: &[u8], sample_rate: f64) -> Result<Emulator, JsValue> {
        let cartridge =
            Cartridge::from_ines(rom).map_err(|err| js_sys::Error::new(&err.to_string()))?;

        let mut emulator = Emulator {
            nes: Nes::new_with_sample_rate(cartridge, sample_rate),
            rgba_frame: vec![0; frame::WIDTH * frame::HEIGHT * 4],
            audio: Vec::new(),
            controller1: ControllerButtons::default(),
            controller2: ControllerButtons::default(),
        };

        emulator.copy_frame_to_rgba();
        Ok(emulator)
    }

    /// Advances emulation until the next complete video frame.
    ///
    /// After calling this method, use `frameBuffer()` to read the new frame and
    /// `takeAudio()` to drain the audio samples generated so far.
    #[wasm_bindgen(js_name = runFrame)]
    pub fn run_frame(&mut self) {
        let input = InputFrame::new(self.controller1, self.controller2);
        self.nes.run_frame(input);
        self.copy_frame_to_rgba();

        while let Some(sample) = self.nes.pop_audio_sample() {
            self.audio.push(sample);
        }
    }

    /// Returns the video frame width in pixels.
    #[wasm_bindgen(js_name = frameWidth)]
    pub fn frame_width(&self) -> usize {
        frame::WIDTH
    }

    /// Returns the video frame height in pixels.
    #[wasm_bindgen(js_name = frameHeight)]
    pub fn frame_height(&self) -> usize {
        frame::HEIGHT
    }

    /// Returns a copy of the current video frame as row-major RGBA bytes.
    ///
    /// The returned array contains `frameWidth() * frameHeight() * 4` bytes and
    /// can be copied directly into an `ImageData` object's `data` array.
    #[wasm_bindgen(js_name = frameBuffer)]
    pub fn frame_buffer(&self) -> Uint8Array {
        Uint8Array::from(self.rgba_frame.as_slice())
    }

    /// Returns all queued mono audio samples and clears the audio queue.
    ///
    /// Samples are 32-bit floating-point values at the sample rate passed to
    /// the constructor or the most recent call to `reset()`.
    #[wasm_bindgen(js_name = takeAudio)]
    pub fn take_audio(&mut self) -> Float32Array {
        let samples = Float32Array::from(self.audio.as_slice());
        self.audio.clear();
        samples
    }

    /// Sets the buttons currently held on controller 1.
    ///
    /// Combine buttons with bitwise OR:
    /// A = `1 << 0`
    /// B = `1 << 1`
    /// Select = `1 << 2`
    /// Start = `1 << 3`
    /// Up = `1 << 4`
    /// Down = `1 << 5`
    /// Left = `1 << 6`
    /// Right = `1 << 7`.
    ///
    /// Pass `0` to release every button.
    #[wasm_bindgen(js_name = setController1)]
    pub fn set_controller1(&mut self, mask: u8) {
        self.controller1 = ControllerButtons::from_bits(mask);
    }

    /// Sets the buttons currently held on controller 2.
    ///
    /// Uses the same button mask as `setController1()`. Pass `0` to release
    /// every button.
    #[wasm_bindgen(js_name = setController2)]
    pub fn set_controller2(&mut self, mask: u8) {
        self.controller2 = ControllerButtons::from_bits(mask);
    }

    /// Replaces the current game with a new iNES ROM.
    ///
    /// This starts the new game from power-on state and discards queued audio.
    /// `sample_rate` is in hertz. Throws an `Error` if `rom` is invalid or uses
    /// an unsupported mapper.
    pub fn reset(&mut self, rom: &[u8], sample_rate: f64) -> Result<(), JsValue> {
        let cartridge =
            Cartridge::from_ines(rom).map_err(|err| js_sys::Error::new(&err.to_string()))?;

        self.nes = Nes::new_with_sample_rate(cartridge, sample_rate);
        self.audio.clear();
        self.controller1 = ControllerButtons::default();
        self.controller2 = ControllerButtons::default();
        self.copy_frame_to_rgba();

        Ok(())
    }

    /// Returns a copy of the current cartridge's battery-backed save RAM.
    ///
    /// Throws an `Error` when the loaded ROM does not declare battery-backed
    /// RAM. Persist the returned bytes and restore them with `loadSaveRam()`.
    #[wasm_bindgen(js_name = saveRam)]
    pub fn save_ram(&self) -> Result<Uint8Array, JsValue> {
        self.nes
            .save_ram()
            .map(Uint8Array::from)
            .ok_or_else(|| js_sys::Error::new("No save RAM available").into())
    }

    /// Restores the current cartridge's battery-backed save RAM.
    ///
    /// Throws an `Error` if the ROM has no battery-backed RAM or if `ram` has
    /// the wrong length for the cartridge.
    #[wasm_bindgen(js_name = loadSaveRam)]
    pub fn load_save_ram(&mut self, ram: &[u8]) -> Result<(), JsValue> {
        self.nes
            .load_save_ram(ram)
            .map_err(|err| js_sys::Error::new(&err.to_string()).into())
    }

    /// Returns an opaque copy of the emulator's current state.
    ///
    /// The state can be restored with `loadState()`.
    #[wasm_bindgen(js_name = saveState)]
    pub fn save_state(&self) -> Result<Uint8Array, JsValue> {
        let mut image = vec![0; self.nes.serialized_state_size()];

        self.nes
            .serialize_state(&mut image)
            .map_err(|err| js_sys::Error::new(&err.to_string()))?;

        Ok(Uint8Array::from(image.as_slice()))
    }

    /// Restores a state created by `saveState()`.
    ///
    /// Throws for corrupt data, a different ROM/sample rate, or an unsupported
    /// state version. Failed loads leave the emulator unchanged.
    #[wasm_bindgen(js_name = loadState)]
    pub fn load_state(&mut self, image: &[u8]) -> Result<(), JsValue> {
        self.nes
            .unserialize_state(image)
            .map_err(|err| js_sys::Error::new(&err.to_string()))?;

        self.audio.clear();
        self.copy_frame_to_rgba();

        Ok(())
    }

    /// Replaces all active Game Genie codes.
    ///
    /// Each entry must be a six- or eight-character NES Game Genie code.
    /// Passing an empty array disables all cheats. If any entry is invalid,
    /// the active codes remain unchanged.
    #[wasm_bindgen(js_name = setGameGenieCodes)]
    pub fn set_game_genie_codes(&mut self, codes: Vec<String>) -> Result<(), JsValue> {
        let codes = parse_game_genie_codes(codes).map_err(|error| js_sys::Error::new(&error))?;
        self.nes.set_game_genie_codes(codes);
        Ok(())
    }
}

impl Emulator {
    fn copy_frame_to_rgba(&mut self) {
        let expected_len = frame::WIDTH * frame::HEIGHT * 4;

        if self.rgba_frame.len() != expected_len {
            self.rgba_frame.resize(expected_len, 0);
        }

        for (rgba, rgb) in self
            .rgba_frame
            .chunks_exact_mut(4)
            .zip(self.nes.frame().pixels().chunks_exact(3))
        {
            rgba[0..=2].copy_from_slice(&rgb[0..=2]);
            rgba[3] = 0xFF;
        }
    }
}

fn parse_game_genie_codes(codes: Vec<String>) -> Result<Vec<GameGenieCode>, String> {
    codes
        .into_iter()
        .map(|code| {
            code.parse()
                .map_err(|error| format!("Invalid Game Genie code {code:?}: {error}"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_game_genie_code_list() {
        let codes = parse_game_genie_codes(vec!["GOSSIP".into(), "ZEXPYGLA".into()]).unwrap();
        assert_eq!(codes.len(), 2);
    }

    #[test]
    fn rejects_a_list_containing_an_invalid_code() {
        let result = parse_game_genie_codes(vec!["GOSSIP".into(), "INVALID".into()]);
        assert!(result.is_err());
    }

    #[test]
    fn empty_list_clears_cheats() {
        assert!(parse_game_genie_codes(Vec::new()).unwrap().is_empty());
    }
}
