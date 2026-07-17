use std::collections::BTreeMap;

use libretro::{
    CheatCode, CheatIndex, ContentContract, ControllerDescription, ControllerDevice,
    ControllerInfo, CoreMemory, CoreOptionCategory, CoreOptionDefinition, CoreOptionValue,
    CoreOptions, Environment, GameGeometry, GameInfo, InputDescriptor, InputPort, JoypadButton,
    Logger, MemoryDescriptorFlag, MemoryDescriptorFlags, MemoryMapDescriptor, MemoryMapMask,
    MemoryRegion, PixelFormat, Region, Runtime, SystemInfo,
};
use sen_core::{
    cartridge::Cartridge,
    cheat::{GameGenieCode, GameGenieCodeError},
    controller::ControllerButtons,
    frame,
    nes::{InputFrame, Nes},
};

const DEFAULT_SAMPLE_RATE: f64 = 48_000.0;
const NTSC_FRAME_RATE: f64 = 60.0988;
const CONTROLLER_PORTS: usize = 2;

const OPTION_CROP_OVERSCAN: &str = "sen_crop_overscan";
const OPTION_ASPECT_RATIO: &str = "sen_aspect_ratio";
const OPTION_ALLOW_OPPOSITE_DIRECTIONS: &str = "sen_allow_opposite_directions";
const OPTION_AUDIO_GAIN: &str = "sen_audio_gain";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum CropOverscan {
    #[default]
    Disabled,
    Vertical,
    All,
}

