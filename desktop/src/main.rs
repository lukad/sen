use std::{
    collections::VecDeque,
    error::Error,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use cpal::{
    SampleFormat, StreamConfig,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use pixels::{Pixels, SurfaceTexture};
use sen_core::{
    cartridge::Cartridge,
    controller::ControllerButtons,
    frame,
    nes::{InputFrame, Nes},
};
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::{ElementState, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::{Window, WindowId},
};

const AUDIO_BUFFER_SIZE: usize = 4096;
const NTSC_FRAME_RATE: f64 = 60.0988;

#[derive(Default)]
struct InputState {
    a: bool,
    b: bool,
    select: bool,
    start: bool,
    up: bool,
    down: bool,
    left: bool,
    right: bool,
}

impl InputState {
    fn set_key(&mut self, code: KeyCode, pressed: bool) {
        match code {
            KeyCode::KeyZ => self.a = pressed,
            KeyCode::KeyX => self.b = pressed,
            KeyCode::ShiftLeft => self.select = pressed,
            KeyCode::Enter => self.start = pressed,
            KeyCode::ArrowUp => self.up = pressed,
            KeyCode::ArrowDown => self.down = pressed,
            KeyCode::ArrowLeft => self.left = pressed,
            KeyCode::ArrowRight => self.right = pressed,
            _ => {}
        }
    }

    fn buttons(&self) -> ControllerButtons {
        ControllerButtons::default()
            .with_a(self.a)
            .with_b(self.b)
            .with_select(self.select)
            .with_start(self.start)
            .with_up(self.up && !self.down)
            .with_down(self.down && !self.up)
            .with_left(self.left && !self.right)
            .with_right(self.right && !self.left)
    }
}

struct Audio {
    samples: Arc<Mutex<VecDeque<f32>>>,
    _stream: cpal::Stream,
    sample_rate: f64,
}

fn create_audio() -> Result<Audio, Box<dyn Error>> {
    let host = cpal::default_host();
    let device = host.default_output_device().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "no output audio device")
    })?;

    let supported_config = device.default_output_config()?;
    let sample_format = supported_config.sample_format();
    let sample_rate = supported_config.sample_rate() as f64;
    let config: StreamConfig = supported_config.into();

    let samples = Arc::new(Mutex::new(VecDeque::with_capacity(AUDIO_BUFFER_SIZE)));
    let stream_samples = samples.clone();

    let stream = match sample_format {
        SampleFormat::F32 => build_audio_stream::<f32>(&device, config, stream_samples)?,
        SampleFormat::I16 => build_audio_stream::<i16>(&device, config, stream_samples)?,
        SampleFormat::U16 => build_audio_stream::<u16>(&device, config, stream_samples)?,
        other => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                format!("unsupported ouput sample format {other}"),
            )
            .into());
        }
    };

    stream.play()?;

    Ok(Audio {
        samples,
        _stream: stream,
        sample_rate,
    })
}

fn build_audio_stream<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    samples: Arc<Mutex<VecDeque<f32>>>,
) -> Result<cpal::Stream, cpal::Error>
where
    T: cpal::SizedSample + cpal::FromSample<f32>,
{
    let channels = config.channels as usize;
    let mut last_sample = 0.0;

    device.build_output_stream(
        config,
        move |data: &mut [T], _| write_audio_data(data, channels, &samples, &mut last_sample),
        move |err| eprintln!("audio stream error: {err}"),
        None,
    )
}

fn write_audio_data<T>(
    data: &mut [T],
    channels: usize,
    samples: &Mutex<VecDeque<f32>>,
    last_sample: &mut f32,
) where
    T: cpal::Sample + cpal::FromSample<f32>,
{
    let mut samples = samples.lock().unwrap();

    for frame in data.chunks_mut(channels) {
        let value = samples.pop_front().unwrap_or(*last_sample).clamp(-1.0, 1.0);
        *last_sample = value;
        let value = T::from_sample(value);

        for output in frame {
            *output = value;
        }
    }
}

fn save_path_for_rom(rom_path: &Path) -> PathBuf {
    rom_path.with_extension("sav")
}

fn load_save(nes: &mut Nes, save_path: &Path) -> Result<(), Box<dyn Error>> {
    if nes.save_ram().is_none() {
        return Ok(());
    }

    match std::fs::read(save_path) {
        Ok(data) => nes.load_save_ram(&data)?,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(err.into()),
    }

    Ok(())
}

fn write_save(nes: &Nes, save_path: &Path) -> Result<(), Box<dyn Error>> {
    let Some(data) = nes.save_ram() else {
        return Ok(());
    };

    std::fs::write(save_path, data)?;
    Ok(())
}

struct App {
    nes: Nes,
    audio: Audio,
    input: InputState,
    window: Option<Arc<Window>>,
    pixels: Option<Pixels<'static>>,
    next_frame: Instant,
    frame_period: Duration,
}

