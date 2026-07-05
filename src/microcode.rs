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
    /// Add with carry
    Adc,
    /// Subtract with carry
    Sbc,
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
    /// Compare A with the value read from the source
    Compare(Reg, AluSrc),
    /// Indexed compare with page-cross timing.
    /// base is in addr_lo/addr_hi.
    /// - no page cross: read from base+index, compare reg with value, set C/Z/N, end instruction.
    /// - page cross: dummy read, eff_addr = corrected address, continue.
    AbsIndexedCompareOrDummy(Reg, Reg),
    /// Indexed ALU operation on A with page-cross timing.
    /// base is in addr_lo/addr_hi.
    /// - no page cross: read from base+index, A = A (op) value, set NZ, end instruction.
    /// - page cross: dummy read, eff_addr = corrected address, continue.
    AbsIndexedAluAOrDummy(Reg, AluOp),

    /// Read the next byte from the program counter and discard it
    ReadPcAndDiscard,
    /// Read the low byte of the BRK/IRQ vector at $FFFE.
    ReadIrqVectorLo,
    /// Read the high byte of the BRK/IRQ vector at $FFFF, set PC, and set interrupt disable.
    ReadIrqVectorHiSetPcAndI,

    /// Clear a status bit in the status register
    ClearStatusBit(u8),
    /// Set a status bit in the status register
    SetStatusBit(u8),
    /// Increment a register and set Z/N from the new value.
    IncRegSetNZ(Reg),
    /// Decrement a register and set Z/N from the new value.
    DecRegSetNZ(Reg),
    /// Read the byte at zero-page addr_lo into the CPU data scratch.
    ReadZpAddrToData,
    /// Write the CPU data scratch to zero-page addr_lo.
    WriteDataToZpAddr,
    /// Increment data, set Z/N from the new value, then write it to zero-page addr_lo.
    IncDataSetNZAndWriteZpAddr,
    /// Decrement data, set Z/N from the new value, then write it to zero-page addr_lo.
    DecDataSetNZAndWriteZpAddr,
    /// Read the byte at eff_addr into the CPU data scratch.
    ReadEffAddrToData,
    /// Write the CPU data scratch to eff_addr.
    WriteDataToEffAddr,
    /// Increment data, set Z/N from the new value, then write it to eff_addr.
    IncDataSetNZAndWriteEffAddr,
    /// Decrement data, set Z/N from the new value, then write it to eff_addr.
    DecDataSetNZAndWriteEffAddr,
}

use MicroOp::*;
use Reg::*;
use valuable::Valuable;

static LDA_IMM: &[MicroOp] = &[ReadPcToRegSetNZ(A)];
static LDA_ABS: &[MicroOp] = &[ReadPcToAddrLo, ReadPcToAddrHi, ReadEffAddrToRegSetNZ(A)];
static LDA_ABSX: &[MicroOp] = &[
    ReadPcToAddrLo,
    ReadPcToAddrHi,
    AbsIndexedReadOrDummy(X, A),
    ReadEffAddrToRegSetNZ(A),
];
static LDA_ABSY: &[MicroOp] = &[
    ReadPcToAddrLo,
    ReadPcToAddrHi,
    AbsIndexedReadOrDummy(Y, A),
    ReadEffAddrToRegSetNZ(A),
];
static LDA_ZP: &[MicroOp] = &[ReadPcToAddrLo, ReadZpAddrToRegSetNZ(A)];
static LDA_ZPX: &[MicroOp] = &[
    ReadPcToAddrLo,
    ZpIndexedDummyReadAndCompute(X),
    ReadEffAddrToRegSetNZ(A),
];
static LDA_INDX: &[MicroOp] = &[
    ReadPcToAddrLo,
    ZpIndexedDummyReadAndCompute(X),
    ReadEffAddrToAddrLo,
    ReadEffAddrToAddrHi,
    ReadEffAddrToRegSetNZ(A),
];
static LDA_INDY: &[MicroOp] = &[
    ReadPcToAddrLo,
    ReadZpPtrLoToAddrLo,
    ReadZpPtrHiToAddrHi,
    AbsIndexedReadOrDummy(Y, A),
    ReadEffAddrToRegSetNZ(A),
];

