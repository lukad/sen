use crate::{
    cartridge::CartridgeError,
    mapper::{Mapper, Mirroring, mmc3::Mmc3State, txrom::validate_prg},
};

struct TqromResources {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TqromState {
    mmc3: Mmc3State,
    mirroring: Mirroring,
    chr_ram: Box<[u8; 0x2000]>,
}

pub(crate) struct Tqrom {
    resources: TqromResources,
    state: TqromState,
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
            resources: TqromResources {
                prg_rom: prg.to_vec(),
                chr_rom: chr.to_vec(),
            },
            state: TqromState {
                mmc3: Mmc3State::new(),
                mirroring,
                chr_ram: Box::new([0; 0x2000]),
            },
        })
    }

    fn chr_target(&self, addr: u16) -> Option<ChrTarget> {
        let (bank, offset) = self.state.mmc3.chr_bank(addr)?;

        if bank & 0x40 != 0 {
            let bank = bank as usize & 0x07;
            Some(ChrTarget::Ram(bank * 0x0400 + offset))
        } else {
            let bank_count = self.resources.chr_rom.len() / 0x0400;
            let bank = (bank as usize & 0x3F) % bank_count;
            Some(ChrTarget::Rom(bank * 0x0400 + offset))
        }
    }
}

impl Mapper for Tqrom {
    fn mirroring(&self) -> Mirroring {
        self.state.mirroring
    }

    fn cpu_read(&self, addr: u16) -> Option<u8> {
        let offset = self
            .state
            .mmc3
            .prg_rom_offset(addr, self.resources.prg_rom.len())?;
        Some(self.resources.prg_rom[offset])
    }

    fn cpu_write(&mut self, addr: u16, value: u8, _cpu_cycle: u64) {
        match addr {
            0xA000..=0xBFFE if addr & 1 == 0 => {
                self.state.mirroring = if value & 1 == 0 {
                    Mirroring::Vertical
                } else {
                    Mirroring::Horizontal
                };
            }
            0x8000..=0xFFFF => self.state.mmc3.write_register(addr, value),
            _ => (),
        }
    }

    fn ppu_read(&self, addr: u16) -> Option<u8> {
        match self.chr_target(addr)? {
            ChrTarget::Rom(offset) => Some(self.resources.chr_rom[offset]),
            ChrTarget::Ram(offset) => Some(self.state.chr_ram[offset]),
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        if let Some(ChrTarget::Ram(offset)) = self.chr_target(addr) {
            self.state.chr_ram[offset] = value;
        }
    }

    fn observe_ppu_addr(&mut self, addr: u16, ppu_cycle: u64) {
        self.state.mmc3.observe_ppu_addr(addr, ppu_cycle);
    }

    fn irq_asserted(&self) -> bool {
        self.state.mmc3.irq_asserted()
    }
}

enum ChrTarget {
    Rom(usize),
    Ram(usize),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prg_ram_control_register_is_absent_from_tqrom_state() {
        let prg_rom = vec![0; 0x8000];
        let chr_rom = vec![0; 0x2000];
        let mut board = Tqrom::new(&prg_rom, &chr_rom, Mirroring::Horizontal).unwrap();
        let before = board.state.clone();

        board.cpu_write(0xA001, 0xC0, 0);

        assert_eq!(board.state, before);
    }
}
