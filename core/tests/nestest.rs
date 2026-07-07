use sen_core::{cartridge::Cartridge, cpu::Cpu, nes_bus::NesCpuBus};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CpuSnapshot {
    pc: u16,
    a: u8,
    x: u8,
    y: u8,
    p: u8,
    sp: u8,
    cycles: usize,
}

impl CpuSnapshot {
    fn from_cpu(cpu: &Cpu, cycles: usize) -> Self {
        Self {
            pc: cpu.pc,
            a: cpu.a,
            x: cpu.x,
            y: cpu.y,
            p: cpu.status.into(),
            sp: cpu.sp,
            cycles,
        }
    }

    fn parse(line: &str) -> Self {
        Self {
            pc: u16::from_str_radix(&line[0..4], 16).unwrap(),
            a: hex_u8_after(line, "A:"),
            x: hex_u8_after(line, " X:"),
            y: hex_u8_after(line, " Y:"),
            p: hex_u8_after(line, " P:"),
            sp: hex_u8_after(line, "SP:"),
            cycles: dec_after(line, "CYC:"),
        }
    }
}

fn hex_u8_after(line: &str, label: &str) -> u8 {
    let start = line.find(label).unwrap() + label.len();
    u8::from_str_radix(&line[start..start + 2], 16).unwrap()
}

fn dec_after(line: &str, label: &str) -> usize {
    let start = line.find(label).unwrap() + label.len();
    line[start..].trim().parse().unwrap()
}

#[test]
fn nestest_matches_reference_log() {
    let rom = include_bytes!("fixtures/nestest.nes");
    let log = include_str!("fixtures/nestest.log");

    let cartridge = Cartridge::from_ines(rom).unwrap();
    let mut bus = NesCpuBus::new(cartridge);
    let mut cpu = Cpu::new();

    cpu.reset(&mut bus);
    cpu.pc = 0xC000;

    let mut cycles = 7;

    for (step, line) in log.lines().enumerate() {
        let expected = CpuSnapshot::parse(line);
        let actual = CpuSnapshot::from_cpu(&cpu, cycles);

        assert_eq!(actual, expected, "nestest mismatch at step {step}: {line}");

        if line.contains('*') {
            break;
        }

        cycles += cpu.step_instruction(&mut bus);
    }
}
