pub(crate) mod cnrom;
pub(crate) mod mmc1;
pub(crate) mod mmc3;
pub(crate) mod nrom;
pub(crate) mod tqrom;
pub(crate) mod txrom;
pub(crate) mod txsrom;
pub(crate) mod uxrom;

use crate::cartridge::CartridgeError;

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

pub(crate) enum Chr {
    Rom(Box<[u8; 0x2000]>),
    Ram(Box<[u8; 0x2000]>),
}

impl Chr {
    pub(crate) fn new(chr: &[u8]) -> Result<Self, CartridgeError> {
        match chr.len() {
            0 => Ok(Chr::Ram(Box::new([0; 0x2000]))),
            0x2000 => Ok(Chr::Rom(Box::new(chr.try_into().unwrap()))),
            other => Err(CartridgeError::UnsupportedChrRomSize(other)),
        }
    }

    pub(crate) fn read(&self, addr: u16) -> Option<u8> {
        match addr {
            0x0000..=0x1FFF => match self {
                Chr::Rom(bytes) | Chr::Ram(bytes) => Some(bytes[addr as usize]),
            },
            _ => None,
        }
    }

    pub(crate) fn write(&mut self, addr: u16, value: u8) {
        if let (0x0000..=0x1FFF, Chr::Ram(bytes)) = (addr, self) {
            bytes[addr as usize] = value;
        }
    }
}
