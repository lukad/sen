use crate::{
    bus::Bus,
    microcode::{self, AluOp, AluSrc, BranchCond, MicroOp, Reg},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MicroFlow {
    /// Continue execution of the current instruction
    Continue,
    /// End the current instruction
    EndInstruction,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Status {
    pub carry: bool,
    pub zero: bool,
    pub interrupt_disable: bool,
    pub decimal_mode: bool,
    pub break_command: bool,
    pub overflow: bool,
    pub negative: bool,
}

impl std::fmt::Debug for Status {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let binary: u8 = (*self).into();
        write!(f, "{:#010b}", binary)
    }
}

impl From<u8> for Status {
    fn from(byte: u8) -> Self {
        Status {
            carry: byte & 0x01 != 0,
            zero: byte & 0x02 != 0,
            interrupt_disable: byte & 0x04 != 0,
            decimal_mode: byte & 0x08 != 0,
            break_command: byte & 0x10 != 0,
            overflow: byte & 0x40 != 0,
            negative: byte & 0x80 != 0,
        }
    }
}

impl From<Status> for u8 {
    fn from(val: Status) -> Self {
        let mut byte = 0;
        if val.carry {
            byte |= 0x01;
        }
        if val.zero {
            byte |= 0x02;
        }
        if val.interrupt_disable {
            byte |= 0x04;
        }
        if val.decimal_mode {
            byte |= 0x08;
        }
        if val.break_command {
            byte |= 0x10;
        }
        byte |= 0x20;
        if val.overflow {
            byte |= 0x40;
        }
        if val.negative {
            byte |= 0x80;
        }
        byte
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum CpuState {
    Fetch,
    Exec { code: &'static [MicroOp], ip: usize },
}

#[derive(Debug)]
pub struct Cpu {
    pub a: u8,
    pub x: u8,
    pub y: u8,
    pub sp: u8,
    pub pc: u16,
    pub status: Status,

    opcode: u8,
    addr_lo: u8,
    addr_hi: u8,
    eff_addr: u16,
    branch_target: u16,
    branch_page_cross: bool,

    state: CpuState,
}

impl Cpu {
    pub fn new() -> Self {
        Self {
            a: 0,
            x: 0,
            y: 0,
            pc: 0,
            sp: 0xFD,
            status: 0x24.into(),

            opcode: 0,
            addr_lo: 0,
            addr_hi: 0,
            eff_addr: 0,
            branch_target: 0,
            branch_page_cross: false,

            state: CpuState::Fetch,
        }
    }

    pub fn tick<B: Bus>(&mut self, bus: &mut B) -> bool {
        let exec = std::mem::replace(&mut self.state, CpuState::Fetch);
        let mut finished = false;

        #[cfg(feature = "tracing")]
        let _span = {
            let cpu_state = match exec {
                CpuState::Fetch => "fetch",
                CpuState::Exec { .. } => "exec",
            };
            tracing::trace_span!("cpu", state = cpu_state).entered()
        };

        match exec {
            CpuState::Fetch => {
                let op = bus.read(self.pc);
                self.pc = self.pc.wrapping_add(1);
                self.opcode = op;
                let code = microcode::decode(op);
                self.state = CpuState::Exec { code, ip: 0 };
            }
            CpuState::Exec { code, mut ip } => {
                let op = code[ip];
                #[cfg(feature = "tracing")]
                let _span = tracing::trace_span!(
                    "micro_op",
                    ins = %format!("{:#04X?}", self.opcode),
                    ip = ip,
                    op = ?op
                )
                .entered();
                ip += 1;
                match self.exec_micro_op(bus, op) {
                    MicroFlow::EndInstruction => finished = true,
                    MicroFlow::Continue => {
                        if ip >= code.len() {
                            finished = true;
                        } else {
                            self.state = CpuState::Exec { code, ip };
                        }
                    }
                }
            }
        }

        finished
    }

    #[allow(unused)]
    pub fn step_instruction<B: Bus>(&mut self, bus: &mut B) -> usize {
        let mut cycle = 0;
        loop {
            #[cfg(feature = "tracing")]
            let span = tracing::trace_span!("tick", cycle).entered();
            if self.tick(bus) {
                break;
            }
            cycle += 1;
            #[cfg(feature = "tracing")]
            span.exit();
        }
        cycle + 1
    }

    fn reg(&self, reg: Reg) -> u8 {
        match reg {
            Reg::A => self.a,
            Reg::X => self.x,
            Reg::Y => self.y,
            Reg::SP => self.sp,
        }
    }

    fn reg_mut(&mut self, reg: Reg) -> &mut u8 {
        match reg {
            Reg::A => &mut self.a,
            Reg::X => &mut self.x,
            Reg::Y => &mut self.y,
            Reg::SP => &mut self.sp,
        }
    }

    fn stack_addr(&self) -> u16 {
        0x0100 | self.sp as u16
    }

    fn update_nz_flags(&mut self, value: u8) {
        self.status.zero = value == 0;
        self.status.negative = value & 0x80 != 0;
    }

    fn eval_branch_cond(&self, cond: BranchCond) -> bool {
        match cond {
            BranchCond::ZeroSet => self.status.zero,
            BranchCond::ZeroClear => !self.status.zero,
            BranchCond::NegativeSet => self.status.negative,
            BranchCond::NegativeClear => !self.status.negative,
            BranchCond::CarrySet => self.status.carry,
            BranchCond::CarryClear => !self.status.carry,
            BranchCond::OverflowSet => self.status.overflow,
            BranchCond::OverflowClear => !self.status.overflow,
        }
    }

    fn eval_alu_op(&mut self, op: AluOp, value: u8) {
        let result = match op {
            AluOp::And => self.a & value,
            AluOp::Ora => self.a | value,
            AluOp::Eor => self.a ^ value,
            AluOp::Bit => {
                let result = self.a & value;
                self.status.zero = result == 0;
                self.status.overflow = value & 0x40 != 0;
                self.status.negative = value & 0x80 != 0;
                return;
            }
        };
        self.a = result;
        self.update_nz_flags(result);
    }

    fn exec_micro_op<B: Bus>(&mut self, bus: &mut B, op: MicroOp) -> MicroFlow {
        match op {
            MicroOp::ReadPcToRegSetNZ(reg) => {
                let value = bus.read(self.pc);
                self.pc = self.pc.wrapping_add(1);
                *self.reg_mut(reg) = value;
                self.update_nz_flags(value);
            }
            MicroOp::ReadPcToAddrLo => {
                self.addr_lo = bus.read(self.pc);
                self.pc = self.pc.wrapping_add(1);
            }
            MicroOp::ReadPcToAddrHi => {
                self.addr_hi = bus.read(self.pc);
                self.pc = self.pc.wrapping_add(1);
                self.eff_addr = ((self.addr_hi as u16) << 8) | self.addr_lo as u16;
            }
            MicroOp::ReadPcToAddrHiSetPc => {
                self.addr_hi = bus.read(self.pc);
                self.pc = ((self.addr_hi as u16) << 8) | self.addr_lo as u16;
                return MicroFlow::EndInstruction;
            }
            MicroOp::ReadJmpIndirectAddrHiAndJump => {
                let ptr = self.eff_addr;
                self.addr_hi = bus.read((ptr & 0xFF00) | (ptr.wrapping_add(1) & 0x00FF));
                self.pc = ((self.addr_hi as u16) << 8) | self.addr_lo as u16;
                return MicroFlow::EndInstruction;
            }
            MicroOp::ReadEffAddrToRegSetNZ(reg) => {
                let value = bus.read(self.eff_addr);
                *self.reg_mut(reg) = value;
                self.update_nz_flags(value);
            }
            MicroOp::ReadEffAddrToAddrLo => {
                self.addr_lo = bus.read(self.eff_addr);
            }
            MicroOp::ReadEffAddrToAddrHi => {
                self.addr_hi = bus.read((self.eff_addr as u8).wrapping_add(1) as u16);
                self.eff_addr = ((self.addr_hi as u16) << 8) | self.addr_lo as u16;
            }
            MicroOp::ReadZpPtrLoToAddrLo => {
                let zp = self.addr_lo;
                let lo = bus.read(zp as u16);
                self.eff_addr = zp as u16;
                self.addr_lo = lo;
            }
            MicroOp::ReadZpPtrHiToAddrHi => {
                let zp = self.eff_addr as u8;
                let hi = bus.read(zp.wrapping_add(1) as u16);
                self.addr_hi = hi;
                self.eff_addr = ((self.addr_hi as u16) << 8) | self.addr_lo as u16;
            }
            MicroOp::ReadZpAddrToRegSetNZ(reg) => {
                let addr = self.addr_lo as u16;
                let value = bus.read(addr);
                *self.reg_mut(reg) = value;
                self.update_nz_flags(value);
            }
            MicroOp::WriteRegToEffAddr(reg) => {
                bus.write(self.eff_addr, self.reg(reg));
            }
            MicroOp::CopyRegToRegSetNZ(src, dest) => {
                let value = self.reg(src);
                *self.reg_mut(dest) = value;
                self.update_nz_flags(value);
            }
            MicroOp::CopyRegToReg(src, dest) => {
                *self.reg_mut(dest) = self.reg(src);
            }
            MicroOp::ZpIndexedDummyReadAndCompute(index) => {
                let base = self.addr_lo;
                let _dummy = bus.read(base as u16);
                let idx = self.reg(index);
                let addr = base.wrapping_add(idx);
                self.eff_addr = addr as u16;
            }
            MicroOp::WriteRegToZpAddr(reg) => {
                bus.write(self.addr_lo as u16, self.reg(reg));
            }
            MicroOp::IndexEffAddrNoPenalty(index) => {
                let base = self.eff_addr;
                let idx = self.reg(index);

                let base_lo = (base & 0xFF) as u8;
                let base_hi = (base >> 8) as u8;

                let sum = base_lo.wrapping_add(idx);
                let new_hi = base_hi.wrapping_add(if (base_lo as u16 + idx as u16) > 0xFF {
                    1
                } else {
                    0
                });

                let dummy_addr = ((new_hi as u16) << 8) | (sum as u16);
                let _dummy = bus.read(dummy_addr);
                self.eff_addr = dummy_addr;
            }
            MicroOp::AbsIndexedReadOrDummy(index, dest) => {
                let base_lo = self.addr_lo;
                let base_hi = self.addr_hi;
                let idx = self.reg(index);
                let sum = base_lo.wrapping_add(idx);
                let low = sum;
                let base_page = base_hi;

                let addr = ((base_page as u16) << 8) | (low as u16);
                let carry = (base_lo as u16 + idx as u16) > 0xFF;

                if !carry {
                    let final_addr = addr;
                    let value = bus.read(final_addr);
                    *self.reg_mut(dest) = value;
                    self.update_nz_flags(value);
                    return MicroFlow::EndInstruction;
                }

                let _dummy = bus.read(addr);
                self.eff_addr = ((base_page.wrapping_add(1) as u16 & 0xFF) << 8) | (low as u16);
            }
            MicroOp::AbsIndexedAluAOrDummy(index, op) => {
                let base_lo = self.addr_lo;
                let base_hi = self.addr_hi;
                let idx = self.reg(index);
                let sum = base_lo.wrapping_add(idx);
                let low = sum;
                let base_page = base_hi;

                let addr = ((base_page as u16) << 8) | (low as u16);
                let carry = (base_lo as u16 + idx as u16) > 0xFF;

                if !carry {
                    let final_addr = addr;
                    let value = bus.read(final_addr);
                    self.eval_alu_op(op, value);
                    return MicroFlow::EndInstruction;
                }

                let _dummy = bus.read(addr);
                self.eff_addr = ((base_page.wrapping_add(1) as u16 & 0xFF) << 8) | (low as u16);
            }
            MicroOp::BranchReadOffsetAndDecide(branch_cond) => {
                let offset = bus.read(self.pc);
                self.pc = self.pc.wrapping_add(1);
                let taken = self.eval_branch_cond(branch_cond);

                if !taken {
                    return MicroFlow::EndInstruction;
                }

                let rel = offset as i8 as i16;
                let target = (self.pc as i16).wrapping_add(rel) as u16;

                self.branch_target = target;
                self.branch_page_cross = (self.pc & 0xFF00) != (target & 0xFF00);
            }
            MicroOp::BranchApplyIfTaken => {
                let _dummy = bus.read(self.pc);
                self.pc = self.branch_target;
                if !self.branch_page_cross {
                    return MicroFlow::EndInstruction;
                }
            }
            MicroOp::BranchPageCrossPenalty => {
                let _dummy = bus.read(self.pc);
            }
            MicroOp::ExtraCycle => {
                #[cfg(feature = "tracing")]
                tracing::trace!("Extra cycle");
            }
            MicroOp::StackPushReg(reg) => {
                let addr = self.stack_addr();
                self.sp = self.sp.wrapping_sub(1);
                bus.write(addr, self.reg(reg));
            }
            MicroOp::StackPushPcHi => {
                let addr = self.stack_addr();
                self.sp = self.sp.wrapping_sub(1);
                bus.write(addr, (self.pc >> 8) as u8);
            }
            MicroOp::StackPushPcLo => {
                let addr = self.stack_addr();
                self.sp = self.sp.wrapping_sub(1);
                bus.write(addr, self.pc as u8);
            }
            MicroOp::StackPushStatus => {
                let value: u8 = self.status.into();
                let addr = self.stack_addr();
                self.sp = self.sp.wrapping_sub(1);
                bus.write(addr, value | 0b0011_0000);
            }
            MicroOp::StackPullRegSetNZ(reg) => {
                self.sp = self.sp.wrapping_add(1);
                let addr = self.stack_addr();
                let value = bus.read(addr);
                *self.reg_mut(reg) = value;
                self.update_nz_flags(value);
            }
            MicroOp::StackPullStatus => {
                self.sp = self.sp.wrapping_add(1);
                let addr = self.stack_addr();
                self.status = bus.read(addr).into();
            }
            MicroOp::Alu(op, src) => {
                let value = match src {
                    AluSrc::Imm => {
                        let v = bus.read(self.pc);
                        self.pc = self.pc.wrapping_add(1);
                        v
                    }
                    AluSrc::ZpAddrLo => bus.read(self.addr_lo as u16),
                    AluSrc::EffAddr => bus.read(self.eff_addr),
                };
                self.eval_alu_op(op, value);
            }
        }

        MicroFlow::Continue
    }
}

#[cfg(test)]
mod tests {
    use crate::simple_bus::SimpleBus;

    use super::*;

    use test_log::test;

    fn run_instructions<B: Bus>(cpu: &mut Cpu, bus: &mut B, amount: usize) -> usize {
        let mut cycles = 0;
        for _ in 0..amount {
            cycles += cpu.step_instruction(bus);
        }
        cycles
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
}
