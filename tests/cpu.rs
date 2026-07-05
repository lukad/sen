use test_log::test;

use sen::{bus::Bus, cpu::*, simple_bus::SimpleBus};

fn run_instructions<B: Bus>(cpu: &mut Cpu, bus: &mut B, amount: usize) -> usize {
    let mut cycles = 0;
    for _ in 0..amount {
        cycles += cpu.step_instruction(bus);
    }
    cycles
}

struct RecordingBus {
    inner: SimpleBus,
    writes: Vec<(u16, u8)>,
}

impl RecordingBus {
    fn new() -> Self {
        Self {
            inner: SimpleBus::new(),
            writes: Vec::new(),
        }
    }

    fn load(&mut self, start: u16, data: &[u8]) {
        self.inner.load(start, data);
    }

    fn poke(&mut self, addr: u16, value: u8) {
        self.inner.poke(addr, value);
    }

    fn peek(&self, addr: u16) -> u8 {
        self.inner.peek(addr)
    }
}

impl Bus for RecordingBus {
    fn read(&mut self, address: u16) -> u8 {
        self.inner.read(address)
    }

    fn write(&mut self, address: u16, value: u8) {
        self.writes.push((address, value));
        self.inner.write(address, value);
    }
}

#[test]
fn test_lda_imm() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    bus.load(0x8000, &[0xA9, 0x80]);
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 2);

    assert_eq!(cpu.pc, 0x8002);
    assert_eq!(cpu.a, 0x80);

    assert!(cpu.status.negative);
    assert!(!cpu.status.zero);
}

#[test]
fn lda_imm_sets_zero_flag() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    bus.load(0x8000, &[0xA9, 0x00]); // LDA #$00
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 2);

    assert_eq!(cpu.a, 0x00);
    assert!(cpu.status.zero);
    assert!(!cpu.status.negative);
}

#[test]
fn test_lda_abs() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    bus.load(0x8000, &[0xAD, 0x34, 0x12]);
    bus.poke(0x1234, 0x42);
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 4);

    assert_eq!(cpu.pc, 0x8003);
    assert_eq!(cpu.a, 0x42);
    assert!(!cpu.status.negative);
    assert!(!cpu.status.zero);
}

#[test]
fn test_lda_absx() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    bus.load(0x8000, &[0xBD, 0x34, 0x12]);
    bus.poke(0x1234, 0x42);
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 4);

    assert_eq!(cpu.pc, 0x8003);
    assert_eq!(cpu.a, 0x42);
    assert!(!cpu.status.negative);
    assert!(!cpu.status.zero);
}

#[test]
fn lda_absx_page_cross() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    // Base = $80FF, X = 1 -> target = $8100 (page cross)
    cpu.x = 1;
    bus.load(0x8000, &[0xBD, 0xFF, 0x80]); // LDA $80FF,X
    bus.poke(0x8100, 0x7F);
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 5);

    assert_eq!(cpu.pc, 0x8003);
    assert_eq!(cpu.a, 0x7F);
    assert!(!cpu.status.zero);
    assert!(!cpu.status.negative);
}

#[test]
fn test_lda_absy() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    bus.load(0x8000, &[0xB9, 0x34, 0x12]);
    bus.poke(0x1234, 0x42);
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 4);

    assert_eq!(cpu.pc, 0x8003);
    assert_eq!(cpu.a, 0x42);
    assert!(!cpu.status.negative);
    assert!(!cpu.status.zero);
}

#[test]
fn lda_absy_page_cross() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    // Base = $40FF, Y = 2 -> target = $4101
    cpu.y = 2;
    bus.load(0x8000, &[0xB9, 0xFF, 0x40]); // LDA $40FF,Y
    bus.poke(0x4101, 0x80);
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 5);

    assert_eq!(cpu.pc, 0x8003);
    assert_eq!(cpu.a, 0x80);
    assert!(!cpu.status.zero);
    assert!(cpu.status.negative);
}

#[test]
fn lda_indx_basic() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    // Set up zero-page pointer table:
    // (d + X) & 0xFF = 0x24
    // [0x24] = 0x34 (low), [0x25] = 0x12 (high) => target = 0x1234
    bus.poke(0x0024, 0x34);
    bus.poke(0x0025, 0x12);
    bus.poke(0x1234, 0xAB);

    cpu.x = 0x04;
    bus.load(0x8000, &[0xA1, 0x20]); // LDA ($20,X)
    cpu.pc = 0x8000;

    let cycles = run_instructions(&mut cpu, &mut bus, 1);
    assert_eq!(cycles, 6);

    assert_eq!(cpu.pc, 0x8002);
    assert_eq!(cpu.a, 0xAB);
    assert!(!cpu.status.zero);
    assert!(cpu.status.negative); // 0xAB has bit 7 set
}

#[test]
fn lda_indx_pointer_high_byte_wraps_in_zero_page() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    // (d + X) & 0xFF = 0xFF
    // The pointer high byte should be read from $0000, not $0100.
    bus.poke(0x00FF, 0x34);
    bus.poke(0x0000, 0x12); // wrapped high byte -> target = $1234
    bus.poke(0x0100, 0x99); // wrong high byte if zero-page wrap is missed
    bus.poke(0x1234, 0x42);
    bus.poke(0x9934, 0x00);

    cpu.x = 0x7F;
    bus.load(0x8000, &[0xA1, 0x80]); // LDA ($80,X)
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 6);

    assert_eq!(cpu.pc, 0x8002);
    assert_eq!(cpu.a, 0x42);
    assert!(!cpu.status.zero);
    assert!(!cpu.status.negative);
}

#[test]
fn lda_indy_no_page_cross() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    // Pointer at zp[0x40] = 0x2000
    bus.poke(0x0040, 0x00); // low
    bus.poke(0x0041, 0x20); // high

    cpu.y = 0x05; // target = 0x2005
    bus.poke(0x2005, 0x11);

    bus.load(0x8000, &[0xB1, 0x40]); // LDA ($40),Y
    cpu.pc = 0x8000;

    let cycles = run_instructions(&mut cpu, &mut bus, 1);
    assert_eq!(cycles, 5);

    assert_eq!(cpu.pc, 0x8002);
    assert_eq!(cpu.a, 0x11);
    assert!(!cpu.status.zero);
    assert!(!cpu.status.negative);
}

#[test]
fn lda_indy_page_cross() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    // Pointer at zp[0x50] = 0x20FF
    bus.poke(0x0050, 0xFF); // low
    bus.poke(0x0051, 0x20); // high

    cpu.y = 0x02; // base 0x20FF + 2 = 0x2101 (page cross)
    bus.poke(0x2101, 0x80);

    bus.load(0x8000, &[0xB1, 0x50]); // LDA ($50),Y
    cpu.pc = 0x8000;

    let cycles = run_instructions(&mut cpu, &mut bus, 1);
    assert_eq!(cycles, 6);

    assert_eq!(cpu.pc, 0x8002);
    assert_eq!(cpu.a, 0x80);
    assert!(!cpu.status.zero);
    assert!(cpu.status.negative);
}

#[test]
fn lda_zp() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    bus.load(0x8000, &[0xA5, 0x10]); // LDA $10
    bus.poke(0x0010, 0x42);
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 3);

    assert_eq!(cpu.pc, 0x8002);
    assert_eq!(cpu.a, 0x42);
    assert!(!cpu.status.zero);
    assert!(!cpu.status.negative);
}

#[test]
fn ldx_zpy() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.y = 1;
    bus.load(0x8000, &[0xB6, 0x10]); // LDX $10,Y
    bus.poke(0x0011, 0x01);
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 4);

    assert_eq!(cpu.pc, 0x8002);
    assert_eq!(cpu.x, 0x01);
    assert!(!cpu.status.zero);
    assert!(!cpu.status.negative);
}

#[test]
fn ldy_zpx_is_4_cycles() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.x = 2;
    bus.load(0x8000, &[0xB4, 0x10]); // LDY $10,X
    bus.poke(0x0012, 0x00);
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 4);

    assert_eq!(cpu.pc, 0x8002);
    assert_eq!(cpu.y, 0x00);
    assert!(cpu.status.zero);
    assert!(!cpu.status.negative);
}