impl App {
    fn new(nes: Nes, audio: Audio) -> Self {
        Self {
            nes,
            audio,
            input: InputState::default(),
            window: None,
            pixels: None,
            next_frame: Instant::now(),
            frame_period: Duration::from_secs_f64(1.0 / NTSC_FRAME_RATE),
        }
    }

    fn create_window(&mut self, event_loop: &ActiveEventLoop) {
        let size = LogicalSize::new((frame::WIDTH * 2) as f64, (frame::HEIGHT * 2) as f64);
        let attributes = Window::default_attributes()
            .with_title("SEN")
            .with_inner_size(size)
            .with_resizable(false);

        let window = match event_loop.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(err) => {
                eprintln!("failed to create window: {err}");
                event_loop.exit();
                return;
            }
        };

        let window_size = window.inner_size();
        let surface = SurfaceTexture::new(window_size.width, window_size.height, window.clone());
        let pixels = match Pixels::new(frame::WIDTH as u32, frame::HEIGHT as u32, surface) {
            Ok(pixels) => pixels,
            Err(err) => {
                eprintln!("failed to create pixel surface: {err}");
                event_loop.exit();
                return;
            }
        };

        self.window = Some(window);
        self.pixels = Some(pixels);
        self.next_frame = Instant::now();
    }

    fn run_frame(&mut self) {
        self.nes.run_frame(InputFrame::new(
            self.input.buttons(),
            ControllerButtons::default(),
        ));

        let mut queue = self.audio.samples.lock().unwrap();

        while let Some(sample) = self.nes.pop_audio_sample() {
            queue.push_back(sample);
        }

        while queue.len() > AUDIO_BUFFER_SIZE {
            queue.pop_front();
        }
    }

    fn draw(&mut self, event_loop: &ActiveEventLoop) {
        let Some(pixels) = self.pixels.as_mut() else {
            return;
        };

        copy_frame_to_pixels(self.nes.frame(), pixels.frame_mut());

        if let Err(err) = pixels.render() {
            eprintln!("failed to render frame: {err}");
            event_loop.exit();
        }
    }

    fn resize_surface(&mut self, width: u32, height: u32, event_loop: &ActiveEventLoop) {
        if width == 0 || height == 0 {
            return;
        }

        let Some(pixels) = self.pixels.as_mut() else {
            return;
        };

        if let Err(err) = pixels.resize_surface(width, height) {
            eprintln!("failed to resize pixel surface: {err}");
            event_loop.exit();
        }
    }

    fn schedule_next_frame(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none() {
            event_loop.set_control_flow(ControlFlow::Wait);
            return;
        }

        let now = Instant::now();
        if now >= self.next_frame {
            self.run_frame();
            if let Some(window) = &self.window {
                window.request_redraw();
            }

            self.next_frame += self.frame_period;

            let after_frame = Instant::now();
            if after_frame
                .checked_duration_since(self.next_frame)
                .is_some_and(|late| late > self.frame_period)
            {
                self.next_frame = after_frame + self.frame_period;
            }
        }

        event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_frame));
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none() {
            self.create_window(event_loop);
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        if self
            .window
            .as_ref()
            .is_some_and(|window| window.id() != window_id)
        {
            return;
        }

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::KeyboardInput { event, .. } => {
                let PhysicalKey::Code(code) = event.physical_key else {
                    return;
                };

                let pressed = event.state == ElementState::Pressed;
                if code == KeyCode::Escape && pressed {
                    event_loop.exit();
                    return;
                }

                self.input.set_key(code, pressed);
            }
            WindowEvent::Resized(size) => {
                self.resize_surface(size.width, size.height, event_loop);
            }
            WindowEvent::Focused(false) => {
                self.input = InputState::default();
            }
            WindowEvent::RedrawRequested => {
                self.draw(event_loop);
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.schedule_next_frame(event_loop);
    }
}

fn copy_frame_to_pixels(src: &frame::Frame, dst: &mut [u8]) {
    for (rgba, rgb) in dst.chunks_exact_mut(4).zip(src.pixels().chunks_exact(3)) {
        rgba[0] = rgb[0];
        rgba[1] = rgb[1];
        rgba[2] = rgb[2];
        rgba[3] = 0xFF;
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let rom_path_str = std::env::args_os().nth(1).expect("no rom path provided");
    let rom_path = PathBuf::from(rom_path_str);
    let save_path = save_path_for_rom(&rom_path);
    let rom_data = std::fs::read(&rom_path).expect("failed to read rom");

    let cartridge = Cartridge::from_ines(&rom_data).expect("failed to parse cartridge");
    let audio = create_audio()?;
    let mut nes = Nes::new_with_sample_rate(cartridge, audio.sample_rate);

    load_save(&mut nes, &save_path)?;

    let event_loop = EventLoop::new()?;
    let mut app = App::new(nes, audio);

    let run_result = event_loop.run_app(&mut app).map_err(|e| e.into());
    write_save(&app.nes, &save_path)?;
    run_result
}