static LDX_IMM: &[MicroOp] = &[ReadPcToRegSetNZ(X)];
static LDX_ABS: &[MicroOp] = &[ReadPcToAddrLo, ReadPcToAddrHi, ReadEffAddrToRegSetNZ(X)];
static LDX_ABSY: &[MicroOp] = &[
    ReadPcToAddrLo,
    ReadPcToAddrHi,
    AbsIndexedReadOrDummy(Y, X),
    ReadEffAddrToRegSetNZ(X),
];
static LDX_ZP: &[MicroOp] = &[ReadPcToAddrLo, ReadZpAddrToRegSetNZ(X)];
static LDX_ZPY: &[MicroOp] = &[
    ReadPcToAddrLo,
    ZpIndexedDummyReadAndCompute(Y),
    ReadEffAddrToRegSetNZ(X),
];

static LDY_IMM: &[MicroOp] = &[ReadPcToRegSetNZ(Y)];
static LDY_ABS: &[MicroOp] = &[ReadPcToAddrLo, ReadPcToAddrHi, ReadEffAddrToRegSetNZ(Y)];
static LDY_ABSX: &[MicroOp] = &[
    ReadPcToAddrLo,
    ReadPcToAddrHi,
    AbsIndexedReadOrDummy(X, Y),
    ReadEffAddrToRegSetNZ(Y),
];
static LDY_ZP: &[MicroOp] = &[ReadPcToAddrLo, ReadZpAddrToRegSetNZ(Y)];
static LDY_ZPX: &[MicroOp] = &[
    ReadPcToAddrLo,
    ZpIndexedDummyReadAndCompute(X),
    ReadEffAddrToRegSetNZ(Y),
];

static STA_ZP: &[MicroOp] = &[ReadPcToAddrLo, WriteRegToZpAddr(A)];
static STA_ZPX: &[MicroOp] = &[
    ReadPcToAddrLo,
    ZpIndexedDummyReadAndCompute(X),
    WriteRegToEffAddr(A),
];
static STA_ABS: &[MicroOp] = &[ReadPcToAddrLo, ReadPcToAddrHi, WriteRegToEffAddr(A)];
static STA_ABSX: &[MicroOp] = &[
    ReadPcToAddrLo,
    ReadPcToAddrHi,
    IndexEffAddrNoPenalty(X),
    WriteRegToEffAddr(A),
];
static STA_ABSY: &[MicroOp] = &[
    ReadPcToAddrLo,
    ReadPcToAddrHi,
    IndexEffAddrNoPenalty(Y),
    WriteRegToEffAddr(A),
];
static STA_INDX: &[MicroOp] = &[
    ReadPcToAddrLo,
    ZpIndexedDummyReadAndCompute(X),
    ReadEffAddrToAddrLo,
    ReadEffAddrToAddrHi,
    WriteRegToEffAddr(A),
];

static STA_INDY: &[MicroOp] = &[
    ReadPcToAddrLo,
    ReadZpPtrLoToAddrLo,
    ReadZpPtrHiToAddrHi,
    IndexEffAddrNoPenalty(Y),
    WriteRegToEffAddr(A),
];

static STX_ZP: &[MicroOp] = &[ReadPcToAddrLo, WriteRegToZpAddr(X)];
static STX_ZPY: &[MicroOp] = &[
    ReadPcToAddrLo,
    ZpIndexedDummyReadAndCompute(Y),
    WriteRegToEffAddr(X),
];
static STX_ABS: &[MicroOp] = &[ReadPcToAddrLo, ReadPcToAddrHi, WriteRegToEffAddr(X)];

static STY_ZP: &[MicroOp] = &[ReadPcToAddrLo, WriteRegToZpAddr(Y)];
static STY_ZPX: &[MicroOp] = &[
    ReadPcToAddrLo,
    ZpIndexedDummyReadAndCompute(X),
    WriteRegToEffAddr(Y),
];
static STY_ABS: &[MicroOp] = &[ReadPcToAddrLo, ReadPcToAddrHi, WriteRegToEffAddr(Y)];