#[test]
fn test_ldx_imm() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    bus.load(0x8000, &[0xA2, 0x80]);
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 2);

    assert_eq!(cpu.pc, 0x8002);
    assert_eq!(cpu.x, 0x80);
    assert!(cpu.status.negative);
    assert!(!cpu.status.zero);
}

#[test]
fn test_ldx_abs() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    bus.load(0x8000, &[0xAE, 0x34, 0x12]);
    bus.poke(0x1234, 0x42);
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 4);

    assert_eq!(cpu.pc, 0x8003);
    assert_eq!(cpu.x, 0x42);
    assert!(!cpu.status.negative);
    assert!(!cpu.status.zero);
}

#[test]
fn test_ldx_absy() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    bus.load(0x8000, &[0xBE, 0x34, 0x12]);
    bus.poke(0x1234, 0x42);
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 4);

    assert_eq!(cpu.pc, 0x8003);
    assert_eq!(cpu.x, 0x42);
    assert!(!cpu.status.negative);
    assert!(!cpu.status.zero);
}

#[test]
fn ldx_absy_page_cross() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.y = 3;
    bus.load(0x8000, &[0xBE, 0xFE, 0x20]); // LDX $20FE,Y -> base=$20FE, target=$2101
    bus.poke(0x2101, 0x00);
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 5);

    assert_eq!(cpu.pc, 0x8003);
    assert_eq!(cpu.x, 0x00);
    assert!(cpu.status.zero);
    assert!(!cpu.status.negative);
}

#[test]
fn test_ldy_imm() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    bus.load(0x8000, &[0xA0, 0x80]);
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 2);

    assert_eq!(cpu.pc, 0x8002);
    assert_eq!(cpu.y, 0x80);
    assert!(cpu.status.negative);
    assert!(!cpu.status.zero);
}

#[test]
fn test_ldy_abs() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    bus.load(0x8000, &[0xAC, 0x34, 0x12]);
    bus.poke(0x1234, 0x42);
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 4);

    assert_eq!(cpu.pc, 0x8003);
    assert_eq!(cpu.y, 0x42);
    assert!(!cpu.status.negative);
    assert!(!cpu.status.zero);
}

#[test]
fn test_ldy_absx() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    bus.load(0x8000, &[0xBC, 0x34, 0x12]);
    bus.poke(0x1234, 0x42);
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 4);

    assert_eq!(cpu.pc, 0x8003);
    assert_eq!(cpu.y, 0x42);
    assert!(!cpu.status.negative);
    assert!(!cpu.status.zero);
}

#[test]
fn ldy_absx_page_cross() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.x = 4;
    bus.load(0x8000, &[0xBC, 0xFD, 0x10]); // LDY $10FD,X -> target=$1101
    bus.poke(0x1101, 0xFF);
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 5);

    assert_eq!(cpu.pc, 0x8003);
    assert_eq!(cpu.y, 0xFF);
    assert!(!cpu.status.zero);
    assert!(cpu.status.negative);
}

#[test]
fn bne_not_taken() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.status.zero = true;
    bus.load(0x8000, &[0xD0, 0x05]);
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 2);

    assert_eq!(cpu.pc, 0x8002);
}

#[test]
fn bne_taken_no_page_cross() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.status.zero = false;
    bus.load(0x8000, &[0xD0, 0x05]);
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 3);
    assert_eq!(cpu.pc, 0x8007);
}

#[test]
fn bne_taken_page_cross() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.status.zero = false;
    bus.load(0x80FC, &[0xD0, 0x02]);
    cpu.pc = 0x80FC;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 4);
    assert_eq!(cpu.pc, 0x8100);
}

#[test]
fn bcc_taken_when_carry_clear() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.status.carry = false;
    bus.load(0x8000, &[0x90, 0x05]); // BCC
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 3);
    assert_eq!(cpu.pc, 0x8007);
}

#[test]
fn bcc_not_taken_when_carry_set() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.status.carry = true;
    bus.load(0x8000, &[0x90, 0x05]); // BCC
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 2);
    assert_eq!(cpu.pc, 0x8002);
}

#[test]
fn bcs_taken_when_carry_set() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.status.carry = true;
    bus.load(0x8000, &[0xB0, 0x05]); // BCS
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 3);
    assert_eq!(cpu.pc, 0x8007);
}

#[test]
fn bcs_not_taken_when_carry_clear() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.status.carry = false;
    bus.load(0x8000, &[0xB0, 0x05]); // BCS
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 2);
    assert_eq!(cpu.pc, 0x8002);
}

#[test]
fn beq_taken_when_zero_set() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.status.zero = true;
    bus.load(0x8000, &[0xF0, 0x05]); // BEQ
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 3);
    assert_eq!(cpu.pc, 0x8007);
}

#[test]
fn beq_not_taken_when_zero_clear() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.status.zero = false;
    bus.load(0x8000, &[0xF0, 0x05]); // BEQ
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 2);
    assert_eq!(cpu.pc, 0x8002);
}

#[test]
fn bmi_taken_when_negative_set() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.status.negative = true;
    bus.load(0x8000, &[0x30, 0x05]); // BMI
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 3);
    assert_eq!(cpu.pc, 0x8007);
}

#[test]
fn bmi_not_taken_when_negative_clear() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.status.negative = false;
    bus.load(0x8000, &[0x30, 0x05]); // BMI
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 2);
    assert_eq!(cpu.pc, 0x8002);
}

#[test]
fn bpl_taken_when_negative_clear() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.status.negative = false;
    bus.load(0x8000, &[0x10, 0x05]); // BPL
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 3);
    assert_eq!(cpu.pc, 0x8007);
}

#[test]
fn bpl_not_taken_when_negative_set() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.status.negative = true;
    bus.load(0x8000, &[0x10, 0x05]); // BPL
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 2);
    assert_eq!(cpu.pc, 0x8002);
}

#[test]
fn bvc_taken_when_overflow_clear() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.status.overflow = false;
    bus.load(0x8000, &[0x50, 0x05]); // BVC
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 3);
    assert_eq!(cpu.pc, 0x8007);
}

#[test]
fn bvc_not_taken_when_overflow_set() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.status.overflow = true;
    bus.load(0x8000, &[0x50, 0x05]); // BVC
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 2);
    assert_eq!(cpu.pc, 0x8002);
}

#[test]
fn bvs_taken_when_overflow_set() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.status.overflow = true;
    bus.load(0x8000, &[0x70, 0x05]); // BVS
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 3);
    assert_eq!(cpu.pc, 0x8007);
}

#[test]
fn bvs_not_taken_when_overflow_clear() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.status.overflow = false;
    bus.load(0x8000, &[0x70, 0x05]); // BVS
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 2);
    assert_eq!(cpu.pc, 0x8002);
}

#[test]
fn sta_zp() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.a = 0x42;
    bus.load(0x8000, &[0x85, 0x10]); // STA $10
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 3);

    assert_eq!(cpu.pc, 0x8002);
    assert_eq!(bus.peek(0x0010), 0x42);
    // STA does not touch flags
}

#[test]
fn sta_zpx() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.a = 0x99;
    cpu.x = 0x03;
    bus.load(0x8000, &[0x95, 0x10]); // STA $10,X -> $13
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 4);

    assert_eq!(cpu.pc, 0x8002);
    assert_eq!(bus.peek(0x0013), 0x99);
}

#[test]
fn sta_abs() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.a = 0x7E;
    bus.load(0x8000, &[0x8D, 0x34, 0x12]); // STA $1234
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 4);

    assert_eq!(cpu.pc, 0x8003);
    assert_eq!(bus.peek(0x1234), 0x7E);
}

#[test]
fn sta_absx() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.a = 0x55;
    cpu.x = 0x01;
    bus.load(0x8000, &[0x9D, 0xFF, 0x80]); // STA $80FF,X -> $8100
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 5);

    assert_eq!(cpu.pc, 0x8003);
    assert_eq!(bus.peek(0x8100), 0x55);
}

#[test]
fn sta_absy() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.a = 0xAA;
    cpu.y = 0x02;
    bus.load(0x8000, &[0x99, 0xFE, 0x40]); // STA $40FE,Y -> $4100
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 5);

    assert_eq!(cpu.pc, 0x8003);
    assert_eq!(bus.peek(0x4100), 0xAA);
}

