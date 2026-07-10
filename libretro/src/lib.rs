use libretro::{
    ContentContract, ControllerDescription, ControllerDevice, ControllerInfo, CoreMemory,
    Environment, GameInfo, InputDescriptor, InputPort, JoypadButton, MemoryRegion, PixelFormat,
    Region, Runtime, SystemInfo,
};
use sen_core::{cartridge::Cartridge, controller::ControllerButtons, frame, nes::Nes};

const DEFAULT_SAMPLE_RATE: f64 = 48_000.0;
const NTSC_FRAME_RATE: f64 = 60.0988;
const CONTROLLER_PORTS: usize = 2;

struct Core {
    nes: Option<Nes>,
    rom: Option<Vec<u8>>,
    video: Vec<u32>,
    audio: Vec<[i16; 2]>,
    sample_rate: f64,
    controller_enabled: [bool; CONTROLLER_PORTS],
}

impl Default for Core {
    fn default() -> Self {
        Self {
            nes: None,
            rom: None,
            video: vec![0; frame::WIDTH * frame::HEIGHT],
            audio: Vec::with_capacity(1024),
            sample_rate: DEFAULT_SAMPLE_RATE,
            controller_enabled: [true; CONTROLLER_PORTS],
        }
    }
}

impl Core {
    fn content_contract() -> ContentContract {
        ContentContract::new("nes")
            .with_need_fullpath(false)
            .with_block_extract(false)
            .with_support_no_game(false)
            .with_persistent_data(false)
    }

    fn controller_info() -> Vec<ControllerInfo> {
        (0..CONTROLLER_PORTS)
            .map(|_| {
                ControllerInfo::new(vec![ControllerDescription::new(
                    "NES Controller",
                    ControllerDevice::Joypad,
                )])
            })
            .collect()
    }

    fn input_descriptors() -> Vec<InputDescriptor> {
        let mut descriptors = Vec::with_capacity(CONTROLLER_PORTS * 8);

        for port in 0..CONTROLLER_PORTS as u32 {
            descriptors.extend([
                InputDescriptor::joypad(port, JoypadButton::A, "A"),
                InputDescriptor::joypad(port, JoypadButton::B, "B"),
                InputDescriptor::joypad(port, JoypadButton::Select, "Select"),
                InputDescriptor::joypad(port, JoypadButton::Start, "Start"),
                InputDescriptor::joypad(port, JoypadButton::Up, "D-Pad Up"),
                InputDescriptor::joypad(port, JoypadButton::Down, "D-Pad Down"),
                InputDescriptor::joypad(port, JoypadButton::Left, "D-Pad Left"),
                InputDescriptor::joypad(port, JoypadButton::Right, "D-Pad Right"),
            ]);
        }

        descriptors
    }

    fn controller_buttons(runtime: &Runtime<'_>, port: u32, enabled: bool) -> ControllerButtons {
        if !enabled {
            return ControllerButtons::default();
        }

        let up = runtime.joypad_pressed(port, JoypadButton::Up);
        let down = runtime.joypad_pressed(port, JoypadButton::Down);
        let left = runtime.joypad_pressed(port, JoypadButton::Left);
        let right = runtime.joypad_pressed(port, JoypadButton::Right);

        ControllerButtons::default()
            .with_a(runtime.joypad_pressed(port, JoypadButton::A))
            .with_b(runtime.joypad_pressed(port, JoypadButton::B))
            .with_select(runtime.joypad_pressed(port, JoypadButton::Select))
            .with_start(runtime.joypad_pressed(port, JoypadButton::Start))
            .with_up(up && !down)
            .with_down(down && !up)
            .with_left(left && !right)
            .with_right(right && !left)
    }
}

impl libretro::Core for Core {
    fn system_info(&self) -> libretro::SystemInfo {
        let mut info = SystemInfo::new("SEN", env!("CARGO_PKG_VERSION"));
        Self::content_contract().apply_to_system_info(&mut info);
        info
    }

    fn av_info(&self) -> libretro::SystemAvInfo {
        let mut info = libretro::fixed_system_av_info(
            frame::WIDTH as u32,
            frame::HEIGHT as u32,
            NTSC_FRAME_RATE,
            self.sample_rate,
        );

        info.geometry.aspect_ratio = 4.0 / 3.0;
        info
    }

    fn on_set_environment(&mut self, env: &mut Environment<'_>) {
        let _ = Self::content_contract().register_environment(env);
        let _ = env.set_controller_info(&Self::controller_info());
        let _ = env.set_input_descriptors(&Self::input_descriptors());
    }

    fn set_controller_port_device(&mut self, port: InputPort, device: ControllerDevice) {
        let Some(enabled) = self.controller_enabled.get_mut(port.as_raw() as usize) else {
            return;
        };

        *enabled = matches!(device, ControllerDevice::Joypad);
    }

