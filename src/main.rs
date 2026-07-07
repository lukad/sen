use sen::{cartridge::Cartridge, cpu::Cpu, nes_bus::NesCpuBus};

#[cfg(feature = "tracing")]
fn init_tracing() {
    use tracing_subscriber::fmt::format::FmtSpan;

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_span_events(FmtSpan::ENTER | FmtSpan::CLOSE)
        .init();
}

fn main() {
    #[cfg(feature = "tracing")]
    init_tracing();

    let rom_path = std::env::args().nth(1).expect("no rom path provided");
    let rom_data = std::fs::read(&rom_path).expect("failed to read rom");

    let cartridge = Cartridge::from_ines(&rom_data).expect("failed to parse cartridge");
    let mut cpu = Cpu::new();
    let mut bus = NesCpuBus::new(cartridge);

    cpu.reset(&mut bus);

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
