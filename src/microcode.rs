#[derive(Debug, Clone, Copy, Valuable, PartialEq, Eq)]
pub enum Reg {
    A,
    X,
    Y,
    SP,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BranchCond {
    NegativeSet,
    NegativeClear,
    ZeroSet,
    ZeroClear,
    CarrySet,
    CarryClear,
    OverflowSet,
    OverflowClear,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AluSrc {
    Imm,
    ZpAddrLo,
    EffAddr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AluOp {
    /// AND A with the value read from the source
    And,
    /// OR A with the value read from the source
    Ora,
    /// XOR A with the value read from the source
    Eor,
    /// AND A with value read from the source but only set the zero, negative and overflow flags
    Bit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MicroOp {
    /// Read the next byte into a register and set the zero and negative flags
    ReadPcToRegSetNZ(Reg),
    /// Read the next byte into the address low byte
    ReadPcToAddrLo,
    /// Read the next byte into the address high byte and combine with the address low byte to form the effective address
    ReadPcToAddrHi,
    /// Read the next byte into the address high byte and jump to the effective address
    ReadPcToAddrHiSetPc,
    /// Read the next byte from the effective address into a register and set the zero and negative flags
    ReadEffAddrToRegSetNZ(Reg),
    /// Write reg to eff_addr (used by abs / abs,X / abs,Y / zp-indexed)
    WriteRegToEffAddr(Reg),
    /// Copies the value of one register to another and sets the zero and negative flags
    CopyRegToRegSetNZ(Reg, Reg),
    /// Copies the value of one register to another without setting the zero and negative flags
    CopyRegToReg(Reg, Reg),

    /// Read low byte of zp pointer at addr_lo into addr_lo
    ReadZpPtrLoToAddrLo,
    /// Read high byte of zp pointer at (addr_lo+1) into addr_hi,
    /// and form base address in addr_lo/addr_hi (and optionally eff_addr)
    ReadZpPtrHiToAddrHi,

    /// Read the value at addr_lo into a register and set the zero and negative flags
    ReadZpAddrToRegSetNZ(Reg),
    ZpIndexedDummyReadAndCompute(Reg),
    /// Write reg to zero page address in addr_lo
    WriteRegToZpAddr(Reg),
    /// Reads the low byte of the effective address into the address low byte
    ReadEffAddrToAddrLo,
    /// Reads the high byte of the effective address into the address high byte
    ReadEffAddrToAddrHi,
    /// Reads the high byte of the effective address into the address high byte and jumps to the address
    ReadJmpIndirectAddrHiAndJump,

    /// Index eff_addr by reg (no page-cross penalty), with a dummy read.
    /// Used for STA abs,X / abs,Y (always 5 cycles, no conditional extra).
    IndexEffAddrNoPenalty(Reg),
    AbsIndexedReadOrDummy(Reg, Reg),

    /// When the condition is met, read the next byte to compute the branch target, otherwise end the instruction
    BranchReadOffsetAndDecide(BranchCond),
    /// Do a dummy read and continue if the jump crosses a page boundary, otherwise end the instruction
    BranchApplyIfTaken,
    /// Do a dummy read
    BranchPageCrossPenalty,

    /// Dummy cycle that does nothing
    ExtraCycle,
    /// Increment the stack pointer
    StackIncSp,
    /// Push a register to the stack
    StackPushReg(Reg),
    /// Push the high byte of the program counter onto the stack
    StackPushPcHi,
    /// Push the low byte of the program counter onto the stack
    StackPushPcLo,
    /// Push the status flags to the stack
    StackPushStatus,
    /// Pull a register from the stack
    StackPullRegSetNZ(Reg),
    /// Pop the status flags from the stack
    StackPullStatus,
    /// Read the low byte of the program counter from the stack and increment the stack pointer
    StackReadPcLoThenIncSp,
    /// Read the high byte of the program counter from the stack
    StackReadPcHi,
    /// Read the status flags from the stack and increment the stack pointer
    StackReadStatusThenIncSp,
    /// Increment the program counter
    IncPc,

    /// Perform an ALU operation on A and the value read from the source
    Alu(AluOp, AluSrc),
    /// Indexed ALU operation on A with page-cross timing.
    /// base is in addr_lo/addr_hi.
    /// - no page cross: read from base+index, A = A (op) value, set NZ, end instruction.
    /// - page cross: dummy read, eff_addr = corrected address, continue.
    AbsIndexedAluAOrDummy(Reg, AluOp),
}

use MicroOp::*;
use Reg::*;
use valuable::Valuable;

pub static LDA_IMM: &[MicroOp] = &[ReadPcToRegSetNZ(A)];
pub static LDA_ABS: &[MicroOp] = &[ReadPcToAddrLo, ReadPcToAddrHi, ReadEffAddrToRegSetNZ(A)];
pub static LDA_ABSX: &[MicroOp] = &[
    ReadPcToAddrLo,
    ReadPcToAddrHi,
    AbsIndexedReadOrDummy(X, A),
    ReadEffAddrToRegSetNZ(A),
];
pub static LDA_ABSY: &[MicroOp] = &[
    ReadPcToAddrLo,
    ReadPcToAddrHi,
    AbsIndexedReadOrDummy(Y, A),
    ReadEffAddrToRegSetNZ(A),
];
pub static LDA_ZP: &[MicroOp] = &[ReadPcToAddrLo, ReadZpAddrToRegSetNZ(A)];
pub static LDA_ZPX: &[MicroOp] = &[
    ReadPcToAddrLo,
    ZpIndexedDummyReadAndCompute(X),
    ReadEffAddrToRegSetNZ(A),
];
pub static LDA_INDX: &[MicroOp] = &[
    ReadPcToAddrLo,
    ZpIndexedDummyReadAndCompute(X),
    ReadEffAddrToAddrLo,
    ReadEffAddrToAddrHi,
    ReadEffAddrToRegSetNZ(A),
];
pub static LDA_INDY: &[MicroOp] = &[
    ReadPcToAddrLo,
    ReadZpPtrLoToAddrLo,
    ReadZpPtrHiToAddrHi,
    AbsIndexedReadOrDummy(Y, A),
    ReadEffAddrToRegSetNZ(A),
];

pub static LDX_IMM: &[MicroOp] = &[ReadPcToRegSetNZ(X)];
pub static LDX_ABS: &[MicroOp] = &[ReadPcToAddrLo, ReadPcToAddrHi, ReadEffAddrToRegSetNZ(X)];
pub static LDX_ABSY: &[MicroOp] = &[
    ReadPcToAddrLo,
    ReadPcToAddrHi,
    AbsIndexedReadOrDummy(Y, X),
    ReadEffAddrToRegSetNZ(X),
];
pub static LDX_ZP: &[MicroOp] = &[ReadPcToAddrLo, ReadZpAddrToRegSetNZ(X)];
pub static LDX_ZPY: &[MicroOp] = &[
    ReadPcToAddrLo,
    ZpIndexedDummyReadAndCompute(Y),
    ReadEffAddrToRegSetNZ(X),
];

pub static LDY_IMM: &[MicroOp] = &[ReadPcToRegSetNZ(Y)];
pub static LDY_ABS: &[MicroOp] = &[ReadPcToAddrLo, ReadPcToAddrHi, ReadEffAddrToRegSetNZ(Y)];
pub static LDY_ABSX: &[MicroOp] = &[
    ReadPcToAddrLo,
    ReadPcToAddrHi,
    AbsIndexedReadOrDummy(X, Y),
    ReadEffAddrToRegSetNZ(Y),
];
pub static LDY_ZP: &[MicroOp] = &[ReadPcToAddrLo, ReadZpAddrToRegSetNZ(Y)];
pub static LDY_ZPX: &[MicroOp] = &[
    ReadPcToAddrLo,
    ZpIndexedDummyReadAndCompute(X),
    ReadEffAddrToRegSetNZ(Y),
];

pub static STA_ZP: &[MicroOp] = &[ReadPcToAddrLo, WriteRegToZpAddr(A)];
pub static STA_ZPX: &[MicroOp] = &[
    ReadPcToAddrLo,
    ZpIndexedDummyReadAndCompute(X),
    WriteRegToEffAddr(A),
];
pub static STA_ABS: &[MicroOp] = &[ReadPcToAddrLo, ReadPcToAddrHi, WriteRegToEffAddr(A)];
pub static STA_ABSX: &[MicroOp] = &[
    ReadPcToAddrLo,
    ReadPcToAddrHi,
    IndexEffAddrNoPenalty(X),
    WriteRegToEffAddr(A),
];
pub static STA_ABSY: &[MicroOp] = &[
    ReadPcToAddrLo,
    ReadPcToAddrHi,
    IndexEffAddrNoPenalty(Y),
    WriteRegToEffAddr(A),
];
pub static STA_INDX: &[MicroOp] = &[
    ReadPcToAddrLo,
    ZpIndexedDummyReadAndCompute(X),
    ReadEffAddrToAddrLo,
    ReadEffAddrToAddrHi,
    WriteRegToEffAddr(A),
];

pub static STA_INDY: &[MicroOp] = &[
    ReadPcToAddrLo,
    ReadZpPtrLoToAddrLo,
    ReadZpPtrHiToAddrHi,
    IndexEffAddrNoPenalty(Y),
    WriteRegToEffAddr(A),
];

pub static STX_ZP: &[MicroOp] = &[ReadPcToAddrLo, WriteRegToZpAddr(X)];
pub static STX_ZPY: &[MicroOp] = &[
    ReadPcToAddrLo,
    ZpIndexedDummyReadAndCompute(Y),
    WriteRegToEffAddr(X),
];
pub static STX_ABS: &[MicroOp] = &[ReadPcToAddrLo, ReadPcToAddrHi, WriteRegToEffAddr(X)];

pub static STY_ZP: &[MicroOp] = &[ReadPcToAddrLo, WriteRegToZpAddr(Y)];
pub static STY_ZPX: &[MicroOp] = &[
    ReadPcToAddrLo,
    ZpIndexedDummyReadAndCompute(X),
    WriteRegToEffAddr(Y),
];
pub static STY_ABS: &[MicroOp] = &[ReadPcToAddrLo, ReadPcToAddrHi, WriteRegToEffAddr(Y)];

pub static BCC: &[MicroOp] = &[
    BranchReadOffsetAndDecide(BranchCond::CarryClear),
    BranchApplyIfTaken,
    BranchPageCrossPenalty,
];

pub static BCS: &[MicroOp] = &[
    BranchReadOffsetAndDecide(BranchCond::CarrySet),
    BranchApplyIfTaken,
    BranchPageCrossPenalty,
];

pub static BEQ: &[MicroOp] = &[
    BranchReadOffsetAndDecide(BranchCond::ZeroSet),
    BranchApplyIfTaken,
    BranchPageCrossPenalty,
];

pub static BNE: &[MicroOp] = &[
    BranchReadOffsetAndDecide(BranchCond::ZeroClear),
    BranchApplyIfTaken,
    BranchPageCrossPenalty,
];

pub static BMI: &[MicroOp] = &[
    BranchReadOffsetAndDecide(BranchCond::NegativeSet),
    BranchApplyIfTaken,
    BranchPageCrossPenalty,
];

pub static BPL: &[MicroOp] = &[
    BranchReadOffsetAndDecide(BranchCond::NegativeClear),
    BranchApplyIfTaken,
    BranchPageCrossPenalty,
];

pub static BVC: &[MicroOp] = &[
    BranchReadOffsetAndDecide(BranchCond::OverflowClear),
    BranchApplyIfTaken,
    BranchPageCrossPenalty,
];

pub static BVS: &[MicroOp] = &[
    BranchReadOffsetAndDecide(BranchCond::OverflowSet),
    BranchApplyIfTaken,
    BranchPageCrossPenalty,
];

pub static TAX: &[MicroOp] = &[CopyRegToRegSetNZ(A, X)];
pub static TAY: &[MicroOp] = &[CopyRegToRegSetNZ(A, Y)];
pub static TSX: &[MicroOp] = &[CopyRegToRegSetNZ(SP, X)];
pub static TXA: &[MicroOp] = &[CopyRegToRegSetNZ(X, A)];
pub static TXS: &[MicroOp] = &[CopyRegToReg(X, SP)];
pub static TYA: &[MicroOp] = &[CopyRegToRegSetNZ(Y, A)];

pub static PHA: &[MicroOp] = &[ExtraCycle, StackPushReg(A)];
pub static PHP: &[MicroOp] = &[ExtraCycle, StackPushStatus];
pub static PLA: &[MicroOp] = &[ExtraCycle, StackPullRegSetNZ(A), ExtraCycle];
pub static PLP: &[MicroOp] = &[ExtraCycle, StackPullStatus, ExtraCycle];

pub static AND_IMM: &[MicroOp] = &[Alu(AluOp::And, AluSrc::Imm)];
pub static AND_ZP: &[MicroOp] = &[ReadPcToAddrLo, Alu(AluOp::And, AluSrc::ZpAddrLo)];
pub static AND_ZPX: &[MicroOp] = &[
    ReadPcToAddrLo,
    ZpIndexedDummyReadAndCompute(X),
    Alu(AluOp::And, AluSrc::EffAddr),
];
pub static AND_ABS: &[MicroOp] = &[
    ReadPcToAddrLo,
    ReadPcToAddrHi,
    Alu(AluOp::And, AluSrc::EffAddr),
];
pub static AND_ABSX: &[MicroOp] = &[
    ReadPcToAddrLo,
    ReadPcToAddrHi,
    AbsIndexedAluAOrDummy(X, AluOp::And),
    Alu(AluOp::And, AluSrc::EffAddr),
];
pub static AND_ABSY: &[MicroOp] = &[
    ReadPcToAddrLo,
    ReadPcToAddrHi,
    AbsIndexedAluAOrDummy(Y, AluOp::And),
    Alu(AluOp::And, AluSrc::EffAddr),
];
pub static AND_INDX: &[MicroOp] = &[
    ReadPcToAddrLo,
    ZpIndexedDummyReadAndCompute(X),
    ReadEffAddrToAddrLo,
    ReadEffAddrToAddrHi,
    Alu(AluOp::And, AluSrc::EffAddr),
];
pub static AND_INDY: &[MicroOp] = &[
    ReadPcToAddrLo,
    ReadZpPtrLoToAddrLo,
    ReadZpPtrHiToAddrHi,
    AbsIndexedAluAOrDummy(Y, AluOp::And),
    Alu(AluOp::And, AluSrc::EffAddr),
];

pub static ORA_IMM: &[MicroOp] = &[Alu(AluOp::Ora, AluSrc::Imm)];
pub static ORA_ZP: &[MicroOp] = &[ReadPcToAddrLo, Alu(AluOp::Ora, AluSrc::ZpAddrLo)];
pub static ORA_ZPX: &[MicroOp] = &[
    ReadPcToAddrLo,
    ZpIndexedDummyReadAndCompute(X),
    Alu(AluOp::Ora, AluSrc::EffAddr),
];
pub static ORA_ABS: &[MicroOp] = &[
    ReadPcToAddrLo,
    ReadPcToAddrHi,
    Alu(AluOp::Ora, AluSrc::EffAddr),
];
pub static ORA_ABSX: &[MicroOp] = &[
    ReadPcToAddrLo,
    ReadPcToAddrHi,
    AbsIndexedAluAOrDummy(X, AluOp::Ora),
    Alu(AluOp::Ora, AluSrc::EffAddr),
];
pub static ORA_ABSY: &[MicroOp] = &[
    ReadPcToAddrLo,
    ReadPcToAddrHi,
    AbsIndexedAluAOrDummy(Y, AluOp::Ora),
    Alu(AluOp::Ora, AluSrc::EffAddr),
];
pub static ORA_INDX: &[MicroOp] = &[
    ReadPcToAddrLo,
    ZpIndexedDummyReadAndCompute(X),
    ReadEffAddrToAddrLo,
    ReadEffAddrToAddrHi,
    Alu(AluOp::Ora, AluSrc::EffAddr),
];
pub static ORA_INDY: &[MicroOp] = &[
    ReadPcToAddrLo,
    ReadZpPtrLoToAddrLo,
    ReadZpPtrHiToAddrHi,
    AbsIndexedAluAOrDummy(Y, AluOp::Ora),
    Alu(AluOp::Ora, AluSrc::EffAddr),
];

pub static EOR_IMM: &[MicroOp] = &[Alu(AluOp::Eor, AluSrc::Imm)];
pub static EOR_ZP: &[MicroOp] = &[ReadPcToAddrLo, Alu(AluOp::Eor, AluSrc::ZpAddrLo)];
pub static EOR_ZPX: &[MicroOp] = &[
    ReadPcToAddrLo,
    ZpIndexedDummyReadAndCompute(X),
    Alu(AluOp::Eor, AluSrc::EffAddr),
];
pub static EOR_ABS: &[MicroOp] = &[
    ReadPcToAddrLo,
    ReadPcToAddrHi,
    Alu(AluOp::Eor, AluSrc::EffAddr),
];
pub static EOR_ABSX: &[MicroOp] = &[
    ReadPcToAddrLo,
    ReadPcToAddrHi,
    AbsIndexedAluAOrDummy(X, AluOp::Eor),
    Alu(AluOp::Eor, AluSrc::EffAddr),
];
pub static EOR_ABSY: &[MicroOp] = &[
    ReadPcToAddrLo,
    ReadPcToAddrHi,
    AbsIndexedAluAOrDummy(Y, AluOp::Eor),
    Alu(AluOp::Eor, AluSrc::EffAddr),
];
pub static EOR_INDX: &[MicroOp] = &[
    ReadPcToAddrLo,
    ZpIndexedDummyReadAndCompute(X),
    ReadEffAddrToAddrLo,
    ReadEffAddrToAddrHi,
    Alu(AluOp::Eor, AluSrc::EffAddr),
];
pub static EOR_INDY: &[MicroOp] = &[
    ReadPcToAddrLo,
    ReadZpPtrLoToAddrLo,
    ReadZpPtrHiToAddrHi,
    AbsIndexedAluAOrDummy(Y, AluOp::Eor),
    Alu(AluOp::Eor, AluSrc::EffAddr),
];

pub static BIT_ZP: &[MicroOp] = &[ReadPcToAddrLo, Alu(AluOp::Bit, AluSrc::ZpAddrLo)];
pub static BIT_ABS: &[MicroOp] = &[
    ReadPcToAddrLo,
    ReadPcToAddrHi,
    Alu(AluOp::Bit, AluSrc::EffAddr),
];

pub static NOP: &[MicroOp] = &[ExtraCycle];

pub static JMP_ABS: &[MicroOp] = &[ReadPcToAddrLo, ReadPcToAddrHiSetPc];

pub static JMP_IND: &[MicroOp] = &[
    ReadPcToAddrLo,
    ReadPcToAddrHi,
    ReadEffAddrToAddrLo,
    ReadJmpIndirectAddrHiAndJump,
];

pub static JSR: &[MicroOp] = &[
    ReadPcToAddrLo,
    ExtraCycle,
    StackPushPcHi,
    StackPushPcLo,
    ReadPcToAddrHiSetPc,
];

pub static RTS: &[MicroOp] = &[
    ExtraCycle,
    StackIncSp,
    StackReadPcLoThenIncSp,
    StackReadPcHi,
    IncPc,
];

pub static RTI: &[MicroOp] = &[
    ExtraCycle,
    StackIncSp,
    StackReadStatusThenIncSp,
    StackReadPcLoThenIncSp,
    StackReadPcHi,
];

pub fn decode(opcode: u8) -> &'static [MicroOp] {
    match opcode {
        0xEA => NOP,
        0x4C => JMP_ABS,
        0x6C => JMP_IND,
        0x20 => JSR,
        0x60 => RTS,
        0x40 => RTI,
        0xA9 => LDA_IMM,
        0xA5 => LDA_ZP,
        0xB5 => LDA_ZPX,
        0xAD => LDA_ABS,
        0xBD => LDA_ABSX,
        0xB9 => LDA_ABSY,
        0xA1 => LDA_INDX,
        0xB1 => LDA_INDY,
        0xA2 => LDX_IMM,
        0xAE => LDX_ABS,
        0xBE => LDX_ABSY,
        0xA6 => LDX_ZP,
        0xB6 => LDX_ZPY,
        0xA0 => LDY_IMM,
        0xAC => LDY_ABS,
        0xBC => LDY_ABSX,
        0xA4 => LDY_ZP,
        0xB4 => LDY_ZPX,
        0x85 => STA_ZP,
        0x95 => STA_ZPX,
        0x8D => STA_ABS,
        0x9D => STA_ABSX,
        0x99 => STA_ABSY,
        0x81 => STA_INDX,
        0x91 => STA_INDY,
        0x86 => STX_ZP,
        0x96 => STX_ZPY,
        0x8E => STX_ABS,
        0x84 => STY_ZP,
        0x94 => STY_ZPX,
        0x8C => STY_ABS,
        0x90 => BCC,
        0xB0 => BCS,
        0xF0 => BEQ,
        0x30 => BMI,
        0xD0 => BNE,
        0x10 => BPL,
        0x50 => BVC,
        0x70 => BVS,
        0xAA => TAX,
        0xA8 => TAY,
        0xBA => TSX,
        0x8A => TXA,
        0x9A => TXS,
        0x98 => TYA,
        0x48 => PHA,
        0x08 => PHP,
        0x68 => PLA,
        0x28 => PLP,
        0x29 => AND_IMM,
        0x25 => AND_ZP,
        0x35 => AND_ZPX,
        0x2D => AND_ABS,
        0x3D => AND_ABSX,
        0x39 => AND_ABSY,
        0x21 => AND_INDX,
        0x31 => AND_INDY,
        0x09 => ORA_IMM,
        0x05 => ORA_ZP,
        0x15 => ORA_ZPX,
        0x0D => ORA_ABS,
        0x1D => ORA_ABSX,
        0x19 => ORA_ABSY,
        0x01 => ORA_INDX,
        0x11 => ORA_INDY,
        0x49 => EOR_IMM,
        0x45 => EOR_ZP,
        0x55 => EOR_ZPX,
        0x4D => EOR_ABS,
        0x5D => EOR_ABSX,
        0x59 => EOR_ABSY,
        0x41 => EOR_INDX,
        0x51 => EOR_INDY,
        0x24 => BIT_ZP,
        0x2C => BIT_ABS,
        _ => todo!("Implement decoding for opcode {:#04X}", opcode),
    }
}