impl CropOverscan {
    fn viewport(self) -> Viewport {
        const OVERSCAN: usize = 8;

        match self {
            Self::Disabled => Viewport {
                left: 0,
                top: 0,
                width: frame::WIDTH,
                height: frame::HEIGHT,
            },
            Self::Vertical => Viewport {
                left: 0,
                top: OVERSCAN,
                width: frame::WIDTH,
                height: frame::HEIGHT - OVERSCAN * 2,
            },
            Self::All => Viewport {
                left: OVERSCAN,
                top: OVERSCAN,
                width: frame::WIDTH - OVERSCAN * 2,
                height: frame::HEIGHT - OVERSCAN * 2,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum AspectRatio {
    #[default]
    FourThree,
    Pixel,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct CoreSettings {
    crop_overscan: CropOverscan,
    aspect_ratio: AspectRatio,
    allow_opposite_directions: bool,
    audio_gain: f32,
}

impl Default for CoreSettings {
    fn default() -> Self {
        Self {
            crop_overscan: CropOverscan::Disabled,
            aspect_ratio: AspectRatio::FourThree,
            allow_opposite_directions: false,
            audio_gain: 1.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Viewport {
    left: usize,
    top: usize,
    width: usize,
    height: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Cheat {
    enabled: bool,
    codes: Vec<GameGenieCode>,
}

fn parse_game_genie_codes(code: &str) -> Result<Vec<GameGenieCode>, GameGenieCodeError> {
    code.split('+').map(str::trim).map(str::parse).collect()
}

struct Core {
    nes: Option<Nes>,
    rom: Option<Vec<u8>>,
    video: Vec<u32>,
    audio: Vec<[i16; 2]>,
    sample_rate: f64,
    controller_enabled: [bool; CONTROLLER_PORTS],
    settings: CoreSettings,
    logger: Logger,
    cheats: BTreeMap<u32, Cheat>,
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
            settings: CoreSettings::default(),
            logger: Logger::default(),
            cheats: BTreeMap::new(),
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

    fn controller_buttons(
        runtime: &Runtime<'_>,
        port: u32,
        enabled: bool,
        allow_opposite_directions: bool,
    ) -> ControllerButtons {
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
            .with_up(up && (allow_opposite_directions || !down))
            .with_down(down && (allow_opposite_directions || !up))
            .with_left(left && (allow_opposite_directions || !right))
            .with_right(right && (allow_opposite_directions || !left))
    }

    fn core_options() -> CoreOptions {
        CoreOptions::new([
            CoreOptionDefinition::new(OPTION_CROP_OVERSCAN, "Crop Overscan", "disabled")
                .with_category("video")
                .with_info(concat!(
                    "Hides 8 pixels from the selected edges to remove graphics normally ",
                    "concealed by a television's overscan area.",
                ))
                .with_values([
                    CoreOptionValue::new("disabled").with_label("Disabled"),
                    CoreOptionValue::new("vertical").with_label("Top and Bottom"),
                    CoreOptionValue::new("all").with_label("All Sides"),
                ]),
            CoreOptionDefinition::new(OPTION_ASPECT_RATIO, "Aspect Ratio", "4:3")
                .with_category("video")
                .with_info(concat!(
                    "Controls the shape of the image when RetroArch uses the core-provided ",
                    "aspect ratio. 4:3 matches a traditional television; 1:1 displays ",
                    "emulated pixels as square.",
                ))
                .with_values([
                    CoreOptionValue::new("4:3").with_label("4:3 (TV)"),
                    CoreOptionValue::new("pixel").with_label("1:1 PAR (Square Pixels)"),
                ]),
            CoreOptionDefinition::new(
                OPTION_ALLOW_OPPOSITE_DIRECTIONS,
                "Allow Opposite Directions",
                "disabled",
            )
            .with_category("input")
            .with_info(concat!(
                "Allows Up+Down and Left+Right to be pressed simultaneously. ",
                "This is impossible on a standard NES controller and may ",
                "produce unusual behavior.",
            ))
            .with_values([
                CoreOptionValue::new("disabled").with_label("Disabled"),
                CoreOptionValue::new("enabled").with_label("Enabled"),
            ]),
            CoreOptionDefinition::new(OPTION_AUDIO_GAIN, "Audio Gain", "100")
                .with_info(
                    "Adjusts the core's output volume. Values above 100% may cause audio clipping.",
                )
                .with_category("audio")
                .with_info("Adjusts the volume of audio produced by the core.")
                .with_values([
                    CoreOptionValue::new("50").with_label("50%"),
                    CoreOptionValue::new("75").with_label("75%"),
                    CoreOptionValue::new("100").with_label("100%"),
                    CoreOptionValue::new("125").with_label("125%"),
                    CoreOptionValue::new("150").with_label("150%"),
                ]),
        ])
        .with_categories([
            CoreOptionCategory::new("video", "Video")
                .with_info("Controls image cropping and proportions."),
            CoreOptionCategory::new("input", "Input")
                .with_info("Controls NES controller behavior."),
            CoreOptionCategory::new("audio", "Audio")
                .with_info("Controls the core's audio output."),
        ])
    }

    fn refresh_settings(&mut self, env: &mut Environment<'_>) -> bool {
        let previous = self.settings;

        self.settings.crop_overscan = match env.get_variable(OPTION_CROP_OVERSCAN).as_deref() {
            Some("vertical") => CropOverscan::Vertical,
            Some("all") => CropOverscan::All,
            _ => CropOverscan::Disabled,
        };

        self.settings.aspect_ratio = match env.get_variable(OPTION_ASPECT_RATIO).as_deref() {
            Some("pixel") => AspectRatio::Pixel,
            _ => AspectRatio::FourThree,
        };

        self.settings.allow_opposite_directions = matches!(
            env.get_variable(OPTION_ALLOW_OPPOSITE_DIRECTIONS)
                .as_deref(),
            Some("enabled")
        );

        self.settings.audio_gain = match env.get_variable(OPTION_AUDIO_GAIN).as_deref() {
            Some("50") => 0.5,
            Some("75") => 0.75,
            Some("125") => 1.25,
            Some("150") => 1.5,
            _ => 1.0,
        };

        previous.crop_overscan != self.settings.crop_overscan
            || previous.aspect_ratio != self.settings.aspect_ratio
    }

    fn geometry(&self) -> GameGeometry {
        let viewport = self.settings.crop_overscan.viewport();

        let aspect_ratio = match self.settings.aspect_ratio {
            AspectRatio::FourThree => 4.0 / 3.0,
            AspectRatio::Pixel => viewport.width as f32 / viewport.height as f32,
        };

        GameGeometry {
            base_width: viewport.width as u32,
            base_height: viewport.height as u32,
            max_width: frame::WIDTH as u32,
            max_height: frame::HEIGHT as u32,
            aspect_ratio,
        }
    }

    fn sync_cheats(&mut self) {
        let codes = self
            .cheats
            .values()
            .filter(|cheat| cheat.enabled)
            .flat_map(|cheat| cheat.codes.iter().copied())
            .collect();

        if let Some(nes) = self.nes.as_mut() {
            nes.set_game_genie_codes(codes);
        }
    }

    fn update_cheat(&mut self, index: u32, enabled: bool, code: Option<&str>) {
        if let Some(code) = code {
            match parse_game_genie_codes(code) {
                Ok(codes) => {
                    self.cheats.insert(index, Cheat { enabled, codes });
                }
                Err(error) => {
                    self.logger
                        .warn(format!("SEN: rejected cheat {index}: {error}"));
                    return;
                }
            }
        } else if let Some(cheat) = self.cheats.get_mut(&index) {
            cheat.enabled = enabled;
        } else {
            return;
        }

        self.sync_cheats();
    }
}

impl libretro::Core for Core {
    fn system_info(&self) -> libretro::SystemInfo {
        let mut info = SystemInfo::new("SEN", env!("CARGO_PKG_VERSION"));
        Self::content_contract().apply_to_system_info(&mut info);
        info
    }

    fn av_info(&self) -> libretro::SystemAvInfo {
        libretro::system_av_info(self.geometry(), NTSC_FRAME_RATE, self.sample_rate)
    }

    fn on_set_environment(&mut self, env: &mut Environment<'_>) {
        self.logger = env.logger();

        let _ = Self::content_contract().register_environment(env);
        let _ = env.set_controller_info(&Self::controller_info());
        let _ = env.set_input_descriptors(&Self::input_descriptors());

        match env.set_core_options(&Self::core_options()) {
            Ok(true) => {}
            Ok(false) => env.logger().warn("SEN: frontend rejected core options"),
            Err(error) => env
                .logger()
                .error(format!("SEN: failed to build core options: {error:?}")),
        }
    }

    fn set_controller_port_device(&mut self, port: InputPort, device: ControllerDevice) {
        let Some(enabled) = self.controller_enabled.get_mut(port.as_raw() as usize) else {
            return;
        };

        *enabled = matches!(device, ControllerDevice::Joypad);
    }

    fn load_game(&mut self, game: Option<GameInfo<'_>>, runtime: &mut Runtime<'_>) -> bool {
        let mut env = runtime.environment();
        self.refresh_settings(&mut env);

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

        let nes = self.nes.as_mut().unwrap();

        let system_ram = MemoryMapDescriptor::from_slice(None::<String>, 0, nes.system_ram_mut())
            .with_flags(MemoryDescriptorFlags::from(MemoryDescriptorFlag::SystemRam))
            .with_select(MemoryMapMask::new(0xE000))
            .with_disconnect(MemoryMapMask::new(0x1800));

        let address_space_end =
            MemoryMapDescriptor::new_inaccessible(None::<String>, 0, MemoryMapMask::new(0xFFFF));

        let _ = runtime
            .environment()
            .set_memory_maps(&[system_ram, address_space_end]);
        let _ = runtime.environment().set_support_achievements(true);

        self.sync_cheats();

        true
    }

    fn unload_game(&mut self) {
        self.nes = None;
        self.rom = None;
        self.audio.clear();
    }

    fn memory_region(&mut self, region: MemoryRegion) -> Option<CoreMemory<'_>> {
        let nes = self.nes.as_mut()?;

        match region {
            MemoryRegion::SaveRam => nes.save_ram_mut().map(CoreMemory::read_write),
            MemoryRegion::SystemRam => Some(CoreMemory::read_write(nes.system_ram_mut())),
            _ => None,
        }
    }

    fn cheat_reset(&mut self) {
        self.cheats.clear();
        self.sync_cheats();
    }

    fn cheat_set(&mut self, index: CheatIndex, enabled: bool, code: Option<CheatCode<'_>>) {
        let index = index.get();
        let code = match code.map(CheatCode::to_str).transpose() {
            Ok(code) => code,
            Err(error) => {
                self.logger
                    .warn(format!("SEN: cheat {index} is not valid UTF-8: {error}"));
                return;
            }
        };

        self.update_cheat(index, enabled, code);
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
        self.sync_cheats();
        self.audio.clear();
    }

    fn serialize_size(&self) -> usize {
        self.nes.as_ref().map_or(0, Nes::serialized_state_size)
    }

    fn serialize(&self, data: &mut [u8]) -> bool {
        let Some(nes) = self.nes.as_ref() else {
            self.logger
                .error("SEN: cannot serialize state without loaded content");
            return false;
        };

        let expected = nes.serialized_state_size();

        if data.len() < expected {
            self.logger.error(format!(
                "SEN: state buffer is too small: expected at least {expected} bytes, got {}",
                data.len(),
            ));
            return false;
        }

        let (image, padding) = data.split_at_mut(expected);

        match nes.serialize_state(image) {
            Ok(()) => {
                padding.fill(0);
                true
            }
            Err(error) => {
                self.logger
                    .error(format!("SEN: failed to serialize state: {error}"));
                false
            }
        }
    }

    fn unserialize(&mut self, data: &[u8]) -> bool {
        let Some(nes) = self.nes.as_mut() else {
            self.logger
                .error("SEN: cannot unserialize state without loaded content");
            return false;
        };

        let expected = nes.serialized_state_size();

        if data.len() < expected {
            self.logger.error(format!(
                "SEN: state buffer is too small: expected at least {expected} bytes, got {}",
                data.len(),
            ));
            return false;
        }

        match nes.unserialize_state(&data[..expected]) {
            Ok(()) => {
                self.audio.clear();
                self.video.fill(0);
                true
            }
            Err(error) => {
                self.logger
                    .error(format!("SEN: failed to unserialize state: {error}"));
                false
            }
        }
    }

    fn run(&mut self, runtime: &mut Runtime<'_>) {
        let mut env = runtime.environment();

        if env.variables_updated() {
            let geometry_changed = self.refresh_settings(&mut env);
            if geometry_changed && !env.set_geometry(self.geometry()) {
                env.logger().warn("SEN: frontend rejected updated geometry")
            }
        }

        runtime.poll_input();

        let controller1 = Self::controller_buttons(
            runtime,
            0,
            self.controller_enabled[0],
            self.settings.allow_opposite_directions,
        );
        let controller2 = Self::controller_buttons(
            runtime,
            1,
            self.controller_enabled[1],
            self.settings.allow_opposite_directions,
        );

        let Some(nes) = self.nes.as_mut() else {
            return;
        };

        nes.run_frame(InputFrame::new(controller1, controller2));

        for (destination, rgb) in self
            .video
            .iter_mut()
            .zip(nes.frame().pixels().chunks_exact(3))
        {
            *destination = ((rgb[0] as u32) << 16) | ((rgb[1] as u32) << 8) | rgb[2] as u32;
        }

        let audio_gain = self.settings.audio_gain;

        while let Some(sample) = nes.pop_audio_sample() {
            let sample = (sample * audio_gain).clamp(-1.0, 1.0);
            let sample = (sample * i16::MAX as f32).round() as i16;
            self.audio.push([sample, sample]);
        }

        let viewport = self.settings.crop_overscan.viewport();
        let offset = viewport.top * frame::WIDTH + viewport.left;
        let visible_video = &self.video[offset..];

        let _ = runtime.video_refresh_frame_with_audio(
            visible_video,
            viewport.width as u32,
            viewport.height as u32,
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

    fn loaded_core_at_frame_boundary() -> Core {
        let mut prg_rom = vec![0; 0x4000];
        prg_rom[..3].copy_from_slice(&[0x4C, 0x00, 0x80]);
        prg_rom[0x3FFC] = 0x00;
        prg_rom[0x3FFD] = 0x80;

        let mut rom = vec![0; 16];
        rom[0..4].copy_from_slice(b"NES\x1A");
        rom[4] = 1;
        rom[5] = 1;
        rom.extend_from_slice(&prg_rom);
        rom.extend_from_slice(&vec![0; 0x2000]);

        let cartridge = Cartridge::from_ines(&rom).unwrap();
        let mut nes = Nes::new_with_sample_rate(cartridge, DEFAULT_SAMPLE_RATE);
        nes.run_frame(InputFrame::default());

        while nes.pop_audio_sample().is_some() {}

        Core {
            nes: Some(nes),
            rom: Some(rom),
            ..Core::default()
        }
    }

    fn serialized_image(core: &Core) -> Vec<u8> {
        let size = <Core as libretro::Core>::serialize_size(core);
        let mut image = vec![0; size];

        assert!(<Core as libretro::Core>::serialize(core, &mut image));

        image
    }

    fn drain_nes_audio(core: &mut Core) -> Vec<f32> {
        let mut samples = Vec::new();

        while let Some(sample) = core.nes.as_mut().unwrap().pop_audio_sample() {
            samples.push(sample);
        }

        samples
    }

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

    #[test]
    fn overscan_viewports_have_expected_dimensions() {
        assert_eq!(
            CropOverscan::Disabled.viewport(),
            Viewport {
                left: 0,
                top: 0,
                width: 256,
                height: 240,
            }
        );

        assert_eq!(
            CropOverscan::Vertical.viewport(),
            Viewport {
                left: 0,
                top: 8,
                width: 256,
                height: 224,
            }
        );

        assert_eq!(
            CropOverscan::All.viewport(),
            Viewport {
                left: 8,
                top: 8,
                width: 240,
                height: 224,
            }
        );
    }

    #[test]
    fn pixel_aspect_ratio_uses_visible_dimensions() {
        let mut core = Core::default();
        core.settings.crop_overscan = CropOverscan::All;
        core.settings.aspect_ratio = AspectRatio::Pixel;

        let geometry = core.geometry();

        assert_eq!(geometry.base_width, 240);
        assert_eq!(geometry.base_height, 224);
        assert_eq!(geometry.max_width, 256);
        assert_eq!(geometry.max_height, 240);
        assert_eq!(geometry.aspect_ratio, 240.0 / 224.0);
    }

    #[test]
    fn serialization_size_tracks_loaded_content() {
        let empty = Core::default();
        assert_eq!(<Core as libretro::Core>::serialize_size(&empty), 0);

        let mut core = loaded_core_at_frame_boundary();
        let size = <Core as libretro::Core>::serialize_size(&core);

        assert_eq!(size, core.nes.as_ref().unwrap().serialized_state_size());

        core.nes.as_mut().unwrap().run_frame(InputFrame::default());

        assert_eq!(<Core as libretro::Core>::serialize_size(&core), size);

        <Core as libretro::Core>::unload_game(&mut core);
        assert_eq!(<Core as libretro::Core>::serialize_size(&core), 0);
    }

    #[test]
    fn serialization_callbacks_accept_oversized_buffers_and_reject_short_ones() {
        let mut core = loaded_core_at_frame_boundary();
        let exact = serialized_image(&core);
        let size = exact.len();

        let mut oversized = vec![0xA5; size + 17];
        assert!(<Core as libretro::Core>::serialize(&core, &mut oversized));
        assert_eq!(&oversized[..size], exact);
        assert!(oversized[size..].iter().all(|&byte| byte == 0));
        assert!(<Core as libretro::Core>::unserialize(&mut core, &oversized));

        let mut short = vec![0; size - 1];
        assert!(!<Core as libretro::Core>::serialize(&core, &mut short));
        assert!(!<Core as libretro::Core>::unserialize(&mut core, &short));
    }

    #[test]
    fn serialization_callbacks_restore_and_replay_deterministically() {
        let mut core = loaded_core_at_frame_boundary();
        let origin = serialized_image(&core);

        let inputs = [
            InputFrame::new(
                ControllerButtons::default().with_a(true),
                ControllerButtons::default(),
            ),
            InputFrame::new(
                ControllerButtons::default(),
                ControllerButtons::default().with_start(true),
            ),
        ];

        for &input in &inputs {
            core.nes.as_mut().unwrap().run_frame(input);
        }

        let expected_state = serialized_image(&core);
        let expected_frame = core.nes.as_ref().unwrap().frame().pixels().to_vec();
        let expected_audio = drain_nes_audio(&mut core);

        core.audio.push([123, 123]);
        core.video.fill(0x00AB_CDEF);

        assert!(<Core as libretro::Core>::unserialize(&mut core, &origin));
        assert!(core.audio.is_empty());
        assert!(core.video.iter().all(|&pixel| pixel == 0));

        for &input in &inputs {
            core.nes.as_mut().unwrap().run_frame(input);
        }

        let actual_state = serialized_image(&core);
        let actual_frame = core.nes.as_ref().unwrap().frame().pixels().to_vec();
        let actual_audio = drain_nes_audio(&mut core);

        assert_eq!(actual_state, expected_state);
        assert_eq!(actual_frame, expected_frame);
        assert_eq!(actual_audio, expected_audio);
    }

    #[test]
    fn failed_unserialize_preserves_machine_and_adapter_staging() {
        let mut core = loaded_core_at_frame_boundary();
        let baseline = serialized_image(&core);
        let mut corrupted = baseline.clone();
        corrupted[0] ^= 1;

        core.audio.push([123, 456]);
        core.video.fill(0x0012_3456);

        assert!(!<Core as libretro::Core>::unserialize(
            &mut core, &corrupted
        ));

        assert_eq!(core.audio, [[123, 456]]);
        assert!(core.video.iter().all(|&pixel| pixel == 0x0012_3456));
        assert_eq!(serialized_image(&core), baseline);
    }

    #[test]
    fn cheat_updates_parse_groups_toggle_and_reset_entries() {
        let mut core = Core::default();

        core.update_cheat(7, true, Some("GOSSIP + ZEXPYGLA"));

        assert_eq!(
            core.cheats.get(&7),
            Some(&Cheat {
                enabled: true,
                codes: vec!["GOSSIP".parse().unwrap(), "ZEXPYGLA".parse().unwrap()],
            })
        );

        core.update_cheat(7, false, None);
        assert!(!core.cheats.get(&7).unwrap().enabled);

        <Core as libretro::Core>::cheat_reset(&mut core);
        assert!(core.cheats.is_empty());
    }

    #[test]
    fn invalid_cheat_update_preserves_the_previous_entry() {
        let mut core = Core::default();
        core.update_cheat(3, true, Some("GOSSIP"));
        let previous = core.cheats.get(&3).unwrap().clone();

        core.update_cheat(3, false, Some("INVALID"));

        assert_eq!(core.cheats.get(&3), Some(&previous));
    }

    #[test]
    fn reset_preserves_the_exposed_system_ram_address() {
        let mut core = loaded_core_at_frame_boundary();
        let original_address = core.nes.as_mut().unwrap().system_ram_mut().as_mut_ptr();

        <Core as libretro::Core>::reset(&mut core);

        assert_eq!(
            core.nes.as_mut().unwrap().system_ram_mut().as_mut_ptr(),
            original_address
        );
    }

    #[test]
    fn exposes_system_ram_to_the_frontend() {
        let mut core = loaded_core_at_frame_boundary();

        {
            let memory =
                <Core as libretro::Core>::memory_region(&mut core, MemoryRegion::SystemRam)
                    .unwrap();

            match memory {
                CoreMemory::ReadWrite(ram) => {
                    assert_eq!(ram.len(), 0x0800);
                    ram[0x123] = 0xA5;
                }
                CoreMemory::ReadOnly(_) => panic!("system RAM must be writable"),
            }
        }

        assert_eq!(core.nes.as_ref().unwrap().system_ram()[0x123], 0xA5,);
    }
}
