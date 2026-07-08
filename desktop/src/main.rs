use std::{
    error::Error,
    sync::Arc,
    time::{Duration, Instant},
};

use pixels::{Pixels, SurfaceTexture};
use sen_core::{cartridge::Cartridge, controller::ControllerButtons, frame, nes::Nes};
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::{ElementState, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::{Window, WindowId},
};

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

struct App {
    nes: Nes,
    input: InputState,
    window: Option<Arc<Window>>,
    pixels: Option<Pixels<'static>>,
    next_frame: Instant,
    frame_period: Duration,
}

impl App {
    fn new(nes: Nes) -> Self {
        Self {
            nes,
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
        self.nes.set_controller1(self.input.buttons());
        self.nes.run_until_frame();
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
    let rom_path = std::env::args().nth(1).expect("no rom path provided");
    let rom_data = std::fs::read(&rom_path).expect("failed to read rom");

    let cartridge = Cartridge::from_ines(&rom_data).expect("failed to parse cartridge");
    let nes = Nes::new(cartridge);

    let event_loop = EventLoop::new()?;
    let mut app = App::new(nes);
    event_loop.run_app(&mut app)?;

    Ok(())
}