static BCC: &[MicroOp] = &[
    BranchReadOffsetAndDecide(BranchCond::CarryClear),
    BranchApplyIfTaken,
    BranchPageCrossPenalty,
];

static BCS: &[MicroOp] = &[
    BranchReadOffsetAndDecide(BranchCond::CarrySet),
    BranchApplyIfTaken,
    BranchPageCrossPenalty,
];

static BEQ: &[MicroOp] = &[
    BranchReadOffsetAndDecide(BranchCond::ZeroSet),
    BranchApplyIfTaken,
    BranchPageCrossPenalty,
];

static BNE: &[MicroOp] = &[
    BranchReadOffsetAndDecide(BranchCond::ZeroClear),
    BranchApplyIfTaken,
    BranchPageCrossPenalty,
];

static BMI: &[MicroOp] = &[
    BranchReadOffsetAndDecide(BranchCond::NegativeSet),
    BranchApplyIfTaken,
    BranchPageCrossPenalty,
];

static BPL: &[MicroOp] = &[
    BranchReadOffsetAndDecide(BranchCond::NegativeClear),
    BranchApplyIfTaken,
    BranchPageCrossPenalty,
];

static BVC: &[MicroOp] = &[
    BranchReadOffsetAndDecide(BranchCond::OverflowClear),
    BranchApplyIfTaken,
    BranchPageCrossPenalty,
];

static BVS: &[MicroOp] = &[
    BranchReadOffsetAndDecide(BranchCond::OverflowSet),
    BranchApplyIfTaken,
    BranchPageCrossPenalty,
];

static TAX: &[MicroOp] = &[CopyRegToRegSetNZ(A, X)];
static TAY: &[MicroOp] = &[CopyRegToRegSetNZ(A, Y)];
static TSX: &[MicroOp] = &[CopyRegToRegSetNZ(SP, X)];
static TXA: &[MicroOp] = &[CopyRegToRegSetNZ(X, A)];
static TXS: &[MicroOp] = &[CopyRegToReg(X, SP)];
static TYA: &[MicroOp] = &[CopyRegToRegSetNZ(Y, A)];

static PHA: &[MicroOp] = &[ExtraCycle, StackPushReg(A)];
static PHP: &[MicroOp] = &[ExtraCycle, StackPushStatus];
static PLA: &[MicroOp] = &[ExtraCycle, StackPullRegSetNZ(A), ExtraCycle];
static PLP: &[MicroOp] = &[ExtraCycle, StackPullStatus, ExtraCycle];

static AND_IMM: &[MicroOp] = &[Alu(AluOp::And, AluSrc::Imm)];
static AND_ZP: &[MicroOp] = &[ReadPcToAddrLo, Alu(AluOp::And, AluSrc::ZpAddrLo)];
static AND_ZPX: &[MicroOp] = &[
    ReadPcToAddrLo,
    ZpIndexedDummyReadAndCompute(X),
    Alu(AluOp::And, AluSrc::EffAddr),
];
static AND_ABS: &[MicroOp] = &[
    ReadPcToAddrLo,
    ReadPcToAddrHi,
    Alu(AluOp::And, AluSrc::EffAddr),
];
static AND_ABSX: &[MicroOp] = &[
    ReadPcToAddrLo,
    ReadPcToAddrHi,
    AbsIndexedAluAOrDummy(X, AluOp::And),
    Alu(AluOp::And, AluSrc::EffAddr),
];
static AND_ABSY: &[MicroOp] = &[
    ReadPcToAddrLo,
    ReadPcToAddrHi,
    AbsIndexedAluAOrDummy(Y, AluOp::And),
    Alu(AluOp::And, AluSrc::EffAddr),
];
static AND_INDX: &[MicroOp] = &[
    ReadPcToAddrLo,
    ZpIndexedDummyReadAndCompute(X),
    ReadEffAddrToAddrLo,
    ReadEffAddrToAddrHi,
    Alu(AluOp::And, AluSrc::EffAddr),
];
static AND_INDY: &[MicroOp] = &[
    ReadPcToAddrLo,
    ReadZpPtrLoToAddrLo,
    ReadZpPtrHiToAddrHi,
    AbsIndexedAluAOrDummy(Y, AluOp::And),
    Alu(AluOp::And, AluSrc::EffAddr),
];

