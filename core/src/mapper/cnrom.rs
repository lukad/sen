use crate::{
    cartridge::CartridgeError,
    mapper::{Mapper, Mirroring},
};

pub(crate) struct Cnrom {
    resources: CnromResources,
    pub(super) state: CnromState,
}

struct CnromResources {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    mirroring: Mirroring,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CnromState {
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
            resources: CnromResources {
                prg_rom: prg.to_vec(),
                chr_rom: chr.to_vec(),
                mirroring,
            },
            state: CnromState { chr_bank: 0 },
        })
    }
}

impl Mapper for Cnrom {
    fn mirroring(&self) -> super::Mirroring {
        self.resources.mirroring
    }

    fn cpu_read(&self, addr: u16) -> Option<u8> {
        if !(0x8000..=0xFFFF).contains(&addr) {
            return None;
        }
        let offset = (addr - 0x8000) as usize;

        Some(self.resources.prg_rom[offset % self.resources.prg_rom.len()])
    }

    fn cpu_write(&mut self, addr: u16, value: u8, _cpu_cycle: u64) {
        if !(0x8000..=0xFFFF).contains(&addr) {
            return;
        }

        let rom_value = self.cpu_read(addr).unwrap_or(0xFF);
        self.state.chr_bank = value & rom_value;
    }

    fn ppu_read(&self, addr: u16) -> Option<u8> {
        if !(0x0000..=0x1FFF).contains(&addr) {
            return None;
        }
        let offset = addr as usize;

        let bank_count = self.resources.chr_rom.len() / 0x2000;
        let bank = self.state.chr_bank as usize % bank_count;

        Some(self.resources.chr_rom[bank * 0x2000 + offset])
    }

    fn ppu_write(&mut self, _addr: u16, _value: u8) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cloneable_bank_state_is_independent_of_the_live_board() {
        let prg_rom = vec![0xFF; 0x4000];
        let mut chr_rom = vec![0; 0x4000];
        chr_rom[0x0000] = 0x11;
        chr_rom[0x2000] = 0x22;
        let mut cnrom = Cnrom::new(&prg_rom, &chr_rom, Mirroring::Vertical).unwrap();

        cnrom.cpu_write(0x8000, 1, 0);
        let captured = cnrom.state.clone();

        cnrom.cpu_write(0x8000, 0, 1);

        assert_eq!(captured.chr_bank, 1);
        assert_eq!(cnrom.state.chr_bank, 0);
        assert_eq!(cnrom.ppu_read(0x0000), Some(0x11));
        assert_eq!(cnrom.mirroring(), Mirroring::Vertical);
    }

    #[test]
    fn bank_select_still_observes_prg_rom_bus_conflicts() {
        let mut prg_rom = vec![0xFF; 0x4000];
        prg_rom[0] = 0b0000_0010;
        let mut chr_rom = vec![0; 0x8000];
        for bank in 0..4 {
            chr_rom[bank * 0x2000] = bank as u8;
        }
        let mut cnrom = Cnrom::new(&prg_rom, &chr_rom, Mirroring::Horizontal).unwrap();

        cnrom.cpu_write(0x8000, 0b0000_0011, 0);

        assert_eq!(cnrom.state.chr_bank, 0b0000_0010);
        assert_eq!(cnrom.ppu_read(0x0000), Some(2));
    }
}
