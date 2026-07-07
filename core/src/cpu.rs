use std::ops::{BitAndAssign, BitOrAssign};

use crate::{
    bus::Bus,
    microcode::{self, AluOp, AluSrc, BranchCond, MicroOp, Reg, ShiftOp, StatusPushKind},
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

impl BitAndAssign<u8> for Status {
    fn bitand_assign(&mut self, rhs: u8) {
        self.carry &= rhs & 0x01 != 0;
        self.zero &= rhs & 0x02 != 0;
        self.interrupt_disable &= rhs & 0x04 != 0;
        self.decimal_mode &= rhs & 0x08 != 0;
        self.overflow &= rhs & 0x40 != 0;
        self.negative &= rhs & 0x80 != 0;
    }
}

impl BitOrAssign<u8> for Status {
    fn bitor_assign(&mut self, rhs: u8) {
        self.carry |= rhs & 0x01 != 0;
        self.zero |= rhs & 0x02 != 0;
        self.interrupt_disable |= rhs & 0x04 != 0;
        self.decimal_mode |= rhs & 0x08 != 0;
        self.overflow |= rhs & 0x40 != 0;
        self.negative |= rhs & 0x80 != 0;
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum CpuState {
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
    data: u8,
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
            data: 0,
            branch_target: 0,
            branch_page_cross: false,

            state: CpuState::Fetch,
        }
    }

    pub fn reset<B: Bus>(&mut self, bus: &mut B) {
        let lo = bus.read(0xFFFC);
        let hi = bus.read(0xFFFD);

        self.pc = u16::from_le_bytes([lo, hi]);
        self.sp = 0xFD;
        self.status = 0x24.into();
        self.state = CpuState::Fetch;

        self.opcode = 0;
        self.addr_lo = 0;
        self.addr_hi = 0;
        self.eff_addr = 0;
        self.data = 0;
        self.branch_target = 0;
        self.branch_page_cross = false;
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

    pub fn start_nmi(&mut self) {
        assert_eq!(self.state, CpuState::Fetch);
        self.state = CpuState::Exec {
            code: microcode::NMI,
            ip: 0,
        };
    }

    pub fn start_irq(&mut self) {
        assert_eq!(self.state, CpuState::Fetch);
        self.state = CpuState::Exec {
            code: microcode::IRQ,
            ip: 0,
        };
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
            AluOp::Adc => {
                let carry_in = if self.status.carry { 1 } else { 0 };
                let sum = self.a as u16 + value as u16 + carry_in;
                let result = sum as u8;
                self.status.carry = sum > 0xFF;
                self.status.overflow = ((self.a ^ result) & (value ^ result) & 0x80) != 0;
                result
            }
            AluOp::Sbc => {
                let carry_in = if self.status.carry { 1 } else { 0 };
                let borrow = 1 - carry_in;
                let result = self.a.wrapping_sub(value).wrapping_sub(borrow);
                self.status.carry = (self.a as u16) >= (value as u16 + borrow as u16);
                self.status.overflow = ((self.a ^ result) & (self.a ^ value) & 0x80) != 0;
                result
            }
        };
        self.a = result;
        self.update_nz_flags(result);
    }

    fn eval_shift_op(&mut self, op: ShiftOp, value: u8) -> u8 {
        match op {
            ShiftOp::Asl => {
                self.status.carry = value & 0x80 != 0;
                value << 1
            }
            ShiftOp::Lsr => {
                self.status.carry = value & 0x01 != 0;
                value >> 1
            }
            ShiftOp::Rol => {
                let carry_in = if self.status.carry { 1 } else { 0 };
                self.status.carry = value & 0x80 != 0;
                (value << 1) | carry_in
            }
            ShiftOp::Ror => {
                let carry_in = if self.status.carry { 0x80 } else { 0 };
                self.status.carry = value & 0x01 != 0;
                (value >> 1) | carry_in
            }
        }
    }

    fn eval_compare(&mut self, reg: Reg, value: u8) {
        let lhs = self.reg(reg);
        let result = lhs.wrapping_sub(value);

        self.status.carry = lhs >= value;
        self.status.zero = result == 0;
        self.status.negative = result & 0x80 != 0;
    }

    fn exec_micro_op<B: Bus>(&mut self, bus: &mut B, op: MicroOp) -> MicroFlow {
        match op {
            MicroOp::ReadPcAndDiscard => {
                let _dummy = bus.read(self.pc);
                self.pc = self.pc.wrapping_add(1);
            }
            MicroOp::ReadPcAndDiscardNoInc => {
                let _dummy = bus.read(self.pc);
            }
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
            MicroOp::StackIncSp => {
                self.sp = self.sp.wrapping_add(1);
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
            MicroOp::StackPushStatus(kind) => {
                let value: u8 = self.status.into();
                let pushed = match kind {
                    StatusPushKind::PhpOrBrk => value | 0x30,
                    StatusPushKind::Interrupt => value | 0x20,
                };
                let addr = self.stack_addr();
                self.sp = self.sp.wrapping_sub(1);
                bus.write(addr, pushed);
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
            MicroOp::StackReadPcLoThenIncSp => {
                self.addr_lo = bus.read(self.stack_addr());
                self.sp = self.sp.wrapping_add(1);
            }
            MicroOp::StackReadPcHi => {
                self.addr_hi = bus.read(self.stack_addr());
                self.pc = ((self.addr_hi as u16) << 8) | self.addr_lo as u16;
            }
            MicroOp::StackReadStatusThenIncSp => {
                self.status = bus.read(self.stack_addr()).into();
                self.sp = self.sp.wrapping_add(1);
            }
            MicroOp::IncPc => {
                self.pc = self.pc.wrapping_add(1);
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
            MicroOp::Compare(reg, src) => {
                let value = match src {
                    AluSrc::Imm => {
                        let v = bus.read(self.pc);
                        self.pc = self.pc.wrapping_add(1);
                        v
                    }
                    AluSrc::ZpAddrLo => bus.read(self.addr_lo as u16),
                    AluSrc::EffAddr => bus.read(self.eff_addr),
                };
                self.eval_compare(reg, value);
            }
            MicroOp::AbsIndexedCompareOrDummy(index, reg) => {
                let base_lo = self.addr_lo;
                let base_hi = self.addr_hi;
                let idx = self.reg(index);

                let low = base_lo.wrapping_add(idx);
                let addr = ((base_hi as u16) << 8) | low as u16;
                let carry = (base_lo as u16 + idx as u16) > 0xFF;

                if !carry {
                    let value = bus.read(addr);
                    self.eval_compare(reg, value);
                    return MicroFlow::EndInstruction;
                }

                let _dummy = bus.read(addr);
                self.eff_addr = ((base_hi.wrapping_add(1) as u16) << 8) | low as u16;
            }
            MicroOp::ReadVectorLo(addr) => {
                self.addr_lo = bus.read(addr);
            }
            MicroOp::ReadVectorHiSetPcAndI(addr) => {
                self.addr_hi = bus.read(addr);
                self.pc = ((self.addr_hi as u16) << 8) | self.addr_lo as u16;
                self.status.interrupt_disable = true;
            }
            MicroOp::ClearStatusBit(mask) => {
                self.status &= !mask;
            }
            MicroOp::SetStatusBit(mask) => {
                self.status |= mask;
            }
            MicroOp::IncRegSetNZ(reg) => {
                let value = self.reg(reg).wrapping_add(1);
                *self.reg_mut(reg) = value;
                self.update_nz_flags(value);
            }
            MicroOp::DecRegSetNZ(reg) => {
                let value = self.reg(reg).wrapping_sub(1);
                *self.reg_mut(reg) = value;
                self.update_nz_flags(value);
            }
            MicroOp::IncDataSetNZAndWriteZpAddr => {
                self.data = self.data.wrapping_add(1);
                self.update_nz_flags(self.data);
                bus.write(self.addr_lo as u16, self.data);
            }
            MicroOp::DecDataSetNZAndWriteZpAddr => {
                self.data = self.data.wrapping_sub(1);
                self.update_nz_flags(self.data);
                bus.write(self.addr_lo as u16, self.data);
            }
            MicroOp::IncDataSetNZAndWriteEffAddr => {
                self.data = self.data.wrapping_add(1);
                self.update_nz_flags(self.data);
                bus.write(self.eff_addr, self.data);
            }
            MicroOp::DecDataSetNZAndWriteEffAddr => {
                self.data = self.data.wrapping_sub(1);
                self.update_nz_flags(self.data);
                bus.write(self.eff_addr, self.data);
            }
            MicroOp::ReadZpAddrToData => self.data = bus.read(self.addr_lo as u16),
            MicroOp::WriteDataToZpAddr => bus.write(self.addr_lo as u16, self.data),
            MicroOp::ReadEffAddrToData => self.data = bus.read(self.eff_addr),
            MicroOp::WriteDataToEffAddr => bus.write(self.eff_addr, self.data),
            MicroOp::ShiftRegSetCZN(reg, shift_op) => {
                let value = self.reg(reg);
                let result = self.eval_shift_op(shift_op, value);
                *self.reg_mut(reg) = result;
                self.update_nz_flags(result);
            }
            MicroOp::ShiftDataSetCZNAndWriteZpAddr(shift_op) => {
                self.data = self.eval_shift_op(shift_op, self.data);
                self.update_nz_flags(self.data);
                bus.write(self.addr_lo as u16, self.data);
            }
            MicroOp::ShiftDataSetCZNAndWriteEffAddr(shift_op) => {
                self.data = self.eval_shift_op(shift_op, self.data);
                self.update_nz_flags(self.data);
                bus.write(self.eff_addr, self.data);
            }
        }

        MicroFlow::Continue
    }
}

impl Default for Cpu {
    fn default() -> Self {
        Self::new()
    }
}
