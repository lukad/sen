use bincode::{Decode, Encode};

use crate::{
    cartridge::CartridgeError,
    mapper::{ChrState, Mapper, Mirroring},
};

use super::SaveRamError;

struct Mmc1Resources {
    prg_rom: Vec<u8>,
    chr_rom: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub(crate) struct Mmc1State {
    prg_ram: [u8; 0x2000],
    shift: u8,
    shift_count: u8,
    control: u8,
    chr_bank0: u8,
    chr_bank1: u8,
    prg_bank: u8,
    last_write_cycle: Option<u64>,
    chr: ChrState,
}

pub(crate) struct Mmc1 {
    resources: Mmc1Resources,
    pub(super) state: Mmc1State,
}

impl Mmc1 {
    pub(crate) fn new(prg: &[u8], chr: &[u8]) -> Result<Self, CartridgeError> {
        if prg.len() < 0x8000 || !prg.len().is_multiple_of(0x4000) {
            return Err(CartridgeError::UnsupportedPrgRomSize(prg.len()));
        }

        if prg.len() > 0x40000 {
            return Err(CartridgeError::UnsupportedPrgRomSize(prg.len()));
        }

        let (chr_rom, chr) = if chr.is_empty() {
            (None, ChrState::Ram(Box::new([0; 0x2000])))
        } else if chr.len().is_multiple_of(0x2000) && chr.len() <= 0x20000 {
            (Some(chr.to_vec()), ChrState::Rom)
        } else {
            return Err(CartridgeError::UnsupportedChrRomSize(chr.len()));
        };

        Ok(Self {
            resources: Mmc1Resources {
                prg_rom: prg.to_vec(),
                chr_rom,
            },
            state: Mmc1State {
                prg_ram: [0; 0x2000],
                shift: 0,
                shift_count: 0,
                control: 0x0C,
                chr_bank0: 0,
                chr_bank1: 0,
                prg_bank: 0,
                last_write_cycle: None,
                chr,
            },
        })
    }

    fn write_serial(&mut self, addr: u16, value: u8, cycle_count: u64) {
        if value & 0x80 != 0 {
            self.state.shift = 0;
            self.state.shift_count = 0;
            self.state.control |= 0x0C;
            self.state.last_write_cycle = Some(cycle_count);
            return;
        }

        if self.state.last_write_cycle.and_then(|c| c.checked_add(1)) == Some(cycle_count) {
            self.state.last_write_cycle = Some(cycle_count);
            return;
        }

        self.state.last_write_cycle = Some(cycle_count);
        self.state.shift |= (value & 0x01) << self.state.shift_count;
        self.state.shift_count += 1;

        if self.state.shift_count == 5 {
            self.commit_register(addr, self.state.shift & 0x1F);
            self.state.shift = 0;
            self.state.shift_count = 0;
        }
    }

    fn commit_register(&mut self, addr: u16, value: u8) {
        match addr {
            0x8000..=0x9FFF => self.state.control = value,
            0xA000..=0xBFFF => self.state.chr_bank0 = value,
            0xC000..=0xDFFF => self.state.chr_bank1 = value,
            0xE000..=0xFFFF => self.state.prg_bank = value,
            _ => unreachable!(),
        }
    }

    fn prg_ram_enabled(&self) -> bool {
        self.state.prg_bank & 0x10 == 0
    }

    fn prg_rom_read(&self, addr: u16) -> Option<u8> {
        if !matches!(addr, 0x8000..=0xFFFF) {
            return None;
        }

        let bank_count = self.resources.prg_rom.len() / 0x4000;
        let prg_mode = (self.state.control >> 2) & 0x03;
        let selected_bank = (self.state.prg_bank & 0x0F) as usize % bank_count;

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

        Some(self.resources.prg_rom[(bank % bank_count) * 0x4000 + offset])
    }

    fn chr_len(&self) -> usize {
        match &self.state.chr {
            ChrState::Rom => self
                .resources
                .chr_rom
                .as_ref()
                .expect("CHR ROM state always has a CHR ROM resource")
                .len(),
            ChrState::Ram(bytes) => bytes.len(),
        }
    }