static ORA_IMM: &[MicroOp] = &[Alu(AluOp::Ora, AluSrc::Imm)];
static ORA_ZP: &[MicroOp] = &[ReadPcToAddrLo, Alu(AluOp::Ora, AluSrc::ZpAddrLo)];
static ORA_ZPX: &[MicroOp] = &[
    ReadPcToAddrLo,
    ZpIndexedDummyReadAndCompute(X),
    Alu(AluOp::Ora, AluSrc::EffAddr),
];
static ORA_ABS: &[MicroOp] = &[
    ReadPcToAddrLo,
    ReadPcToAddrHi,
    Alu(AluOp::Ora, AluSrc::EffAddr),
];
static ORA_ABSX: &[MicroOp] = &[
    ReadPcToAddrLo,
    ReadPcToAddrHi,
    AbsIndexedAluAOrDummy(X, AluOp::Ora),
    Alu(AluOp::Ora, AluSrc::EffAddr),
];
static ORA_ABSY: &[MicroOp] = &[
    ReadPcToAddrLo,
    ReadPcToAddrHi,
    AbsIndexedAluAOrDummy(Y, AluOp::Ora),
    Alu(AluOp::Ora, AluSrc::EffAddr),
];
static ORA_INDX: &[MicroOp] = &[
    ReadPcToAddrLo,
    ZpIndexedDummyReadAndCompute(X),
    ReadEffAddrToAddrLo,
    ReadEffAddrToAddrHi,
    Alu(AluOp::Ora, AluSrc::EffAddr),
];
static ORA_INDY: &[MicroOp] = &[
    ReadPcToAddrLo,
    ReadZpPtrLoToAddrLo,
    ReadZpPtrHiToAddrHi,
    AbsIndexedAluAOrDummy(Y, AluOp::Ora),
    Alu(AluOp::Ora, AluSrc::EffAddr),
];

static EOR_IMM: &[MicroOp] = &[Alu(AluOp::Eor, AluSrc::Imm)];
static EOR_ZP: &[MicroOp] = &[ReadPcToAddrLo, Alu(AluOp::Eor, AluSrc::ZpAddrLo)];
static EOR_ZPX: &[MicroOp] = &[
    ReadPcToAddrLo,
    ZpIndexedDummyReadAndCompute(X),
    Alu(AluOp::Eor, AluSrc::EffAddr),
];
static EOR_ABS: &[MicroOp] = &[
    ReadPcToAddrLo,
    ReadPcToAddrHi,
    Alu(AluOp::Eor, AluSrc::EffAddr),
];
static EOR_ABSX: &[MicroOp] = &[
    ReadPcToAddrLo,
    ReadPcToAddrHi,
    AbsIndexedAluAOrDummy(X, AluOp::Eor),
    Alu(AluOp::Eor, AluSrc::EffAddr),
];
static EOR_ABSY: &[MicroOp] = &[
    ReadPcToAddrLo,
    ReadPcToAddrHi,
    AbsIndexedAluAOrDummy(Y, AluOp::Eor),
    Alu(AluOp::Eor, AluSrc::EffAddr),
];
static EOR_INDX: &[MicroOp] = &[
    ReadPcToAddrLo,
    ZpIndexedDummyReadAndCompute(X),
    ReadEffAddrToAddrLo,
    ReadEffAddrToAddrHi,
    Alu(AluOp::Eor, AluSrc::EffAddr),
];
static EOR_INDY: &[MicroOp] = &[
    ReadPcToAddrLo,
    ReadZpPtrLoToAddrLo,
    ReadZpPtrHiToAddrHi,
    AbsIndexedAluAOrDummy(Y, AluOp::Eor),
    Alu(AluOp::Eor, AluSrc::EffAddr),
];

static BIT_ZP: &[MicroOp] = &[ReadPcToAddrLo, Alu(AluOp::Bit, AluSrc::ZpAddrLo)];
static BIT_ABS: &[MicroOp] = &[
    ReadPcToAddrLo,
    ReadPcToAddrHi,
    Alu(AluOp::Bit, AluSrc::EffAddr),
];