#[test]
fn sta_indx() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    // Zero-page pointer at (d + X) & 0xFF = 0x30
    bus.poke(0x0030, 0x00);
    bus.poke(0x0031, 0x20); // pointer = 0x2000

    cpu.x = 0x10;
    cpu.a = 0x55;
    bus.load(0x8000, &[0x81, 0x20]); // STA ($20,X)
    cpu.pc = 0x8000;

    let cycles = run_instructions(&mut cpu, &mut bus, 1);
    assert_eq!(cycles, 6);

    assert_eq!(cpu.pc, 0x8002);
    assert_eq!(bus.peek(0x2000), 0x55);
}

#[test]
fn sta_indy_no_page_cross() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    // Pointer stored at zp[$60] = $3000
    bus.poke(0x0060, 0x00);
    bus.poke(0x0061, 0x30);

    cpu.y = 0x10; // base + Y = $3000 + $10 = $3010 (no page cross)
    cpu.a = 0x7E;
    cpu.pc = 0x8000;

    bus.load(0x8000, &[0x91, 0x60]);

    let cycles = run_instructions(&mut cpu, &mut bus, 1);
    assert_eq!(cycles, 6);
    assert_eq!(cpu.pc, 0x8002);
    assert_eq!(bus.peek(0x3010), 0x7E);

    assert!(!cpu.status.zero);
    assert!(!cpu.status.negative);
}

#[test]
fn sta_indy_page_cross() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    // Pointer at zp[$70] = $30FF
    bus.poke(0x0070, 0xFF);
    bus.poke(0x0071, 0x30);

    cpu.y = 0x02; // base + Y = $30FF + $02 = $3101 (page cross)
    cpu.a = 0x42;
    cpu.pc = 0x8000;

    // STA ($70),Y
    bus.load(0x8000, &[0x91, 0x70]);

    let cycles = run_instructions(&mut cpu, &mut bus, 1);
    assert_eq!(cycles, 6);
    assert_eq!(cpu.pc, 0x8002);
    assert_eq!(bus.peek(0x3101), 0x42);

    assert!(!cpu.status.zero);
    assert!(!cpu.status.negative);
}

#[test]
fn sta_does_not_modify_flags() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    // Set flags to some known state
    cpu.status.zero = true;
    cpu.status.negative = true;

    cpu.a = 0x11;
    bus.load(0x8000, &[0x8D, 0x00, 0x20]); // STA $2000
    cpu.pc = 0x8000;

    run_instructions(&mut cpu, &mut bus, 1);

    assert!(cpu.status.zero);
    assert!(cpu.status.negative);
}

#[test]
fn stx_zp() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.x = 0x12;
    bus.load(0x8000, &[0x86, 0x20]); // STX $20
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 3);

    assert_eq!(cpu.pc, 0x8002);
    assert_eq!(bus.peek(0x0020), 0x12);
}

#[test]
fn stx_zpy() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.x = 0x34;
    cpu.y = 0x02;
    bus.load(0x8000, &[0x96, 0x10]); // STX $10,Y -> $12
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 4);

    assert_eq!(cpu.pc, 0x8002);
    assert_eq!(bus.peek(0x0012), 0x34);
}

#[test]
fn stx_abs() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.x = 0xFE;
    bus.load(0x8000, &[0x8E, 0x34, 0x12]); // STX $1234
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 4);

    assert_eq!(cpu.pc, 0x8003);
    assert_eq!(bus.peek(0x1234), 0xFE);
}

#[test]
fn stx_does_not_modify_flags() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.status.zero = false;
    cpu.status.negative = true;

    cpu.x = 0xAB;
    bus.load(0x8000, &[0x8E, 0x00, 0x10]); // STX $1000
    cpu.pc = 0x8000;

    run_instructions(&mut cpu, &mut bus, 1);

    assert!(!cpu.status.zero);
    assert!(cpu.status.negative);
}

#[test]
fn sty_zp() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.y = 0x77;
    bus.load(0x8000, &[0x84, 0x30]); // STY $30
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 3);

    assert_eq!(cpu.pc, 0x8002);
    assert_eq!(bus.peek(0x0030), 0x77);
}

#[test]
fn sty_zpx() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.y = 0xCC;
    cpu.x = 0x04;
    bus.load(0x8000, &[0x94, 0x10]); // STY $10,X -> $14
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 4);

    assert_eq!(cpu.pc, 0x8002);
    assert_eq!(bus.peek(0x0014), 0xCC);
}

#[test]
fn sty_abs() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.y = 0x01;
    bus.load(0x8000, &[0x8C, 0x34, 0x12]); // STY $1234
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 4);

    assert_eq!(cpu.pc, 0x8003);
    assert_eq!(bus.peek(0x1234), 0x01);
}

#[test]
fn sty_does_not_modify_flags() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.status.zero = true;
    cpu.status.negative = false;

    cpu.y = 0xEE;
    bus.load(0x8000, &[0x8C, 0x00, 0x40]); // STY $4000
    cpu.pc = 0x8000;

    run_instructions(&mut cpu, &mut bus, 1);

    assert!(cpu.status.zero);
    assert!(!cpu.status.negative);
}

#[test]
fn tax() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.a = 0x80;
    cpu.x = 0x00;
    cpu.status.zero = true;
    cpu.status.negative = false;

    bus.load(0x8000, &[0xAA]); // TAX
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 2);

    assert_eq!(cpu.pc, 0x8001);
    assert_eq!(cpu.x, 0x80);
    assert_eq!(cpu.a, 0x80); // unchanged

    assert!(!cpu.status.zero);
    assert!(cpu.status.negative);
}

#[test]
fn tay() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.a = 0x00;
    cpu.y = 0xFF;
    cpu.status.zero = false;
    cpu.status.negative = true;

    bus.load(0x8000, &[0xA8]); // TAY
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 2);

    assert_eq!(cpu.pc, 0x8001);
    assert_eq!(cpu.y, 0x00);
    assert_eq!(cpu.a, 0x00);

    assert!(cpu.status.zero);
    assert!(!cpu.status.negative);
}

#[test]
fn txa() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.x = 0x01;
    cpu.a = 0xFF;
    cpu.status.zero = true;
    cpu.status.negative = true;

    bus.load(0x8000, &[0x8A]); // TXA
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 2);

    assert_eq!(cpu.pc, 0x8001);
    assert_eq!(cpu.a, 0x01);
    assert_eq!(cpu.x, 0x01);

    assert!(!cpu.status.zero);
    assert!(!cpu.status.negative);
}

#[test]
fn tya() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.y = 0xFF;
    cpu.a = 0x00;
    cpu.status.zero = true;
    cpu.status.negative = false;

    bus.load(0x8000, &[0x98]); // TYA
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 2);

    assert_eq!(cpu.pc, 0x8001);
    assert_eq!(cpu.a, 0xFF);
    assert_eq!(cpu.y, 0xFF);

    assert!(!cpu.status.zero);
    assert!(cpu.status.negative);
}

#[test]
fn tsx() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.sp = 0x80;
    cpu.x = 0x00;
    cpu.status.zero = true;
    cpu.status.negative = false;

    bus.load(0x8000, &[0xBA]); // TSX
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 2);

    assert_eq!(cpu.pc, 0x8001);
    assert_eq!(cpu.x, 0x80);
    assert_eq!(cpu.sp, 0x80);

    assert!(!cpu.status.zero);
    assert!(cpu.status.negative);
}

#[test]
fn txs() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.x = 0x42;
    cpu.sp = 0x00;

    cpu.status.zero = true;
    cpu.status.negative = true;

    bus.load(0x8000, &[0x9A]);
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 2);

    assert_eq!(cpu.pc, 0x8001);
    assert_eq!(cpu.sp, 0x42);
    assert_eq!(cpu.x, 0x42);

    assert!(cpu.status.zero);
    assert!(cpu.status.negative);
}

#[test]
fn clc_clears_carry_flag() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.status = 0xFF.into();

    bus.load(0x8000, &[0x18]); // CLC
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 2);

    assert_eq!(cpu.pc, 0x8001);
    assert_eq!(u8::from(cpu.status), 0b1111_1110);
}

#[test]
fn sec_sets_carry_flag() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.status = 0x00.into();

    bus.load(0x8000, &[0x38]); // SEC
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 2);

    assert_eq!(cpu.pc, 0x8001);
    assert_eq!(u8::from(cpu.status), 0b0010_0001);
}

