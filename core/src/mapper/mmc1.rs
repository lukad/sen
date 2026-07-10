use crate::{
    cartridge::CartridgeError,
    mapper::{Mapper, Mirroring},
};

use super::SaveRamError;

enum Mmc1Chr {
    Rom(Vec<u8>),
    Ram(Vec<u8>),
}

pub(crate) struct Mmc1 {
    prg_rom: Vec<u8>,
    prg_ram: Box<[u8; 0x2000]>,
    chr: Mmc1Chr,
    shift: u8,
    shift_count: u8,
    control: u8,
    chr_bank0: u8,
    chr_bank1: u8,
    prg_bank: u8,
    last_write_cycle: Option<u64>,
}

impl Mmc1 {
    pub(crate) fn new(prg: &[u8], chr: &[u8]) -> Result<Self, CartridgeError> {
        if prg.len() < 0x8000 || !prg.len().is_multiple_of(0x4000) {
            return Err(CartridgeError::UnsupportedPrgRomSize(prg.len()));
        }

        if prg.len() > 0x40000 {
            return Err(CartridgeError::UnsupportedPrgRomSize(prg.len()));
        }

        let chr = if chr.is_empty() {
            Mmc1Chr::Ram(vec![0; 0x2000])
        } else if chr.len().is_multiple_of(0x2000) && chr.len() <= 0x20000 {
            Mmc1Chr::Rom(chr.to_vec())
        } else {
            return Err(CartridgeError::UnsupportedChrRomSize(chr.len()));
        };

        Ok(Self {
            prg_rom: prg.to_vec(),
            prg_ram: Box::new([0; 0x2000]),
            chr,
            shift: 0,
            shift_count: 0,
            control: 0x0C,
            chr_bank0: 0,
            chr_bank1: 0,
            prg_bank: 0,
            last_write_cycle: None,
        })
    }

    fn write_serial(&mut self, addr: u16, value: u8, cycle_count: u64) {
        if value & 0x80 != 0 {
            self.shift = 0;
            self.shift_count = 0;
            self.control |= 0x0C;
            self.last_write_cycle = Some(cycle_count);
            return;
        }

        if self.last_write_cycle.and_then(|c| c.checked_add(1)) == Some(cycle_count) {
            self.last_write_cycle = Some(cycle_count);
            return;
        }

        self.last_write_cycle = Some(cycle_count);
        self.shift |= (value & 0x01) << self.shift_count;
        self.shift_count += 1;

        if self.shift_count == 5 {
            self.commit_register(addr, self.shift & 0x1F);
            self.shift = 0;
            self.shift_count = 0;
        }
    }

    fn commit_register(&mut self, addr: u16, value: u8) {
        match addr {
            0x8000..=0x9FFF => self.control = value,
            0xA000..=0xBFFF => self.chr_bank0 = value,
            0xC000..=0xDFFF => self.chr_bank1 = value,
            0xE000..=0xFFFF => self.prg_bank = value,
            _ => unreachable!(),
        }
    }

    fn prg_ram_enabled(&self) -> bool {
        self.prg_bank & 0x10 == 0
    }

    fn prg_rom_read(&self, addr: u16) -> Option<u8> {
        if !matches!(addr, 0x8000..=0xFFFF) {
            return None;
        }

        let bank_count = self.prg_rom.len() / 0x4000;
        let prg_mode = (self.control >> 2) & 0x03;
        let selected_bank = (self.prg_bank & 0x0F) as usize % bank_count;

        let (bank, offset) = match (prg_mode, addr) {
            // 32 KiB mode. Low bit ignored. $8000-$FFFF maps two consecutive 16 KiB banks.
            (0 | 1, 0x8000..=0xFFFF) => {
                let bank = selected_bank & !1;
                (
                    bank + ((addr - 0x8000) as usize / 0x4000),
                    (addr as usize) & 0x3FFF,
                )
            }

            // Fix first 16 KiB bank at $8000, switch 16 KiB bank at $C000.
            (2, 0x8000..=0xBFFF) => (0, (addr - 0x8000) as usize),
            (2, 0xC000..=0xFFFF) => (selected_bank, (addr - 0xC000) as usize),

            // Switch 16 KiB bank at $8000, fix last 16 KiB bank at $C000.
            (3, 0x8000..=0xBFFF) => (selected_bank, (addr - 0x8000) as usize),
            (3, 0xC000..=0xFFFF) => (bank_count - 1, (addr - 0xC000) as usize),

            _ => unreachable!(),
        };

        Some(self.prg_rom[(bank % bank_count) * 0x4000 + offset])
    }

    fn chr_len(&self) -> usize {
        match &self.chr {
            Mmc1Chr::Rom(bytes) | Mmc1Chr::Ram(bytes) => bytes.len(),
        }
    }

    fn chr_offset(&self, addr: u16) -> Option<usize> {
        if !matches!(addr, 0x0000..=0x1FFF) {
            return None;
        }

        let bank_count = self.chr_len() / 0x1000;
        let chr_4k_mode = self.control & 0x10 != 0;

        let offset = if chr_4k_mode {
            match addr {
                0x0000..=0x0FFF => {
                    let bank = self.chr_bank0 as usize % bank_count;
                    bank * 0x1000 + addr as usize
                }
                0x1000..=0x1FFF => {
                    let bank = self.chr_bank1 as usize % bank_count;
                    bank * 0x1000 + (addr as usize - 0x1000)
                }
                _ => unreachable!(),
            }
        } else {
            let bank = (self.chr_bank0 as usize & !1) % bank_count;
            bank * 0x1000 + addr as usize
        };

        Some(offset)
    }
}

impl Mapper for Mmc1 {
    fn mirroring(&self) -> Mirroring {
        match self.control & 0x03 {
            0 => Mirroring::SingleScreenLower,
            1 => Mirroring::SingleScreenUpper,
            2 => Mirroring::Vertical,
            3 => Mirroring::Horizontal,
            _ => unreachable!(),
        }
    }

    fn cpu_read(&self, addr: u16) -> Option<u8> {
        match addr {
            0x6000..=0x7FFF if self.prg_ram_enabled() => {
                Some(self.prg_ram[(addr - 0x6000) as usize])
            }
            0x6000..=0x7FFF => None,
            0x8000..=0xFFFF => self.prg_rom_read(addr),
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8, cycle_count: u64) {
        match addr {
            0x6000..=0x7FFF if self.prg_ram_enabled() => {
                self.prg_ram[(addr - 0x6000) as usize] = value;
            }
            0x8000..=0xFFFF => self.write_serial(addr, value, cycle_count),
            _ => (),
        }
    }

    fn ppu_read(&self, addr: u16) -> Option<u8> {
        let offset = self.chr_offset(addr)?;

        match &self.chr {
            Mmc1Chr::Rom(bytes) | Mmc1Chr::Ram(bytes) => Some(bytes[offset]),
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let Some(offset) = self.chr_offset(addr) else {
            return;
        };

        if let Mmc1Chr::Ram(bytes) = &mut self.chr {
            bytes[offset] = value;
        }
    }

    fn save_ram(&self) -> Option<&[u8]> {
        Some(self.prg_ram.as_slice())
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
