use crate::{
    cartridge::CartridgeError,
    mapper::{Chr, Mapper, Mirroring},
};

pub(crate) struct Uxrom {
    prg_rom: Vec<u8>,
    chr: Chr,
    mirroring: Mirroring,
    bank_select: u8,
}

impl Uxrom {
    pub(crate) fn new(
        prg: &[u8],
        chr: &[u8],
        mirroring: Mirroring,
    ) -> Result<Self, CartridgeError> {
        if prg.len() < 0x8000 || !prg.len().is_multiple_of(0x4000) {
            return Err(CartridgeError::UnsupportedPrgRomSize(prg.len()));
        }

        Ok(Self {
            prg_rom: prg.to_vec(),
            chr: Chr::new(chr)?,
            mirroring,
            bank_select: 0,
        })
    }
}

impl Mapper for Uxrom {
    fn cpu_read(&self, addr: u16) -> Option<u8> {
        let bank_count = self.prg_rom.len() / 0x4000;

        let (bank, offset_in_bank) = match addr {
            0x8000..=0xBFFF => {
                let bank = self.bank_select as usize % bank_count;
                (bank, (addr - 0x8000) as usize)
            }
            0xC000..=0xFFFF => {
                let bank = bank_count - 1;
                (bank, (addr - 0xC000) as usize)
            }
            _ => return None,
        };

        Some(self.prg_rom[bank * 0x4000 + offset_in_bank])
    }

    fn cpu_write(&mut self, addr: u16, value: u8, _cpu_cycle: u64) {
        if matches!(addr, 0x8000..=0xFFFF) {
            self.bank_select = value;
        }
    }

    fn ppu_read(&self, addr: u16) -> Option<u8> {
        self.chr.read(addr)
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        self.chr.write(addr, value);
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }
}
