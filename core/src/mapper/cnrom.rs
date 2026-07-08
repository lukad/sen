use crate::{
    cartridge::CartridgeError,
    mapper::{Mapper, Mirroring},
};

pub(crate) struct Cnrom {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    mirroring: Mirroring,
    chr_bank: u8,
}

impl Cnrom {
    pub(crate) fn new(
        prg: &[u8],
        chr: &[u8],
        mirroring: Mirroring,
    ) -> Result<Self, CartridgeError> {
        if !matches!(prg.len(), 0x4000 | 0x8000) {
            return Err(CartridgeError::UnsupportedPrgRomSize(prg.len()));
        }

        if !matches!(chr.len(), 0x4000 | 0x8000) {
            return Err(CartridgeError::UnsupportedChrRomSize(chr.len()));
        }

        Ok(Self {
            prg_rom: prg.to_vec(),
            chr_rom: chr.to_vec(),
            mirroring,
            chr_bank: 0,
        })
    }
}

impl Mapper for Cnrom {
    fn mirroring(&self) -> super::Mirroring {
        self.mirroring
    }

    fn cpu_read(&self, addr: u16) -> Option<u8> {
        if !(0x8000..=0xFFFF).contains(&addr) {
            return None;
        }
        let offset = (addr - 0x8000) as usize;

        Some(self.prg_rom[offset % self.prg_rom.len()])
    }

    fn cpu_write(&mut self, addr: u16, value: u8, _cpu_cycle: u64) {
        if !(0x8000..=0xFFFF).contains(&addr) {
            return;
        }

        let rom_value = self.cpu_read(addr).unwrap_or(0xFF);
        self.chr_bank = value & rom_value;
    }

    fn ppu_read(&self, addr: u16) -> Option<u8> {
        if !(0x0000..=0x1FFF).contains(&addr) {
            return None;
        }
        let offset = addr as usize;

        let bank_count = self.chr_rom.len() / 0x2000;
        let bank = self.chr_bank as usize % bank_count;

        Some(self.chr_rom[bank * 0x2000 + offset])
    }

    fn ppu_write(&mut self, _addr: u16, _value: u8) {}
}