#[test]
fn cli_clears_interrupt_disable_flag() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.status = 0xFF.into();

    bus.load(0x8000, &[0x58]); // CLI
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 2);

    assert_eq!(cpu.pc, 0x8001);
    assert_eq!(u8::from(cpu.status), 0b1111_1011);
}

#[test]
fn sei_sets_interrupt_disable_flag() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.status = 0x00.into();

    bus.load(0x8000, &[0x78]); // SEI
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 2);

    assert_eq!(cpu.pc, 0x8001);
    assert_eq!(u8::from(cpu.status), 0b0010_0100);
}

#[test]
fn clv_clears_overflow_flag() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.status = 0xFF.into();

    bus.load(0x8000, &[0xB8]); // CLV
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 2);

    assert_eq!(cpu.pc, 0x8001);
    assert_eq!(u8::from(cpu.status), 0b1011_1111);
}

#[test]
fn cld_clears_decimal_mode_flag() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.status = 0xFF.into();

    bus.load(0x8000, &[0xD8]); // CLD
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 2);

    assert_eq!(cpu.pc, 0x8001);
    assert_eq!(u8::from(cpu.status), 0b1111_0111);
}

#[test]
fn sed_sets_decimal_mode_flag() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.status = 0x00.into();

    bus.load(0x8000, &[0xF8]); // SED
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 2);

    assert_eq!(cpu.pc, 0x8001);
    assert_eq!(u8::from(cpu.status), 0b0010_1000);
}

#[test]
fn pha() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.a = 0x42;
    cpu.sp = 0xFD;
    bus.load(0x8000, &[0x48]); // PHA
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 3);

    assert_eq!(cpu.pc, 0x8001);
    assert_eq!(cpu.sp, 0xFC);
    assert_eq!(bus.peek(0x01FD), 0x42);
}

#[test]
fn pla() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.sp = 0xFC;
    bus.poke(0x01FD, 0x80); // value to pull
    bus.load(0x8000, &[0x68]); // PLA
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 4);

    assert_eq!(cpu.pc, 0x8001);
    assert_eq!(cpu.sp, 0xFD);
    assert_eq!(cpu.a, 0x80);
    assert!(!cpu.status.zero);
    assert!(cpu.status.negative);
}

#[test]
fn and_imm_updates_accumulator_and_sets_zero_flag() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.a = 0b1111_0000;
    bus.load(0x8000, &[0x29, 0b0000_1111]); // AND #$0F
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 2);

    assert_eq!(cpu.pc, 0x8002);
    assert_eq!(cpu.a, 0x00);
    assert!(cpu.status.zero);
    assert!(!cpu.status.negative);
}

#[test]
fn ora_imm_updates_accumulator_and_sets_negative_flag() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.a = 0b0000_1111;
    bus.load(0x8000, &[0x09, 0b1000_0000]); // ORA #$80
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 2);

    assert_eq!(cpu.pc, 0x8002);
    assert_eq!(cpu.a, 0b1000_1111);
    assert!(!cpu.status.zero);
    assert!(cpu.status.negative);
}

#[test]
fn eor_imm_updates_accumulator_and_flags() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.a = 0b1010_1010;
    bus.load(0x8000, &[0x49, 0b1111_0000]); // EOR #$F0
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 2);

    assert_eq!(cpu.pc, 0x8002);
    assert_eq!(cpu.a, 0b0101_1010);
    assert!(!cpu.status.zero);
    assert!(!cpu.status.negative);
}

#[test]
fn and_absx_no_page_cross_updates_accumulator() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.a = 0b1111_0000;
    cpu.x = 0x02;
    bus.load(0x8000, &[0x3D, 0x20, 0x12]); // AND $1220,X -> $1222
    bus.poke(0x1222, 0b1100_1010);
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 4);

    assert_eq!(cpu.pc, 0x8003);
    assert_eq!(cpu.a, 0b1100_0000);
    assert!(!cpu.status.zero);
    assert!(cpu.status.negative);
}

#[test]
fn ora_absy_page_cross_updates_accumulator() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.a = 0b0000_0101;
    cpu.y = 0x02;
    bus.load(0x8000, &[0x19, 0xFF, 0x12]); // ORA $12FF,Y -> $1301
    bus.poke(0x1301, 0b0101_0000);
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 5);

    assert_eq!(cpu.pc, 0x8003);
    assert_eq!(cpu.a, 0b0101_0101);
    assert!(!cpu.status.zero);
    assert!(!cpu.status.negative);
}

#[test]
fn eor_indy_updates_accumulator() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.a = 0b1111_0000;
    cpu.y = 0x03;
    bus.poke(0x0040, 0x10);
    bus.poke(0x0041, 0x20); // pointer = $2010
    bus.load(0x8000, &[0x51, 0x40]); // EOR ($40),Y -> $2013
    bus.poke(0x2013, 0b1010_1010);
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 5);

    assert_eq!(cpu.pc, 0x8002);
    assert_eq!(cpu.a, 0b0101_1010);
    assert!(!cpu.status.zero);
    assert!(!cpu.status.negative);
}

#[test]
fn adc_imm_adds_operand_to_accumulator() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.a = 0x10;
    bus.load(0x8000, &[0x69, 0x20]); // ADC #$20
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 2);

    assert_eq!(cpu.pc, 0x8002);
    assert_eq!(cpu.a, 0x30);
    assert!(!cpu.status.carry);
    assert!(!cpu.status.zero);
    assert!(!cpu.status.negative);
    assert!(!cpu.status.overflow);
}

#[test]
fn adc_imm_uses_carry_in_and_sets_carry_out() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.a = 0xFF;
    cpu.status.carry = true;
    bus.load(0x8000, &[0x69, 0x00]); // ADC #$00
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 2);

    assert_eq!(cpu.pc, 0x8002);
    assert_eq!(cpu.a, 0x00);
    assert!(cpu.status.carry);
    assert!(cpu.status.zero);
    assert!(!cpu.status.negative);
    assert!(!cpu.status.overflow);
}

#[test]
fn adc_imm_sets_overflow_when_positive_sum_becomes_negative() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.a = 0x50;
    bus.load(0x8000, &[0x69, 0x50]); // ADC #$50
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 2);

    assert_eq!(cpu.pc, 0x8002);
    assert_eq!(cpu.a, 0xA0);
    assert!(!cpu.status.carry);
    assert!(!cpu.status.zero);
    assert!(cpu.status.negative);
    assert!(cpu.status.overflow);
}

#[test]
fn adc_imm_ignores_decimal_mode_on_nes() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.a = 0x15;
    cpu.status.decimal_mode = true;
    bus.load(0x8000, &[0x69, 0x27]); // Binary ADC result is $3C; BCD would be $42.
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 2);

    assert_eq!(cpu.pc, 0x8002);
    assert_eq!(cpu.a, 0x3C);
    assert!(cpu.status.decimal_mode);
    assert!(!cpu.status.carry);
    assert!(!cpu.status.zero);
    assert!(!cpu.status.negative);
    assert!(!cpu.status.overflow);
}

#[test]
fn adc_zp_adds_memory_operand() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.a = 0x01;
    bus.load(0x8000, &[0x65, 0x10]); // ADC $10
    bus.poke(0x0010, 0x02);
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 3);

    assert_eq!(cpu.pc, 0x8002);
    assert_eq!(cpu.a, 0x03);
    assert!(!cpu.status.carry);
    assert!(!cpu.status.zero);
    assert!(!cpu.status.negative);
    assert!(!cpu.status.overflow);
}

#[test]
fn adc_zpx_wraps_zero_page_address() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.a = 0x01;
    cpu.x = 0x02;
    bus.load(0x8000, &[0x75, 0xFF]); // ADC $FF,X -> $01
    bus.poke(0x0001, 0x02);
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 4);

    assert_eq!(cpu.pc, 0x8002);
    assert_eq!(cpu.a, 0x03);
    assert!(!cpu.status.carry);
    assert!(!cpu.status.zero);
    assert!(!cpu.status.negative);
    assert!(!cpu.status.overflow);
}

#[test]
fn adc_abs_adds_memory_operand() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.a = 0x10;
    bus.load(0x8000, &[0x6D, 0x34, 0x12]); // ADC $1234
    bus.poke(0x1234, 0x0F);
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 4);

    assert_eq!(cpu.pc, 0x8003);
    assert_eq!(cpu.a, 0x1F);
    assert!(!cpu.status.carry);
    assert!(!cpu.status.zero);
    assert!(!cpu.status.negative);
    assert!(!cpu.status.overflow);
}