static NOP: &[MicroOp] = &[ExtraCycle];

static JMP_ABS: &[MicroOp] = &[ReadPcToAddrLo, ReadPcToAddrHiSetPc];

static JMP_IND: &[MicroOp] = &[
    ReadPcToAddrLo,
    ReadPcToAddrHi,
    ReadEffAddrToAddrLo,
    ReadJmpIndirectAddrHiAndJump,
];

static JSR: &[MicroOp] = &[
    ReadPcToAddrLo,
    ExtraCycle,
    StackPushPcHi,
    StackPushPcLo,
    ReadPcToAddrHiSetPc,
];

static RTS: &[MicroOp] = &[
    ExtraCycle,
    StackIncSp,
    StackReadPcLoThenIncSp,
    StackReadPcHi,
    IncPc,
];

static RTI: &[MicroOp] = &[
    ExtraCycle,
    StackIncSp,
    StackReadStatusThenIncSp,
    StackReadPcLoThenIncSp,
    StackReadPcHi,
];

static BRK: &[MicroOp] = &[
    ReadPcAndDiscard,
    StackPushPcHi,
    StackPushPcLo,
    StackPushStatus,
    ReadIrqVectorLo,
    ReadIrqVectorHiSetPcAndI,
];

static CLC: &[MicroOp] = &[ClearStatusBit(0x01)];
static CLI: &[MicroOp] = &[ClearStatusBit(0x04)];
static CLD: &[MicroOp] = &[ClearStatusBit(0x08)];
static CLV: &[MicroOp] = &[ClearStatusBit(0x40)];

static SEC: &[MicroOp] = &[SetStatusBit(0x01)];
static SEI: &[MicroOp] = &[SetStatusBit(0x04)];
static SED: &[MicroOp] = &[SetStatusBit(0x08)];

static CMP_IMM: &[MicroOp] = &[Compare(A, AluSrc::Imm)];
static CMP_ZP: &[MicroOp] = &[ReadPcToAddrLo, Compare(A, AluSrc::ZpAddrLo)];
static CMP_ZPX: &[MicroOp] = &[
    ReadPcToAddrLo,
    ZpIndexedDummyReadAndCompute(X),
    Compare(A, AluSrc::EffAddr),
];
static CMP_ABS: &[MicroOp] = &[ReadPcToAddrLo, ReadPcToAddrHi, Compare(A, AluSrc::EffAddr)];
static CMP_ABSX: &[MicroOp] = &[
    ReadPcToAddrLo,
    ReadPcToAddrHi,
    AbsIndexedCompareOrDummy(X, A),
    Compare(A, AluSrc::EffAddr),
];
static CMP_ABSY: &[MicroOp] = &[
    ReadPcToAddrLo,
    ReadPcToAddrHi,
    AbsIndexedCompareOrDummy(Y, A),
    Compare(A, AluSrc::EffAddr),
];
static CMP_INDX: &[MicroOp] = &[
    ReadPcToAddrLo,
    ZpIndexedDummyReadAndCompute(X),
    ReadEffAddrToAddrLo,
    ReadEffAddrToAddrHi,
    Compare(A, AluSrc::EffAddr),
];
static CMP_INDY: &[MicroOp] = &[
    ReadPcToAddrLo,
    ReadZpPtrLoToAddrLo,
    ReadZpPtrHiToAddrHi,
    AbsIndexedCompareOrDummy(Y, A),
    Compare(A, AluSrc::EffAddr),
];
static CPX_IMM: &[MicroOp] = &[Compare(X, AluSrc::Imm)];
static CPX_ZP: &[MicroOp] = &[ReadPcToAddrLo, Compare(X, AluSrc::ZpAddrLo)];
static CPX_ABS: &[MicroOp] = &[ReadPcToAddrLo, ReadPcToAddrHi, Compare(X, AluSrc::EffAddr)];
static CPY_IMM: &[MicroOp] = &[Compare(Y, AluSrc::Imm)];
static CPY_ZP: &[MicroOp] = &[ReadPcToAddrLo, Compare(Y, AluSrc::ZpAddrLo)];
static CPY_ABS: &[MicroOp] = &[ReadPcToAddrLo, ReadPcToAddrHi, Compare(Y, AluSrc::EffAddr)];

