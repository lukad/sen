pub(crate) mod cnrom;
pub(crate) mod mmc1;
pub(crate) mod nrom;
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

pub(crate) trait Mapper {
    fn mirroring(&self) -> Mirroring;
    fn cpu_read(&self, addr: u16) -> Option<u8>;
    fn cpu_write(&mut self, addr: u16, value: u8, cpu_cycle: u64);
    fn ppu_read(&self, addr: u16) -> Option<u8>;
    fn ppu_write(&mut self, addr: u16, value: u8);
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