#[test]
fn adc_absx_no_page_cross_takes_four_cycles() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.a = 0x01;
    cpu.x = 0x02;
    bus.load(0x8000, &[0x7D, 0x20, 0x12]); // ADC $1220,X -> $1222
    bus.poke(0x1222, 0x02);
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 4);

    assert_eq!(cpu.pc, 0x8003);
    assert_eq!(cpu.a, 0x03);
    assert!(!cpu.status.carry);
    assert!(!cpu.status.zero);
    assert!(!cpu.status.negative);
    assert!(!cpu.status.overflow);
}

#[test]
fn adc_absx_page_cross_takes_extra_cycle() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.a = 0x01;
    cpu.x = 0x01;
    bus.load(0x8000, &[0x7D, 0xFF, 0x12]); // ADC $12FF,X -> $1300
    bus.poke(0x1300, 0x02);
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 5);

    assert_eq!(cpu.pc, 0x8003);
    assert_eq!(cpu.a, 0x03);
    assert!(!cpu.status.carry);
    assert!(!cpu.status.zero);
    assert!(!cpu.status.negative);
    assert!(!cpu.status.overflow);
}

#[test]
fn adc_absy_page_cross_takes_extra_cycle() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.a = 0x01;
    cpu.y = 0x02;
    bus.load(0x8000, &[0x79, 0xFF, 0x12]); // ADC $12FF,Y -> $1301
    bus.poke(0x1301, 0x02);
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 5);

    assert_eq!(cpu.pc, 0x8003);
    assert_eq!(cpu.a, 0x03);
    assert!(!cpu.status.carry);
    assert!(!cpu.status.zero);
    assert!(!cpu.status.negative);
    assert!(!cpu.status.overflow);
}

#[test]
fn adc_indx_reads_indexed_zero_page_pointer() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.a = 0x10;
    cpu.x = 0x03;
    bus.load(0x8000, &[0x61, 0xFE]); // ADC ($FE,X), pointer at $01/$02
    bus.poke(0x0001, 0x34);
    bus.poke(0x0002, 0x12);
    bus.poke(0x1234, 0x20);
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 6);

    assert_eq!(cpu.pc, 0x8002);
    assert_eq!(cpu.a, 0x30);
    assert!(!cpu.status.carry);
    assert!(!cpu.status.zero);
    assert!(!cpu.status.negative);
    assert!(!cpu.status.overflow);
}

#[test]
fn adc_indy_no_page_cross_reads_pointer_plus_y() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.a = 0x05;
    cpu.y = 0x03;
    bus.load(0x8000, &[0x71, 0x40]); // ADC ($40),Y -> $2013
    bus.poke(0x0040, 0x10);
    bus.poke(0x0041, 0x20);
    bus.poke(0x2013, 0x06);
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 5);

    assert_eq!(cpu.pc, 0x8002);
    assert_eq!(cpu.a, 0x0B);
    assert!(!cpu.status.carry);
    assert!(!cpu.status.zero);
    assert!(!cpu.status.negative);
    assert!(!cpu.status.overflow);
}

#[test]
fn adc_indy_page_cross_takes_extra_cycle() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.a = 0x01;
    cpu.y = 0x01;
    bus.load(0x8000, &[0x71, 0x40]); // ADC ($40),Y -> $2100
    bus.poke(0x0040, 0xFF);
    bus.poke(0x0041, 0x20);
    bus.poke(0x2100, 0x02);
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 6);

    assert_eq!(cpu.pc, 0x8002);
    assert_eq!(cpu.a, 0x03);
    assert!(!cpu.status.carry);
    assert!(!cpu.status.zero);
    assert!(!cpu.status.negative);
    assert!(!cpu.status.overflow);
}

#[test]
fn sbc_imm_subtracts_operand_when_carry_set() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.a = 0x50;
    cpu.status.carry = true;
    bus.load(0x8000, &[0xE9, 0x10]); // SBC #$10
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 2);

    assert_eq!(cpu.pc, 0x8002);
    assert_eq!(cpu.a, 0x40);
    assert!(cpu.status.carry);
    assert!(!cpu.status.zero);
    assert!(!cpu.status.negative);
    assert!(!cpu.status.overflow);
}

#[test]
fn sbc_imm_uses_borrow_when_carry_clear() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.a = 0x50;
    cpu.status.carry = false;
    bus.load(0x8000, &[0xE9, 0x10]); // SBC #$10
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 2);

    assert_eq!(cpu.pc, 0x8002);
    assert_eq!(cpu.a, 0x3F);
    assert!(cpu.status.carry);
    assert!(!cpu.status.zero);
    assert!(!cpu.status.negative);
    assert!(!cpu.status.overflow);
}

#[test]
fn sbc_imm_clears_carry_when_borrow_needed() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.a = 0x00;
    cpu.status.carry = true;
    bus.load(0x8000, &[0xE9, 0x01]); // SBC #$01
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 2);

    assert_eq!(cpu.pc, 0x8002);
    assert_eq!(cpu.a, 0xFF);
    assert!(!cpu.status.carry);
    assert!(!cpu.status.zero);
    assert!(cpu.status.negative);
    assert!(!cpu.status.overflow);
}

#[test]
fn sbc_imm_sets_overflow_when_positive_minus_negative_becomes_negative() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.a = 0x7F;
    cpu.status.carry = true;
    bus.load(0x8000, &[0xE9, 0xFF]); // 127 - (-1) overflows to $80
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 2);

    assert_eq!(cpu.pc, 0x8002);
    assert_eq!(cpu.a, 0x80);
    assert!(!cpu.status.carry);
    assert!(!cpu.status.zero);
    assert!(cpu.status.negative);
    assert!(cpu.status.overflow);
}

#[test]
fn sbc_imm_ignores_decimal_mode_on_nes() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.a = 0x50;
    cpu.status.carry = true;
    cpu.status.decimal_mode = true;
    bus.load(0x8000, &[0xE9, 0x01]); // Binary SBC result is $4F; BCD would be $49.
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 2);

    assert_eq!(cpu.pc, 0x8002);
    assert_eq!(cpu.a, 0x4F);
    assert!(cpu.status.decimal_mode);
    assert!(cpu.status.carry);
    assert!(!cpu.status.zero);
    assert!(!cpu.status.negative);
    assert!(!cpu.status.overflow);
}

#[test]
fn sbc_zp_subtracts_memory_operand() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.a = 0x10;
    cpu.status.carry = true;
    bus.load(0x8000, &[0xE5, 0x10]); // SBC $10
    bus.poke(0x0010, 0x02);
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 3);

    assert_eq!(cpu.pc, 0x8002);
    assert_eq!(cpu.a, 0x0E);
    assert!(cpu.status.carry);
    assert!(!cpu.status.zero);
    assert!(!cpu.status.negative);
    assert!(!cpu.status.overflow);
}

#[test]
fn sbc_zpx_wraps_zero_page_address() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.a = 0x10;
    cpu.x = 0x02;
    cpu.status.carry = true;
    bus.load(0x8000, &[0xF5, 0xFF]); // SBC $FF,X -> $01
    bus.poke(0x0001, 0x02);
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 4);

    assert_eq!(cpu.pc, 0x8002);
    assert_eq!(cpu.a, 0x0E);
    assert!(cpu.status.carry);
    assert!(!cpu.status.zero);
    assert!(!cpu.status.negative);
    assert!(!cpu.status.overflow);
}

#[test]
fn sbc_abs_subtracts_memory_operand() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.a = 0x20;
    cpu.status.carry = true;
    bus.load(0x8000, &[0xED, 0x34, 0x12]); // SBC $1234
    bus.poke(0x1234, 0x01);
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 4);

    assert_eq!(cpu.pc, 0x8003);
    assert_eq!(cpu.a, 0x1F);
    assert!(cpu.status.carry);
    assert!(!cpu.status.zero);
    assert!(!cpu.status.negative);
    assert!(!cpu.status.overflow);
}

