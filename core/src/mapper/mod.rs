use bincode::{Decode, Encode};

pub(crate) mod cnrom;
pub(crate) mod mmc1;
pub(crate) mod mmc3;
pub(crate) mod nrom;
pub(crate) mod tqrom;
pub(crate) mod txrom;
pub(crate) mod txsrom;
pub(crate) mod uxrom;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub enum Mirroring {
    Horizontal,
    Vertical,
    FourScreen,
    SingleScreenLower,
    SingleScreenUpper,
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub(crate) enum ChrState {
    Rom,
    Ram(Box<[u8; 0x2000]>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SaveRamError {
    #[error("unsupported")]
    Unsupported,
    #[error("invalid size: expected {expected}, got {actual}")]
    InvalidSize { expected: usize, actual: usize },
    #[error("not battery backed")]
    NotBatteryBacked,
}

pub(crate) trait Mapper: Send {
    fn mirroring(&self) -> Mirroring;
    fn cpu_read(&self, addr: u16) -> Option<u8>;
    fn cpu_write(&mut self, addr: u16, value: u8, cpu_cycle: u64);
    fn ppu_read(&self, addr: u16) -> Option<u8>;
    fn ppu_write(&mut self, addr: u16, value: u8);

    #[allow(unused_variables)]
    fn observe_ppu_addr(&mut self, addr: u16, ppu_cycle: u64) {}

    fn irq_asserted(&self) -> bool {
        false
    }

    fn nametable_index(&self, addr: u16) -> usize {
        nametable_index(addr, self.mirroring())
    }

    fn save_ram(&self) -> Option<&[u8]> {
        None
    }

    fn save_ram_mut(&mut self) -> Option<&mut [u8]> {
        None
    }

    #[allow(unused_variables)]
    fn load_save_ram(&mut self, data: &[u8]) -> Result<(), SaveRamError> {
        Err(SaveRamError::Unsupported)
    }

    fn prg_ram_mut(&mut self) -> Option<&mut [u8]> {
        self.save_ram_mut()
    }
}

pub(crate) enum Board {
    Nrom(nrom::Nrom),
    Mmc1(mmc1::Mmc1),
    Uxrom(uxrom::Uxrom),
    Cnrom(cnrom::Cnrom),
    Txrom(txrom::Txrom),
    TxSrom(txsrom::TxSrom),
    Tqrom(tqrom::Tqrom),
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub(crate) enum BoardState {
    Nrom(nrom::NromState),
    Mmc1(mmc1::Mmc1State),
    Uxrom(uxrom::UxromState),
    Cnrom(cnrom::CnromState),
    Txrom(txrom::TxromState),
    TxSrom(txsrom::TxSromState),
    Tqrom(tqrom::TqromState),
}

impl Board {
    pub(crate) fn as_mapper(&self) -> &(dyn Mapper + '_) {
        match self {
            Self::Nrom(board) => board,
            Self::Mmc1(board) => board,
            Self::Uxrom(board) => board,
            Self::Cnrom(board) => board,
            Self::Txrom(board) => board,
            Self::TxSrom(board) => board,
            Self::Tqrom(board) => board,
        }
    }

    pub(crate) fn as_mapper_mut(&mut self) -> &mut (dyn Mapper + '_) {
        match self {
            Self::Nrom(board) => board,
            Self::Mmc1(board) => board,
            Self::Uxrom(board) => board,
            Self::Cnrom(board) => board,
            Self::Txrom(board) => board,
            Self::TxSrom(board) => board,
            Self::Tqrom(board) => board,
        }
    }

    pub(crate) fn state(&self) -> BoardState {
        match self {
            Self::Nrom(board) => BoardState::Nrom(board.state.clone()),
            Self::Mmc1(board) => BoardState::Mmc1(board.state.clone()),
            Self::Uxrom(board) => BoardState::Uxrom(board.state.clone()),
            Self::Cnrom(board) => BoardState::Cnrom(board.state.clone()),
            Self::Txrom(board) => BoardState::Txrom(board.state.clone()),
            Self::TxSrom(board) => BoardState::TxSrom(board.state.clone()),
            Self::Tqrom(board) => BoardState::Tqrom(board.state.clone()),
        }
    }

    pub(crate) fn restore_state(&mut self, state: BoardState) {
        match (self, state) {
            (Self::Nrom(board), BoardState::Nrom(state)) => board.state = state,
            (Self::Mmc1(board), BoardState::Mmc1(state)) => board.state = state,
            (Self::Uxrom(board), BoardState::Uxrom(state)) => board.state = state,
            (Self::Cnrom(board), BoardState::Cnrom(state)) => board.state = state,
            (Self::Txrom(board), BoardState::Txrom(state)) => board.state = state,
            (Self::TxSrom(board), BoardState::TxSrom(state)) => board.state = state,
            (Self::Tqrom(board), BoardState::Tqrom(state)) => board.state = state,
            _ => unreachable!("checkpoint belongs to the same machine"),
        }
    }
}

pub(crate) fn nametable_index(addr: u16, mirroring: Mirroring) -> usize {
    let offset = (addr - 0x2000) & 0x0FFF;
    let table = offset / 0x0400;
    let in_table = offset & 0x03FF;

    match mirroring {
        Mirroring::Vertical => match table {
            0 | 2 => in_table as usize,
            1 | 3 => 0x0400 + in_table as usize,
            _ => unreachable!(),
        },
        Mirroring::Horizontal => match table {
            0 | 1 => in_table as usize,
            2 | 3 => 0x0400 + in_table as usize,
            _ => unreachable!(),
        },
        Mirroring::SingleScreenLower => in_table as usize,
        Mirroring::SingleScreenUpper => 0x0400 + in_table as usize,
        Mirroring::FourScreen => offset as usize,
    }
}
