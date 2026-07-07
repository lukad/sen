use minifb::{Key, Window, WindowOptions};

use sen_core::{cartridge::Cartridge, cpu::Cpu, frame, nes_bus::NesCpuBus};

fn copy_frame_to_minifb(frame: &frame::Frame, buffer: &mut [u32]) {
    for (pixel, rgb) in buffer.iter_mut().zip(frame.pixels().chunks_exact(3)) {
        let r = rgb[0] as u32;
        let g = rgb[1] as u32;
        let b = rgb[2] as u32;

        *pixel = (r << 16) | (g << 8) | b;
    }
}

fn main() {
    let rom_path = std::env::args().nth(1).expect("no rom path provided");
    let rom_data = std::fs::read(&rom_path).expect("failed to read rom");

    let cartridge = Cartridge::from_ines(&rom_data).expect("failed to parse cartridge");
    let mut cpu = Cpu::new();
    let mut bus = NesCpuBus::new(cartridge);

    cpu.reset(&mut bus);

    let mut buffer: Vec<u32> = vec![0; frame::WIDTH * frame::HEIGHT];

    let mut window =
        Window::new("SEN", frame::WIDTH, frame::HEIGHT, WindowOptions::default()).unwrap();

    window.set_target_fps(60);

    while window.is_open() && !window.is_key_down(Key::Escape) {
        let mut frame_complete = false;

        while !frame_complete {
            let done = cpu.tick(&mut bus);
            frame_complete = bus.tick_after_cpu_cycle();

            if done {
                if bus.take_nmi() {
                    cpu.start_nmi();
                } else if bus.irq_asserted() && !cpu.status.interrupt_disable {
                    cpu.start_irq();
                }
            }
        }

        copy_frame_to_minifb(bus.frame(), &mut buffer);

        window
            .update_with_buffer(&buffer, frame::WIDTH, frame::HEIGHT)
            .unwrap();
    }
}
