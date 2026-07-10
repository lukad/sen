use crate::{
    cartridge::CartridgeError,
    mapper::{Mapper, Mirroring, SaveRamError, mmc3::Mmc3},
};

enum TxromChr {
    Rom(Vec<u8>),
    Ram(Vec<u8>),
}

pub(crate) struct Txrom {
    mmc3: Mmc3,
    prg_rom: Vec<u8>,
    prg_ram: Box<[u8; 0x2000]>,
    chr: TxromChr,
    four_screen: bool,
}

impl Txrom {
    pub(crate) fn new(
        prg: &[u8],
        chr: &[u8],
        mirroring: Mirroring,
    ) -> Result<Self, CartridgeError> {
        validate_prg(prg, 0x80000)?;

        let chr = if chr.is_empty() {
            TxromChr::Ram(vec![0; 0x2000])
        } else if chr.len().is_multiple_of(0x2000) && chr.len() <= 0x40000 {
            TxromChr::Rom(chr.to_vec())
        } else {
            return Err(CartridgeError::UnsupportedChrRomSize(chr.len()));
        };

        Ok(Self {
            mmc3: Mmc3::new(mirroring),
            prg_rom: prg.to_vec(),
            prg_ram: Box::new([0; 0x2000]),
            chr,
            four_screen: mirroring == Mirroring::FourScreen,
        })
    }

    fn chr_len(&self) -> usize {
        match &self.chr {
            TxromChr::Rom(bytes) | TxromChr::Ram(bytes) => bytes.len(),
        }
    }

    fn chr_offset(&self, addr: u16) -> Option<usize> {
        let (bank, offset) = self.mmc3.chr_bank(addr)?;
        let bank_count = self.chr_len() / 0x0400;
        Some(bank as usize % bank_count * 0x0400 + offset)
    }
}

impl Mapper for Txrom {
    fn mirroring(&self) -> Mirroring {
        if self.four_screen {
            Mirroring::FourScreen
        } else {
            self.mmc3.mirroring()
        }
    }

    fn cpu_read(&self, addr: u16) -> Option<u8> {
        match addr {
            0x6000..=0x7FFF if self.mmc3.prg_ram_enabled() => {
                Some(self.prg_ram[(addr - 0x6000) as usize])
            }
            0x6000..=0x7FFF => None,
            0x8000..=0xFFFF => {
                let offset = self.mmc3.prg_rom_offset(addr, self.prg_rom.len())?;
                Some(self.prg_rom[offset])
            }
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8, _cpu_cycle: u64) {
        match addr {
            0x6000..=0x7FFF if self.mmc3.prg_ram_writable() => {
                self.prg_ram[(addr - 0x6000) as usize] = value;
            }
            0x8000..=0xFFFF => self.mmc3.write_register(addr, value),
            _ => (),
        }
    }

    fn ppu_read(&self, addr: u16) -> Option<u8> {
        let offset = self.chr_offset(addr)?;
        match &self.chr {
            TxromChr::Rom(bytes) | TxromChr::Ram(bytes) => Some(bytes[offset]),
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let Some(offset) = self.chr_offset(addr) else {
            return;
        };

        if let TxromChr::Ram(bytes) = &mut self.chr {
            bytes[offset] = value;
        }
    }

    fn observe_ppu_addr(&mut self, addr: u16, ppu_cycle: u64) {
        self.mmc3.observe_ppu_addr(addr, ppu_cycle);
    }

    fn irq_asserted(&self) -> bool {
        self.mmc3.irq_asserted()
    }

    fn save_ram(&self) -> Option<&[u8]> {
        Some(self.prg_ram.as_slice())
    }

    fn save_ram_mut(&mut self) -> Option<&mut [u8]> {
        Some(self.prg_ram.as_mut_slice())
    }

    fn load_save_ram(&mut self, data: &[u8]) -> Result<(), SaveRamError> {
        if data.len() != self.prg_ram.len() {
            return Err(SaveRamError::InvalidSize {
                expected: self.prg_ram.len(),
                actual: data.len(),
            });
        }

        self.prg_ram.copy_from_slice(data);
        Ok(())
    }
}

pub(super) fn validate_prg(prg: &[u8], max_len: usize) -> Result<(), CartridgeError> {
    if prg.len() < 0x8000 || prg.len() > max_len || !prg.len().is_multiple_of(0x2000) {
        return Err(CartridgeError::UnsupportedPrgRomSize(prg.len()));
    }

    Ok(())
}
