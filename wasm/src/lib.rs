use js_sys::{Float32Array, Uint8Array};
use sen_core::{cartridge::Cartridge, controller::ControllerButtons, frame, nes::Nes};
use wasm_bindgen::{JsValue, prelude::wasm_bindgen};

#[wasm_bindgen]
pub struct Emulator {
    nes: Nes,
    rgba_frame: Vec<u8>,
    audio: Vec<f32>,
}

#[wasm_bindgen]
impl Emulator {
    #[wasm_bindgen(constructor)]
    pub fn new(rom: &[u8], sample_rate: f64) -> Result<Emulator, JsValue> {
        let cartridge =
            Cartridge::from_ines(rom).map_err(|err| js_sys::Error::new(&err.to_string()))?;

        let mut emulator = Emulator {
            nes: Nes::new_with_sample_rate(cartridge, sample_rate),
            rgba_frame: vec![0; frame::WIDTH * frame::HEIGHT * 4],
            audio: Vec::new(),
        };

        emulator.copy_frame_to_rgba();
        Ok(emulator)
    }

    #[wasm_bindgen(js_name = runFrame)]
    pub fn run_frame(&mut self) {
        self.nes.run_until_frame();
        self.copy_frame_to_rgba();

        while let Some(sample) = self.nes.pop_audio_sample() {
            self.audio.push(sample);
        }
    }

    #[wasm_bindgen(js_name = frameWidth)]
    pub fn frame_width(&self) -> usize {
        frame::WIDTH
    }

    #[wasm_bindgen(js_name = frameHeight)]
    pub fn frame_height(&self) -> usize {
        frame::HEIGHT
    }

    #[wasm_bindgen(js_name = frameBuffer)]
    pub fn frame_buffer(&self) -> Uint8Array {
        Uint8Array::from(self.rgba_frame.as_slice())
    }

    #[wasm_bindgen(js_name = takeAudio)]
    pub fn take_audio(&mut self) -> Float32Array {
        let samples = Float32Array::from(self.audio.as_slice());
        self.audio.clear();
        samples
    }

    #[wasm_bindgen(js_name = setController1)]
    pub fn set_controller1(&mut self, mask: u8) {
        self.nes.set_controller1(ControllerButtons::from_bits(mask));
    }

    #[wasm_bindgen(js_name = setController2)]
    pub fn set_controller2(&mut self, mask: u8) {
        self.nes.set_controller2(ControllerButtons::from_bits(mask));
    }

    pub fn reset(&mut self, rom: &[u8], sample_rate: f64) -> Result<(), JsValue> {
        let cartridge =
            Cartridge::from_ines(rom).map_err(|err| js_sys::Error::new(&err.to_string()))?;

        self.nes = Nes::new_with_sample_rate(cartridge, sample_rate);
        self.audio.clear();
        self.copy_frame_to_rgba();

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
