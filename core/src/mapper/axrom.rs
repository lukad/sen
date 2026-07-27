use bincode::{Decode, Encode};

use crate::{
    cartridge::CartridgeError,
    mapper::{Mapper, Mirroring},
};

const PRG_BANK_SIZE: usize = 0x8000;
const MAX_PRG_ROM_SIZE: usize = 0x40000;

pub(crate) struct Axrom {
    resources: AxromResources,
    pub(super) state: AxromState,
}

struct AxromResources {
    prg_rom: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub(crate) struct AxromState {
    bank_select: u8,
    chr_ram: Box<[u8; 0x2000]>,
}

impl Axrom {
    pub(crate) fn new(prg: &[u8], chr: &[u8]) -> Result<Self, CartridgeError> {
        if prg.is_empty()
            || !prg.len().is_multiple_of(PRG_BANK_SIZE)
            || prg.len() > MAX_PRG_ROM_SIZE
        {
            return Err(CartridgeError::UnsupportedPrgRomSize(prg.len()));
        }

        if !chr.is_empty() {
            return Err(CartridgeError::UnsupportedChrRomSize(chr.len()));
        }

        Ok(Self {
            resources: AxromResources {
                prg_rom: prg.to_vec(),
            },
            state: AxromState {
                bank_select: 0,
                chr_ram: Box::new([0; 0x2000]),
            },
        })
    }
}

impl Mapper for Axrom {
    fn mirroring(&self) -> Mirroring {
        if self.state.bank_select & 0x10 == 0 {
            Mirroring::SingleScreenLower
        } else {
            Mirroring::SingleScreenUpper
        }
    }

    fn cpu_read(&self, addr: u16) -> Option<u8> {
        if !(0x8000..=0xFFFF).contains(&addr) {
            return None;
        }

        let bank_count = self.resources.prg_rom.len() / PRG_BANK_SIZE;
        let bank = (self.state.bank_select & 0x07) as usize % bank_count;
        let offset = (addr - 0x8000) as usize;

        Some(self.resources.prg_rom[bank * PRG_BANK_SIZE + offset])
    }

    fn cpu_write(&mut self, addr: u16, value: u8, _cpu_cycle: u64) {
        if matches!(addr, 0x8000..=0xFFFF) {
            self.state.bank_select = value & 0x17;
        }
    }

    fn ppu_read(&self, addr: u16) -> Option<u8> {
        if !(0x0000..=0x1FFF).contains(&addr) {
            return None;
        }

        Some(self.state.chr_ram[addr as usize])
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        if matches!(addr, 0x0000..=0x1FFF) {
            self.state.chr_ram[addr as usize] = value;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prg_banks_with_ids(bank_count: usize) -> Vec<u8> {
        let mut prg_rom = Vec::with_capacity(bank_count * PRG_BANK_SIZE);

        for bank in 0..bank_count {
            prg_rom.extend(std::iter::repeat_n(bank as u8, PRG_BANK_SIZE));
        }

        prg_rom
    }

    #[test]
    fn bank_register_selects_prg_and_single_screen_page() {
        let prg_rom = prg_banks_with_ids(4);
        let mut axrom = Axrom::new(&prg_rom, &[]).unwrap();

        axrom.cpu_write(0x8000, 0x12, 0);

        assert_eq!(axrom.cpu_read(0x8000), Some(2));
        assert_eq!(axrom.cpu_read(0xFFFF), Some(2));
        assert_eq!(axrom.mirroring(), Mirroring::SingleScreenUpper);
    }

    #[test]
    fn chr_ram_is_cloneable_state_independent_of_the_live_board() {
        let prg_rom = prg_banks_with_ids(1);
        let mut axrom = Axrom::new(&prg_rom, &[]).unwrap();

        axrom.ppu_write(0x0123, 0xAB);
        let captured = axrom.state.clone();
        axrom.ppu_write(0x0123, 0xCD);

        assert_eq!(captured.chr_ram[0x0123], 0xAB);
        assert_eq!(axrom.ppu_read(0x0123), Some(0xCD));
    }
}