static INC_ZP: &[MicroOp] = &[
    ReadPcToAddrLo,
    ReadZpAddrToData,
    WriteDataToZpAddr,
    IncDataSetNZAndWriteZpAddr,
];
static INC_ZPX: &[MicroOp] = &[
    ReadPcToAddrLo,
    ZpIndexedDummyReadAndCompute(X),
    ReadEffAddrToData,
    WriteDataToEffAddr,
    IncDataSetNZAndWriteEffAddr,
];
static INC_ABS: &[MicroOp] = &[
    ReadPcToAddrLo,
    ReadPcToAddrHi,
    ReadEffAddrToData,
    WriteDataToEffAddr,
    IncDataSetNZAndWriteEffAddr,
];
static INC_ABSX: &[MicroOp] = &[
    ReadPcToAddrLo,
    ReadPcToAddrHi,
    IndexEffAddrNoPenalty(X),
    ReadEffAddrToData,
    WriteDataToEffAddr,
    IncDataSetNZAndWriteEffAddr,
];
static INX: &[MicroOp] = &[IncRegSetNZ(X)];
static INY: &[MicroOp] = &[IncRegSetNZ(Y)];

static DEC_ZP: &[MicroOp] = &[
    ReadPcToAddrLo,
    ReadZpAddrToData,
    WriteDataToZpAddr,
    DecDataSetNZAndWriteZpAddr,
];
static DEC_ZPX: &[MicroOp] = &[
    ReadPcToAddrLo,
    ZpIndexedDummyReadAndCompute(X),
    ReadEffAddrToData,
    WriteDataToEffAddr,
    DecDataSetNZAndWriteEffAddr,
];
static DEC_ABS: &[MicroOp] = &[
    ReadPcToAddrLo,
    ReadPcToAddrHi,
    ReadEffAddrToData,
    WriteDataToEffAddr,
    DecDataSetNZAndWriteEffAddr,
];
static DEC_ABSX: &[MicroOp] = &[
    ReadPcToAddrLo,
    ReadPcToAddrHi,
    IndexEffAddrNoPenalty(X),
    ReadEffAddrToData,
    WriteDataToEffAddr,
    DecDataSetNZAndWriteEffAddr,
];
static DEX: &[MicroOp] = &[DecRegSetNZ(X)];
static DEY: &[MicroOp] = &[DecRegSetNZ(Y)];

static ADC_IMM: &[MicroOp] = &[Alu(AluOp::Adc, AluSrc::Imm)];
static ADC_ZP: &[MicroOp] = &[ReadPcToAddrLo, Alu(AluOp::Adc, AluSrc::ZpAddrLo)];
static ADC_ZPX: &[MicroOp] = &[
    ReadPcToAddrLo,
    ZpIndexedDummyReadAndCompute(X),
    Alu(AluOp::Adc, AluSrc::EffAddr),
];
static ADC_ABS: &[MicroOp] = &[
    ReadPcToAddrLo,
    ReadPcToAddrHi,
    Alu(AluOp::Adc, AluSrc::EffAddr),
];
static ADC_ABSX: &[MicroOp] = &[
    ReadPcToAddrLo,
    ReadPcToAddrHi,
    AbsIndexedAluAOrDummy(X, AluOp::Adc),
    Alu(AluOp::Adc, AluSrc::EffAddr),
];
static ADC_ABSY: &[MicroOp] = &[
    ReadPcToAddrLo,
    ReadPcToAddrHi,
    AbsIndexedAluAOrDummy(Y, AluOp::Adc),
    Alu(AluOp::Adc, AluSrc::EffAddr),
];
static ADC_INDX: &[MicroOp] = &[
    ReadPcToAddrLo,
    ZpIndexedDummyReadAndCompute(X),
    ReadEffAddrToAddrLo,
    ReadEffAddrToAddrHi,
    Alu(AluOp::Adc, AluSrc::EffAddr),
];
static ADC_INDY: &[MicroOp] = &[
    ReadPcToAddrLo,
    ReadZpPtrLoToAddrLo,
    ReadZpPtrHiToAddrHi,
    AbsIndexedAluAOrDummy(Y, AluOp::Adc),
    Alu(AluOp::Adc, AluSrc::EffAddr),
];

