use crate::{
    cartridge::CartridgeError,
    mapper::{Mapper, Mirroring, SaveRamError, mmc3::Mmc3, txrom::validate_prg},
};

pub(crate) struct TxSrom {
    mmc3: Mmc3,
    prg_rom: Vec<u8>,
    prg_ram: Box<[u8; 0x2000]>,
    chr_rom: Vec<u8>,
}

impl TxSrom {
    pub(crate) fn new(
        prg: &[u8],
        chr: &[u8],
        mirroring: Mirroring,
    ) -> Result<Self, CartridgeError> {
        validate_prg(prg, 0x80000)?;

        if chr.is_empty() || !chr.len().is_multiple_of(0x2000) || chr.len() > 0x20000 {
            return Err(CartridgeError::UnsupportedChrRomSize(chr.len()));
        }

        Ok(Self {
            mmc3: Mmc3::new(mirroring),
            prg_rom: prg.to_vec(),
            prg_ram: Box::new([0; 0x2000]),
            chr_rom: chr.to_vec(),
        })
    }

    fn chr_offset(&self, addr: u16) -> Option<usize> {
        let (bank, offset) = self.mmc3.chr_bank(addr)?;
        let bank_count = self.chr_rom.len() / 0x0400;
        Some(bank as usize % bank_count * 0x0400 + offset)
    }
}

impl Mapper for TxSrom {
    fn mirroring(&self) -> Mirroring {
        self.mmc3.mirroring()
    }

    fn nametable_index(&self, addr: u16) -> usize {
        let nametable_offset = (addr - 0x2000) & 0x0FFF;
        let (bank, _) = self
            .mmc3
            .chr_bank(nametable_offset)
            .expect("nametable offset is always in the first pattern table");
        let ciram_page = (bank >> 7) as usize;

        ciram_page * 0x0400 + (nametable_offset as usize & 0x03FF)
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
        Some(self.chr_rom[offset])
    }

    fn ppu_write(&mut self, _addr: u16, _value: u8) {}

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
