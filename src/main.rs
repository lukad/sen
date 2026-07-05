use sen::{cpu::Cpu, simple_bus::SimpleBus};

fn main() {
    #[cfg(feature = "tracing")]
    tracing_subscriber::fmt::init();

    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    bus.load(
        0x8000,
        &[
            0xA9, 0x42, 0xAD, 0x34, 0x12, 0xBD, 0x34, 0x12, 0xB9, 0x34, 0x12, 0xA5, 0x34, 0xB5,
            0x34, 0xA2, 0x42, 0xAE, 0x34, 0x12,
        ],
    );

    bus.poke(0x1234, 0x99);
    cpu.pc = 0x8000;

    #[cfg(feature = "tracing")]
    let mut cycle = 0;
    loop {
        #[cfg(feature = "tracing")]
        let _span = tracing::trace_span!("tick", cycle).entered();
        cpu.tick(&mut bus);
        #[cfg(feature = "tracing")]
        {
            cycle += 1;
            _span.exit();
        }
    }
}
