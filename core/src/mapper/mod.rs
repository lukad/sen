pub(crate) mod cnrom;
pub(crate) mod mmc1;
pub(crate) mod mmc3;
pub(crate) mod nrom;
pub(crate) mod tqrom;
pub(crate) mod txrom;
pub(crate) mod txsrom;
pub(crate) mod uxrom;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mirroring {
    Horizontal,
    Vertical,
    FourScreen,
    SingleScreenLower,
    SingleScreenUpper,
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
