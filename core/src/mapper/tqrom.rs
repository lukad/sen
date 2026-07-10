use crate::{
    cartridge::CartridgeError,
    mapper::{Mapper, Mirroring, mmc3::Mmc3, txrom::validate_prg},
};

pub(crate) struct Tqrom {
    mmc3: Mmc3,
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    chr_ram: Box<[u8; 0x2000]>,
}

impl Tqrom {
    pub(crate) fn new(
        prg: &[u8],
        chr: &[u8],
        mirroring: Mirroring,
    ) -> Result<Self, CartridgeError> {
        validate_prg(prg, 0x20000)?;

        if chr.is_empty() || !chr.len().is_multiple_of(0x2000) || chr.len() > 0x10000 {
            return Err(CartridgeError::UnsupportedChrRomSize(chr.len()));
        }

        Ok(Self {
            mmc3: Mmc3::new(mirroring),
            prg_rom: prg.to_vec(),
            chr_rom: chr.to_vec(),
            chr_ram: Box::new([0; 0x2000]),
        })
    }

    fn chr_target(&self, addr: u16) -> Option<ChrTarget> {
        let (bank, offset) = self.mmc3.chr_bank(addr)?;

        if bank & 0x40 != 0 {
            let bank = bank as usize & 0x07;
            Some(ChrTarget::Ram(bank * 0x0400 + offset))
        } else {
            let bank_count = self.chr_rom.len() / 0x0400;
            let bank = (bank as usize & 0x3F) % bank_count;
            Some(ChrTarget::Rom(bank * 0x0400 + offset))
        }
    }
}

impl Mapper for Tqrom {
    fn mirroring(&self) -> Mirroring {
        self.mmc3.mirroring()
    }

    fn cpu_read(&self, addr: u16) -> Option<u8> {
        let offset = self.mmc3.prg_rom_offset(addr, self.prg_rom.len())?;
        Some(self.prg_rom[offset])
    }

    fn cpu_write(&mut self, addr: u16, value: u8, _cpu_cycle: u64) {
        if matches!(addr, 0x8000..=0xFFFF) {
            self.mmc3.write_register(addr, value);
        }
    }

    fn ppu_read(&self, addr: u16) -> Option<u8> {
        match self.chr_target(addr)? {
            ChrTarget::Rom(offset) => Some(self.chr_rom[offset]),
            ChrTarget::Ram(offset) => Some(self.chr_ram[offset]),
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        if let Some(ChrTarget::Ram(offset)) = self.chr_target(addr) {
            self.chr_ram[offset] = value;
        }
    }

    fn observe_ppu_addr(&mut self, addr: u16, ppu_cycle: u64) {
        self.mmc3.observe_ppu_addr(addr, ppu_cycle);
    }

    fn irq_asserted(&self) -> bool {
        self.mmc3.irq_asserted()
    }
}

enum ChrTarget {
    Rom(usize),
    Ram(usize),
}