#[test]
fn sbc_absx_no_page_cross_takes_four_cycles() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.a = 0x10;
    cpu.x = 0x02;
    cpu.status.carry = true;
    bus.load(0x8000, &[0xFD, 0x20, 0x12]); // SBC $1220,X -> $1222
    bus.poke(0x1222, 0x02);
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 4);

    assert_eq!(cpu.pc, 0x8003);
    assert_eq!(cpu.a, 0x0E);
    assert!(cpu.status.carry);
    assert!(!cpu.status.zero);
    assert!(!cpu.status.negative);
    assert!(!cpu.status.overflow);
}

#[test]
fn sbc_absx_page_cross_takes_extra_cycle() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.a = 0x10;
    cpu.x = 0x01;
    cpu.status.carry = true;
    bus.load(0x8000, &[0xFD, 0xFF, 0x12]); // SBC $12FF,X -> $1300
    bus.poke(0x1300, 0x02);
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 5);

    assert_eq!(cpu.pc, 0x8003);
    assert_eq!(cpu.a, 0x0E);
    assert!(cpu.status.carry);
    assert!(!cpu.status.zero);
    assert!(!cpu.status.negative);
    assert!(!cpu.status.overflow);
}

#[test]
fn sbc_absy_page_cross_takes_extra_cycle() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.a = 0x10;
    cpu.y = 0x02;
    cpu.status.carry = true;
    bus.load(0x8000, &[0xF9, 0xFF, 0x12]); // SBC $12FF,Y -> $1301
    bus.poke(0x1301, 0x02);
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 5);

    assert_eq!(cpu.pc, 0x8003);
    assert_eq!(cpu.a, 0x0E);
    assert!(cpu.status.carry);
    assert!(!cpu.status.zero);
    assert!(!cpu.status.negative);
    assert!(!cpu.status.overflow);
}

#[test]
fn sbc_indx_reads_indexed_zero_page_pointer() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.a = 0x30;
    cpu.x = 0x03;
    cpu.status.carry = true;
    bus.load(0x8000, &[0xE1, 0xFE]); // SBC ($FE,X), pointer at $01/$02
    bus.poke(0x0001, 0x34);
    bus.poke(0x0002, 0x12);
    bus.poke(0x1234, 0x20);
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 6);

    assert_eq!(cpu.pc, 0x8002);
    assert_eq!(cpu.a, 0x10);
    assert!(cpu.status.carry);
    assert!(!cpu.status.zero);
    assert!(!cpu.status.negative);
    assert!(!cpu.status.overflow);
}

#[test]
fn sbc_indy_no_page_cross_reads_pointer_plus_y() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.a = 0x10;
    cpu.y = 0x03;
    cpu.status.carry = true;
    bus.load(0x8000, &[0xF1, 0x40]); // SBC ($40),Y -> $2013
    bus.poke(0x0040, 0x10);
    bus.poke(0x0041, 0x20);
    bus.poke(0x2013, 0x06);
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 5);

    assert_eq!(cpu.pc, 0x8002);
    assert_eq!(cpu.a, 0x0A);
    assert!(cpu.status.carry);
    assert!(!cpu.status.zero);
    assert!(!cpu.status.negative);
    assert!(!cpu.status.overflow);
}

#[test]
fn sbc_indy_page_cross_takes_extra_cycle() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.a = 0x10;
    cpu.y = 0x01;
    cpu.status.carry = true;
    bus.load(0x8000, &[0xF1, 0x40]); // SBC ($40),Y -> $2100
    bus.poke(0x0040, 0xFF);
    bus.poke(0x0041, 0x20);
    bus.poke(0x2100, 0x02);
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 6);

    assert_eq!(cpu.pc, 0x8002);
    assert_eq!(cpu.a, 0x0E);
    assert!(cpu.status.carry);
    assert!(!cpu.status.zero);
    assert!(!cpu.status.negative);
    assert!(!cpu.status.overflow);
}

#[test]
fn bit_zp_sets_overflow_and_negative_from_operand() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.a = 0xFF;
    bus.load(0x8000, &[0x24, 0x10]); // BIT $10
    bus.poke(0x0010, 0b0100_0000);
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 3);

    assert_eq!(cpu.pc, 0x8002);
    assert_eq!(cpu.a, 0xFF);
    assert!(!cpu.status.zero);
    assert!(cpu.status.overflow);
    assert!(!cpu.status.negative);
}

#[test]
fn cmp_imm_sets_zero_and_carry_when_equal() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.a = 0x42;
    cpu.status.overflow = true;
    bus.load(0x8000, &[0xC9, 0x42]); // CMP #$42
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 2);

    assert_eq!(cpu.pc, 0x8002);
    assert_eq!(cpu.a, 0x42);
    assert!(cpu.status.carry);
    assert!(cpu.status.zero);
    assert!(!cpu.status.negative);
    assert!(cpu.status.overflow);
}

#[test]
fn cmp_imm_clears_carry_and_sets_negative_when_less() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.a = 0x10;
    bus.load(0x8000, &[0xC9, 0x20]); // CMP #$20
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 2);

    assert_eq!(cpu.pc, 0x8002);
    assert_eq!(cpu.a, 0x10);
    assert!(!cpu.status.carry);
    assert!(!cpu.status.zero);
    assert!(cpu.status.negative);
}

#[test]
fn cmp_zp_sets_carry_when_accumulator_is_greater() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.a = 0x80;
    bus.load(0x8000, &[0xC5, 0x10]); // CMP $10
    bus.poke(0x0010, 0x01);
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 3);

    assert_eq!(cpu.pc, 0x8002);
    assert_eq!(cpu.a, 0x80);
    assert!(cpu.status.carry);
    assert!(!cpu.status.zero);
    assert!(!cpu.status.negative);
}

#[test]
fn cmp_zpx_wraps_zero_page_address() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.a = 0x42;
    cpu.x = 0x02;
    bus.load(0x8000, &[0xD5, 0xFF]); // CMP $FF,X -> $01
    bus.poke(0x0001, 0x42);
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 4);

    assert_eq!(cpu.pc, 0x8002);
    assert_eq!(cpu.a, 0x42);
    assert!(cpu.status.carry);
    assert!(cpu.status.zero);
    assert!(!cpu.status.negative);
}

#[test]
fn cmp_abs_sets_negative_when_accumulator_is_less() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.a = 0x00;
    bus.load(0x8000, &[0xCD, 0x34, 0x12]); // CMP $1234
    bus.poke(0x1234, 0x01);
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 4);

    assert_eq!(cpu.pc, 0x8003);
    assert_eq!(cpu.a, 0x00);
    assert!(!cpu.status.carry);
    assert!(!cpu.status.zero);
    assert!(cpu.status.negative);
}

#[test]
fn cmp_absx_page_cross_sets_flags() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.a = 0x80;
    cpu.x = 0x01;
    bus.load(0x8000, &[0xDD, 0xFF, 0x12]); // CMP $12FF,X -> $1300
    bus.poke(0x1300, 0x7F);
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 5);

    assert_eq!(cpu.pc, 0x8003);
    assert_eq!(cpu.a, 0x80);
    assert!(cpu.status.carry);
    assert!(!cpu.status.zero);
    assert!(!cpu.status.negative);
}

#[test]
fn cmp_absy_no_page_cross_sets_flags() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.a = 0x10;
    cpu.y = 0x02;
    bus.load(0x8000, &[0xD9, 0x00, 0x20]); // CMP $2000,Y -> $2002
    bus.poke(0x2002, 0x20);
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 4);

    assert_eq!(cpu.pc, 0x8003);
    assert_eq!(cpu.a, 0x10);
    assert!(!cpu.status.carry);
    assert!(!cpu.status.zero);
    assert!(cpu.status.negative);
}

#[test]
fn cmp_indx_reads_indexed_zero_page_pointer() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.a = 0x30;
    cpu.x = 0x03;
    bus.load(0x8000, &[0xC1, 0xFE]); // CMP ($FE,X), pointer at $01/$02
    bus.poke(0x0001, 0x34);
    bus.poke(0x0002, 0x12);
    bus.poke(0x1234, 0x30);
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 6);

    assert_eq!(cpu.pc, 0x8002);
    assert_eq!(cpu.a, 0x30);
    assert!(cpu.status.carry);
    assert!(cpu.status.zero);
    assert!(!cpu.status.negative);
}

