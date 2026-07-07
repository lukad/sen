use std::time::{Duration, Instant};

use minifb::{Key, Scale, Window, WindowOptions};

use sen_core::{cartridge::Cartridge, controller::ControllerButtons, frame, nes::Nes};
use spin_sleep::SpinSleeper;

fn copy_frame_to_minifb(frame: &frame::Frame, buffer: &mut [u32]) {
    for (pixel, rgb) in buffer.iter_mut().zip(frame.pixels().chunks_exact(3)) {
        let r = rgb[0] as u32;
        let g = rgb[1] as u32;
        let b = rgb[2] as u32;

        *pixel = (r << 16) | (g << 8) | b;
    }
}

fn controller1_buttons(window: &Window) -> ControllerButtons {
    let up = window.is_key_down(Key::Up);
    let down = window.is_key_down(Key::Down);
    let left = window.is_key_down(Key::Left);
    let right = window.is_key_down(Key::Right);

    ControllerButtons::default()
        .with_a(window.is_key_down(Key::Z))
        .with_b(window.is_key_down(Key::X))
        .with_select(window.is_key_down(Key::LeftShift))
        .with_start(window.is_key_down(Key::Enter))
        .with_up(up && !down)
        .with_down(down && !up)
        .with_left(left && !right)
        .with_right(right && !left)
}

const NTSC_FRAME_RATE: f64 = 60.0988;

fn main() {
    let rom_path = std::env::args().nth(1).expect("no rom path provided");
    let rom_data = std::fs::read(&rom_path).expect("failed to read rom");

    let cartridge = Cartridge::from_ines(&rom_data).expect("failed to parse cartridge");
    let mut nes = Nes::new(cartridge);

    let mut buffer: Vec<u32> = vec![0; frame::WIDTH * frame::HEIGHT];

    let mut window = Window::new(
        "SEN",
        frame::WIDTH,
        frame::HEIGHT,
        WindowOptions {
            scale: Scale::X2,
            scale_mode: minifb::ScaleMode::AspectRatioStretch,
            ..Default::default()
        },
    )
    .unwrap();

    let frame_period = Duration::from_secs_f64(1.0 / NTSC_FRAME_RATE);
    let sleeper = SpinSleeper::default().with_spin_strategy(spin_sleep::SpinStrategy::YieldThread);
    let mut next_frame = Instant::now();

    while window.is_open() && !window.is_key_down(Key::Escape) {
        nes.set_controller1(controller1_buttons(&window));
        nes.run_until_frame();

        copy_frame_to_minifb(nes.frame(), &mut buffer);
        window
            .update_with_buffer(&buffer, frame::WIDTH, frame::HEIGHT)
            .unwrap();

        next_frame += frame_period;

        let now = Instant::now();
        if next_frame > now {
            sleeper.sleep_until(next_frame);
        } else {
            next_frame = now;
        }
    }
}