static SBC_IMM: &[MicroOp] = &[Alu(AluOp::Sbc, AluSrc::Imm)];
static SBC_ZP: &[MicroOp] = &[ReadPcToAddrLo, Alu(AluOp::Sbc, AluSrc::ZpAddrLo)];
static SBC_ZPX: &[MicroOp] = &[
    ReadPcToAddrLo,
    ZpIndexedDummyReadAndCompute(X),
    Alu(AluOp::Sbc, AluSrc::EffAddr),
];
static SBC_ABS: &[MicroOp] = &[
    ReadPcToAddrLo,
    ReadPcToAddrHi,
    Alu(AluOp::Sbc, AluSrc::EffAddr),
];
static SBC_ABSX: &[MicroOp] = &[
    ReadPcToAddrLo,
    ReadPcToAddrHi,
    AbsIndexedAluAOrDummy(X, AluOp::Sbc),
    Alu(AluOp::Sbc, AluSrc::EffAddr),
];
static SBC_ABSY: &[MicroOp] = &[
    ReadPcToAddrLo,
    ReadPcToAddrHi,
    AbsIndexedAluAOrDummy(Y, AluOp::Sbc),
    Alu(AluOp::Sbc, AluSrc::EffAddr),
];
static SBC_INDX: &[MicroOp] = &[
    ReadPcToAddrLo,
    ZpIndexedDummyReadAndCompute(X),
    ReadEffAddrToAddrLo,
    ReadEffAddrToAddrHi,
    Alu(AluOp::Sbc, AluSrc::EffAddr),
];
static SBC_INDY: &[MicroOp] = &[
    ReadPcToAddrLo,
    ReadZpPtrLoToAddrLo,
    ReadZpPtrHiToAddrHi,
    AbsIndexedAluAOrDummy(Y, AluOp::Sbc),
    Alu(AluOp::Sbc, AluSrc::EffAddr),
];

pub fn decode(opcode: u8) -> &'static [MicroOp] {
    match opcode {
        0x00 => BRK,
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
        0x18 => CLC,
        0x38 => SEC,
        0x58 => CLI,
        0x78 => SEI,
        0xB8 => CLV,
        0xD8 => CLD,
        0xF8 => SED,
        0xC9 => CMP_IMM,
        0xC5 => CMP_ZP,
        0xD5 => CMP_ZPX,
        0xCD => CMP_ABS,
        0xDD => CMP_ABSX,
        0xD9 => CMP_ABSY,
        0xC1 => CMP_INDX,
        0xD1 => CMP_INDY,
        0xE0 => CPX_IMM,
        0xE4 => CPX_ZP,
        0xEC => CPX_ABS,
        0xC0 => CPY_IMM,
        0xC4 => CPY_ZP,
        0xCC => CPY_ABS,
        0xE6 => INC_ZP,
        0xF6 => INC_ZPX,
        0xEE => INC_ABS,
        0xFE => INC_ABSX,
        0xC6 => DEC_ZP,
        0xD6 => DEC_ZPX,
        0xCE => DEC_ABS,
        0xDE => DEC_ABSX,
        0xE8 => INX,
        0xC8 => INY,
        0xCA => DEX,
        0x88 => DEY,
        0x69 => ADC_IMM,
        0x65 => ADC_ZP,
        0x75 => ADC_ZPX,
        0x6D => ADC_ABS,
        0x7D => ADC_ABSX,
        0x79 => ADC_ABSY,
        0x61 => ADC_INDX,
        0x71 => ADC_INDY,
        0xE9 => SBC_IMM,
        0xE5 => SBC_ZP,
        0xF5 => SBC_ZPX,
        0xED => SBC_ABS,
        0xFD => SBC_ABSX,
        0xF9 => SBC_ABSY,
        0xE1 => SBC_INDX,
        0xF1 => SBC_INDY,
        _ => todo!("Implement decoding for opcode {:#04X}", opcode),
    }
}