#[test]
fn cmp_indy_no_page_cross_reads_pointer_plus_y() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.a = 0x50;
    cpu.y = 0x03;
    bus.load(0x8000, &[0xD1, 0x40]); // CMP ($40),Y -> $2013
    bus.poke(0x0040, 0x10);
    bus.poke(0x0041, 0x20);
    bus.poke(0x2013, 0x40);
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 5);

    assert_eq!(cpu.pc, 0x8002);
    assert_eq!(cpu.a, 0x50);
    assert!(cpu.status.carry);
    assert!(!cpu.status.zero);
    assert!(!cpu.status.negative);
}

#[test]
fn cmp_indy_page_cross_takes_extra_cycle() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.a = 0x00;
    cpu.y = 0x01;
    bus.load(0x8000, &[0xD1, 0x40]); // CMP ($40),Y -> $2100
    bus.poke(0x0040, 0xFF);
    bus.poke(0x0041, 0x20);
    bus.poke(0x2100, 0x01);
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 6);

    assert_eq!(cpu.pc, 0x8002);
    assert_eq!(cpu.a, 0x00);
    assert!(!cpu.status.carry);
    assert!(!cpu.status.zero);
    assert!(cpu.status.negative);
}

#[test]
fn cpx_imm_sets_zero_and_carry_when_equal() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.x = 0x42;
    bus.load(0x8000, &[0xE0, 0x42]); // CPX #$42
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 2);

    assert_eq!(cpu.pc, 0x8002);
    assert_eq!(cpu.x, 0x42);
    assert!(cpu.status.carry);
    assert!(cpu.status.zero);
    assert!(!cpu.status.negative);
}

#[test]
fn cpx_zp_sets_flags_from_x_minus_operand() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.x = 0x01;
    bus.load(0x8000, &[0xE4, 0x10]); // CPX $10
    bus.poke(0x0010, 0x02);
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 3);

    assert_eq!(cpu.pc, 0x8002);
    assert_eq!(cpu.x, 0x01);
    assert!(!cpu.status.carry);
    assert!(!cpu.status.zero);
    assert!(cpu.status.negative);
}

#[test]
fn cpx_abs_sets_carry_when_x_is_greater() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.x = 0x80;
    bus.load(0x8000, &[0xEC, 0x34, 0x12]); // CPX $1234
    bus.poke(0x1234, 0x01);
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 4);

    assert_eq!(cpu.pc, 0x8003);
    assert_eq!(cpu.x, 0x80);
    assert!(cpu.status.carry);
    assert!(!cpu.status.zero);
    assert!(!cpu.status.negative);
}

#[test]
fn cpy_imm_clears_carry_when_y_is_less() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.y = 0x10;
    bus.load(0x8000, &[0xC0, 0x20]); // CPY #$20
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 2);

    assert_eq!(cpu.pc, 0x8002);
    assert_eq!(cpu.y, 0x10);
    assert!(!cpu.status.carry);
    assert!(!cpu.status.zero);
    assert!(cpu.status.negative);
}

#[test]
fn cpy_zp_sets_zero_and_carry_when_equal() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.y = 0x42;
    bus.load(0x8000, &[0xC4, 0x10]); // CPY $10
    bus.poke(0x0010, 0x42);
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 3);

    assert_eq!(cpu.pc, 0x8002);
    assert_eq!(cpu.y, 0x42);
    assert!(cpu.status.carry);
    assert!(cpu.status.zero);
    assert!(!cpu.status.negative);
}

#[test]
fn cpy_abs_sets_carry_when_y_is_greater() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.y = 0x20;
    bus.load(0x8000, &[0xCC, 0x34, 0x12]); // CPY $1234
    bus.poke(0x1234, 0x10);
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 4);

    assert_eq!(cpu.pc, 0x8003);
    assert_eq!(cpu.y, 0x20);
    assert!(cpu.status.carry);
    assert!(!cpu.status.zero);
    assert!(!cpu.status.negative);
}

#[test]
fn inx_wraps_to_zero_and_sets_zero_flag() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.x = 0xFF;
    cpu.status.carry = true;
    bus.load(0x8000, &[0xE8]); // INX
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 2);

    assert_eq!(cpu.pc, 0x8001);
    assert_eq!(cpu.x, 0x00);
    assert!(cpu.status.zero);
    assert!(!cpu.status.negative);
    assert!(cpu.status.carry);
}

#[test]
fn iny_sets_negative_flag() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.y = 0x7F;
    cpu.status.carry = true;
    bus.load(0x8000, &[0xC8]); // INY
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 2);

    assert_eq!(cpu.pc, 0x8001);
    assert_eq!(cpu.y, 0x80);
    assert!(!cpu.status.zero);
    assert!(cpu.status.negative);
    assert!(cpu.status.carry);
}

#[test]
fn dex_sets_zero_flag() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.x = 0x01;
    cpu.status.carry = true;
    bus.load(0x8000, &[0xCA]); // DEX
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 2);

    assert_eq!(cpu.pc, 0x8001);
    assert_eq!(cpu.x, 0x00);
    assert!(cpu.status.zero);
    assert!(!cpu.status.negative);
    assert!(cpu.status.carry);
}

#[test]
fn dey_wraps_to_negative() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.y = 0x00;
    cpu.status.carry = true;
    bus.load(0x8000, &[0x88]); // DEY
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 2);

    assert_eq!(cpu.pc, 0x8001);
    assert_eq!(cpu.y, 0xFF);
    assert!(!cpu.status.zero);
    assert!(cpu.status.negative);
    assert!(cpu.status.carry);
}

#[test]
fn inc_zp_increments_memory_and_sets_zero_flag() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.status.carry = true;
    bus.load(0x8000, &[0xE6, 0x10]); // INC $10
    bus.poke(0x0010, 0xFF);
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 5);

    assert_eq!(cpu.pc, 0x8002);
    assert_eq!(bus.peek(0x0010), 0x00);
    assert!(cpu.status.zero);
    assert!(!cpu.status.negative);
    assert!(cpu.status.carry);
}

#[test]
fn inc_zpx_wraps_zero_page_address() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.x = 0x02;
    bus.load(0x8000, &[0xF6, 0xFF]); // INC $FF,X -> $01
    bus.poke(0x0001, 0x7F);
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 6);

    assert_eq!(cpu.pc, 0x8002);
    assert_eq!(bus.peek(0x0001), 0x80);
    assert!(!cpu.status.zero);
    assert!(cpu.status.negative);
}

#[test]
fn inc_abs_increments_memory_and_sets_zero_flag() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.status.carry = true;
    bus.load(0x8000, &[0xEE, 0x34, 0x12]); // INC $1234
    bus.poke(0x1234, 0xFF);
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 6);

    assert_eq!(cpu.pc, 0x8003);
    assert_eq!(bus.peek(0x1234), 0x00);
    assert!(cpu.status.zero);
    assert!(!cpu.status.negative);
    assert!(cpu.status.carry);
}

#[test]
fn inc_absx_increments_memory_and_sets_negative_flag() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.x = 0x01;
    bus.load(0x8000, &[0xFE, 0xFF, 0x12]); // INC $12FF,X -> $1300
    bus.poke(0x1300, 0x7F);
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 7);

    assert_eq!(cpu.pc, 0x8003);
    assert_eq!(bus.peek(0x1300), 0x80);
    assert!(!cpu.status.zero);
    assert!(cpu.status.negative);
}

#[test]
fn dec_zp_decrements_memory_and_sets_negative_flag() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.status.carry = true;
    bus.load(0x8000, &[0xC6, 0x10]); // DEC $10
    bus.poke(0x0010, 0x00);
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 5);

    assert_eq!(cpu.pc, 0x8002);
    assert_eq!(bus.peek(0x0010), 0xFF);
    assert!(!cpu.status.zero);
    assert!(cpu.status.negative);
    assert!(cpu.status.carry);
}

#[test]
fn dec_zp_writes_old_value_then_decremented_value() {
    let mut cpu = Cpu::new();
    let mut bus = RecordingBus::new();

    bus.load(0x8000, &[0xC6, 0x10]); // DEC $10
    bus.poke(0x0010, 0x42);
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 5);

    assert_eq!(cpu.pc, 0x8002);
    assert_eq!(bus.peek(0x0010), 0x41);
    assert_eq!(bus.writes, vec![(0x0010, 0x42), (0x0010, 0x41)]);
}