    fn load_game(&mut self, game: Option<GameInfo<'_>>, runtime: &mut Runtime<'_>) -> bool {
        let Some(data) = game.and_then(|game| game.data) else {
            runtime.logger().error("SEN: no ROM data supplied");
            return false;
        };

        let cartridge = match Cartridge::from_ines(data) {
            Ok(cartridge) => cartridge,
            Err(error) => {
                runtime
                    .logger()
                    .error(format!("SEN: failed to load ROM: {error}"));
                return false;
            }
        };

        if !runtime
            .environment()
            .set_pixel_format(PixelFormat::Xrgb8888)
        {
            runtime.logger().error("SEN: frontend rejected XRGB8888");
            return false;
        }

        self.sample_rate = runtime
            .environment()
            .target_sample_rate()
            .map(|rate| f64::from(rate.get()))
            .filter(|rate| *rate > 0.0)
            .unwrap_or(DEFAULT_SAMPLE_RATE);

        self.nes = Some(Nes::new_with_sample_rate(cartridge, self.sample_rate));
        self.rom = Some(data.to_vec());
        self.audio.clear();
        true
    }

    fn unload_game(&mut self) {
        self.nes = None;
        self.rom = None;
        self.audio.clear();
    }

    fn memory_region(&mut self, region: MemoryRegion) -> Option<CoreMemory<'_>> {
        match region {
            MemoryRegion::SaveRam => self
                .nes
                .as_mut()?
                .save_ram_mut()
                .map(CoreMemory::read_write),
            _ => None,
        }
    }

    fn region(&self) -> Region {
        Region::Ntsc
    }

    fn reset(&mut self) {
        let Some(rom) = self.rom.as_deref() else {
            return;
        };

        let save = self
            .nes
            .as_ref()
            .and_then(Nes::save_ram)
            .map(<[u8]>::to_vec);

        let Ok(cartridge) = Cartridge::from_ines(rom) else {
            return;
        };

        let mut nes = Nes::new_with_sample_rate(cartridge, self.sample_rate);

        if let Some(save) = save {
            let _ = nes.load_save_ram(&save);
        }

        self.nes = Some(nes);
        self.audio.clear();
    }

    fn run(&mut self, runtime: &mut Runtime<'_>) {
        runtime.poll_input();

        let controller1 = Self::controller_buttons(runtime, 0, self.controller_enabled[0]);
        let controller2 = Self::controller_buttons(runtime, 1, self.controller_enabled[1]);

        let Some(nes) = self.nes.as_mut() else {
            return;
        };

        nes.set_controller1(controller1);
        nes.set_controller2(controller2);
        nes.run_until_frame();

        for (destination, rgb) in self
            .video
            .iter_mut()
            .zip(nes.frame().pixels().chunks_exact(3))
        {
            *destination = ((rgb[0] as u32) << 16) | ((rgb[1] as u32) << 8) | rgb[2] as u32;
        }

        while let Some(sample) = nes.pop_audio_sample() {
            let sample = (sample.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16;
            self.audio.push([sample, sample]);
        }

        let _ = runtime.video_refresh_frame_with_audio(
            &self.video,
            frame::WIDTH as u32,
            frame::HEIGHT as u32,
            frame::WIDTH * size_of::<u32>(),
            &self.audio,
        );

        self.audio.clear();
    }
}

libretro::export_core!(Core::default());

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_nes_content_and_ntsc_av_info() {
        let core = Core::default();
        let system = <Core as libretro::Core>::system_info(&core);
        let av = <Core as libretro::Core>::av_info(&core);

        assert_eq!(system.library_name, "SEN");
        assert_eq!(system.valid_extensions.as_deref(), Some("nes"));
        assert!(!system.need_fullpath);
        assert!(!system.block_extract);
        assert_eq!(av.geometry.base_width, frame::WIDTH as u32);
        assert_eq!(av.geometry.base_height, frame::HEIGHT as u32);
        assert_eq!(av.geometry.aspect_ratio, 4.0 / 3.0);
        assert_eq!(av.timing.fps, NTSC_FRAME_RATE);
        assert_eq!(av.timing.sample_rate, DEFAULT_SAMPLE_RATE);
        assert_eq!(<Core as libretro::Core>::region(&core), Region::Ntsc);
    }

    #[test]
    fn declares_two_nes_controllers_and_all_buttons() {
        let controllers = Core::controller_info();
        let descriptors = Core::input_descriptors();

        assert_eq!(controllers.len(), CONTROLLER_PORTS);
        assert!(controllers.iter().all(|port| port.types.len() == 1));
        assert_eq!(descriptors.len(), CONTROLLER_PORTS * 8);
    }
}