    fn chr_offset(&self, addr: u16) -> Option<usize> {
        if !matches!(addr, 0x0000..=0x1FFF) {
            return None;
        }

        let bank_count = self.chr_len() / 0x1000;
        let chr_4k_mode = self.state.control & 0x10 != 0;

        let offset = if chr_4k_mode {
            match addr {
                0x0000..=0x0FFF => {
                    let bank = self.state.chr_bank0 as usize % bank_count;
                    bank * 0x1000 + addr as usize
                }
                0x1000..=0x1FFF => {
                    let bank = self.state.chr_bank1 as usize % bank_count;
                    bank * 0x1000 + (addr as usize - 0x1000)
                }
                _ => unreachable!(),
            }
        } else {
            let bank = (self.state.chr_bank0 as usize & !1) % bank_count;
            bank * 0x1000 + addr as usize
        };

        Some(offset)
    }
}

impl Mapper for Mmc1 {
    fn mirroring(&self) -> Mirroring {
        match self.state.control & 0x03 {
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
                Some(self.state.prg_ram[(addr - 0x6000) as usize])
            }
            0x6000..=0x7FFF => None,
            0x8000..=0xFFFF => self.prg_rom_read(addr),
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8, cycle_count: u64) {
        match addr {
            0x6000..=0x7FFF if self.prg_ram_enabled() => {
                self.state.prg_ram[(addr - 0x6000) as usize] = value;
            }
            0x8000..=0xFFFF => self.write_serial(addr, value, cycle_count),
            _ => (),
        }
    }

    fn ppu_read(&self, addr: u16) -> Option<u8> {
        let offset = self.chr_offset(addr)?;

        match &self.state.chr {
            ChrState::Rom => Some(
                self.resources
                    .chr_rom
                    .as_ref()
                    .expect("CHR ROM state always has a CHR ROM resource")[offset],
            ),
            ChrState::Ram(bytes) => Some(bytes[offset]),
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let Some(offset) = self.chr_offset(addr) else {
            return;
        };

        if let ChrState::Ram(bytes) = &mut self.state.chr {
            bytes[offset] = value;
        }
    }

    fn save_ram(&self) -> Option<&[u8]> {
        Some(self.state.prg_ram.as_slice())
    }

    fn save_ram_mut(&mut self) -> Option<&mut [u8]> {
        Some(self.state.prg_ram.as_mut_slice())
    }

    fn load_save_ram(&mut self, data: &[u8]) -> Result<(), SaveRamError> {
        if data.len() != self.state.prg_ram.len() {
            return Err(SaveRamError::InvalidSize {
                expected: self.state.prg_ram.len(),
                actual: data.len(),
            });
        }

        self.state.prg_ram.copy_from_slice(data);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cloneable_state_contains_ram_registers_and_serial_timing() {
        let prg_rom = vec![0; 0x8000];
        let mut mmc1 = Mmc1::new(&prg_rom, &[]).unwrap();

        mmc1.cpu_write(0x6000, 0xAB, 0);
        mmc1.ppu_write(0x0123, 0xBC);
        mmc1.cpu_write(0xE000, 1, 10);
        mmc1.cpu_write(0xE000, 0, 11); // Suppressed consecutive-cycle write
        mmc1.cpu_write(0xE000, 0, 13);

        let captured = mmc1.state.clone();

        mmc1.cpu_write(0x6000, 0xCD, 14);
        mmc1.ppu_write(0x0123, 0xDE);
        mmc1.cpu_write(0xE000, 1, 15);

        assert_eq!(captured.prg_ram[0], 0xAB);
        assert_eq!(captured.shift, 0b0000_0001);
        assert_eq!(captured.shift_count, 2);
        assert_eq!(captured.last_write_cycle, Some(13));
        let ChrState::Ram(captured_chr_ram) = captured.chr else {
            panic!("expected CHR RAM");
        };
        assert_eq!(captured_chr_ram[0x0123], 0xBC);

        assert_eq!(mmc1.cpu_read(0x6000), Some(0xCD));
        assert_eq!(mmc1.ppu_read(0x0123), Some(0xDE));
    }

    #[test]
    fn chr_rom_bytes_are_an_immutable_resource_outside_mmc1_state() {
        let prg_rom = vec![0; 0x8000];
        let mut chr_rom = vec![0; 0x2000];
        chr_rom[0x0123] = 0x5A;
        let mut mmc1 = Mmc1::new(&prg_rom, &chr_rom).unwrap();

        assert!(matches!(&mmc1.state.chr, ChrState::Rom));
        assert!(mmc1.resources.chr_rom.is_some());

        mmc1.ppu_write(0x0123, 0xA5);
        assert_eq!(mmc1.ppu_read(0x0123), Some(0x5A));
    }
}