#[test]
fn dec_zpx_wraps_zero_page_address() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.x = 0x02;
    bus.load(0x8000, &[0xD6, 0xFF]); // DEC $FF,X -> $01
    bus.poke(0x0001, 0x01);
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 6);

    assert_eq!(cpu.pc, 0x8002);
    assert_eq!(bus.peek(0x0001), 0x00);
    assert!(cpu.status.zero);
    assert!(!cpu.status.negative);
}

#[test]
fn dec_absx_writes_old_value_then_decremented_value() {
    let mut cpu = Cpu::new();
    let mut bus = RecordingBus::new();

    cpu.x = 0x01;
    bus.load(0x8000, &[0xDE, 0xFF, 0x12]); // DEC $12FF,X -> $1300
    bus.poke(0x1300, 0x80);
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 7);

    assert_eq!(cpu.pc, 0x8003);
    assert_eq!(bus.peek(0x1300), 0x7F);
    assert_eq!(bus.writes, vec![(0x1300, 0x80), (0x1300, 0x7F)]);
}

#[test]
fn dec_abs_decrements_memory_and_sets_negative_flag() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.status.carry = true;
    bus.load(0x8000, &[0xCE, 0x34, 0x12]); // DEC $1234
    bus.poke(0x1234, 0x00);
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 6);

    assert_eq!(cpu.pc, 0x8003);
    assert_eq!(bus.peek(0x1234), 0xFF);
    assert!(!cpu.status.zero);
    assert!(cpu.status.negative);
    assert!(cpu.status.carry);
}

#[test]
fn dec_absx_decrements_memory_and_sets_zero_flag() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.x = 0x01;
    bus.load(0x8000, &[0xDE, 0xFF, 0x12]); // DEC $12FF,X -> $1300
    bus.poke(0x1300, 0x01);
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 7);

    assert_eq!(cpu.pc, 0x8003);
    assert_eq!(bus.peek(0x1300), 0x00);
    assert!(cpu.status.zero);
    assert!(!cpu.status.negative);
}

#[test]
fn jmp_abs() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    bus.load(0x8000, &[0x4C, 0x10, 0x20]); // JMP $2010
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 3);

    assert_eq!(cpu.pc, 0x2010);
}

#[test]
fn jmp_ind() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    bus.load(0x8000, &[0x6C, 0x00, 0x20]); // JMP ($2000)
    bus.poke(0x2000, 0x34);
    bus.poke(0x2001, 0x12);
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 5);

    assert_eq!(cpu.pc, 0x1234);
}

#[test]
fn jmp_ind_wraps_pointer_high_byte_on_page_boundary() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    bus.load(0x8000, &[0x6C, 0xFF, 0x20]); // JMP ($20FF)
    bus.poke(0x20FF, 0x34);
    bus.poke(0x2000, 0x12); // wrapped high byte -> target = $1234
    bus.poke(0x2100, 0x99); // wrong high byte if page wrap is missed
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 5);

    assert_eq!(cpu.pc, 0x1234);
}

#[test]
fn jsr_abs_pushes_return_address_and_jumps() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.status.zero = true;
    cpu.status.negative = true;
    bus.load(0x8000, &[0x20, 0x34, 0x12]); // JSR $1234
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 6);

    assert_eq!(cpu.pc, 0x1234);
    assert_eq!(cpu.sp, 0xFB);
    assert_eq!(bus.peek(0x01FD), 0x80);
    assert_eq!(bus.peek(0x01FC), 0x02);
    assert!(cpu.status.zero);
    assert!(cpu.status.negative);
}

#[test]
fn jsr_abs_pushes_return_address_across_page_boundary() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.sp = 0x10;
    bus.load(0x80FE, &[0x20, 0x78, 0x56]); // JSR $5678
    cpu.pc = 0x80FE;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 6);

    assert_eq!(cpu.pc, 0x5678);
    assert_eq!(cpu.sp, 0x0E);
    assert_eq!(bus.peek(0x0110), 0x81);
    assert_eq!(bus.peek(0x010F), 0x00);
}

#[test]
fn rts_pulls_return_address_and_increments_pc() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.sp = 0xFB;
    cpu.status.zero = true;
    cpu.status.negative = true;
    bus.poke(0x01FC, 0x02);
    bus.poke(0x01FD, 0x80); // pulled address = $8002, final PC = $8003
    bus.load(0x1234, &[0x60]); // RTS
    cpu.pc = 0x1234;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 6);

    assert_eq!(cpu.pc, 0x8003);
    assert_eq!(cpu.sp, 0xFD);
    assert!(cpu.status.zero);
    assert!(cpu.status.negative);
}

#[test]
fn rts_increments_pulled_address_across_page_boundary() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.sp = 0x0E;
    bus.poke(0x010F, 0xFF);
    bus.poke(0x0110, 0x80); // pulled address = $80FF, final PC = $8100
    bus.load(0x5678, &[0x60]); // RTS
    cpu.pc = 0x5678;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 6);

    assert_eq!(cpu.pc, 0x8100);
    assert_eq!(cpu.sp, 0x10);
}

#[test]
fn rti_pulls_status_and_pc() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.sp = 0xFA;
    cpu.status.carry = false;
    cpu.status.zero = false;
    cpu.status.interrupt_disable = false;
    cpu.status.overflow = false;
    cpu.status.negative = false;

    bus.poke(0x01FB, 0b1100_0111);
    bus.poke(0x01FC, 0x23);
    bus.poke(0x01FD, 0x81); // pulled PC = $8123
    bus.load(0x4000, &[0x40]); // RTI
    cpu.pc = 0x4000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 6);

    assert_eq!(cpu.pc, 0x8123);
    assert_eq!(cpu.sp, 0xFD);
    assert!(cpu.status.carry);
    assert!(cpu.status.zero);
    assert!(cpu.status.interrupt_disable);
    assert!(!cpu.status.decimal_mode);
    assert!(!cpu.status.break_command);
    assert!(cpu.status.overflow);
    assert!(cpu.status.negative);
}

#[test]
fn rti_does_not_increment_restored_pc() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.sp = 0xFA;
    bus.poke(0x01FB, 0x00);
    bus.poke(0x01FC, 0xFF);
    bus.poke(0x01FD, 0x80); // pulled PC = $80FF, with no RTS-style increment
    bus.load(0x4000, &[0x40]); // RTI
    cpu.pc = 0x4000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 6);

    assert_eq!(cpu.pc, 0x80FF);
    assert_eq!(cpu.sp, 0xFD);
}

#[test]
fn brk_pushes_pc_plus_two_and_status_then_loads_irq_vector() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    cpu.status.carry = true;
    cpu.status.zero = false;
    cpu.status.interrupt_disable = false;
    cpu.status.decimal_mode = true;
    cpu.status.break_command = false;
    cpu.status.overflow = true;
    cpu.status.negative = false;

    bus.load(0x8000, &[0x00, 0xEA]); // BRK, signature byte
    bus.poke(0xFFFE, 0x34);
    bus.poke(0xFFFF, 0x12);
    cpu.pc = 0x8000;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 7);

    assert_eq!(cpu.pc, 0x1234);
    assert_eq!(cpu.sp, 0xFA);
    assert_eq!(bus.peek(0x01FD), 0x80);
    assert_eq!(bus.peek(0x01FC), 0x02);
    assert_eq!(bus.peek(0x01FB), 0b0111_1001);
    assert!(cpu.status.interrupt_disable);
    assert!(cpu.status.carry);
    assert!(!cpu.status.zero);
    assert!(cpu.status.overflow);
    assert!(!cpu.status.negative);
}

#[test]
fn brk_pushes_pc_plus_two_across_page_boundary() {
    let mut cpu = Cpu::new();
    let mut bus = SimpleBus::new();

    bus.load(0x80FF, &[0x00, 0xEA]); // BRK, signature byte
    bus.poke(0xFFFE, 0x78);
    bus.poke(0xFFFF, 0x56);
    cpu.pc = 0x80FF;

    assert_eq!(run_instructions(&mut cpu, &mut bus, 1), 7);

    assert_eq!(cpu.pc, 0x5678);
    assert_eq!(cpu.sp, 0xFA);
    assert_eq!(bus.peek(0x01FD), 0x81);
    assert_eq!(bus.peek(0x01FC), 0x01);
}
