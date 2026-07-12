use bincode::{Decode, Encode};

use crate::{
    cartridge::CartridgeError,
    mapper::{
        Mapper, Mirroring, SaveRamError,
        mmc3::{Mmc3PrgRamControl, Mmc3State},
        txrom::validate_prg,
    },
};

struct TxSromResources {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub(crate) struct TxSromState {
    mmc3: Mmc3State,
    prg_ram: Box<[u8; 0x2000]>,
    prg_ram_control: Mmc3PrgRamControl,
}

pub(crate) struct TxSrom {
    resources: TxSromResources,
    pub(super) state: TxSromState,
}

impl TxSrom {
    pub(crate) fn new(
        prg: &[u8],
        chr: &[u8],
        _mirroring: Mirroring,
    ) -> Result<Self, CartridgeError> {
        validate_prg(prg, 0x80000)?;

        if chr.is_empty() || !chr.len().is_multiple_of(0x2000) || chr.len() > 0x20000 {
            return Err(CartridgeError::UnsupportedChrRomSize(chr.len()));
        }

        Ok(Self {
            resources: TxSromResources {
                prg_rom: prg.to_vec(),
                chr_rom: chr.to_vec(),
            },
            state: TxSromState {
                mmc3: Mmc3State::new(),
                prg_ram: Box::new([0; 0x2000]),
                prg_ram_control: Mmc3PrgRamControl::new(),
            },
        })
    }

    fn chr_offset(&self, addr: u16) -> Option<usize> {
        let (bank, offset) = self.state.mmc3.chr_bank(addr)?;
        let bank_count = self.resources.chr_rom.len() / 0x0400;
        Some(bank as usize % bank_count * 0x0400 + offset)
    }
}

impl Mapper for TxSrom {
    fn mirroring(&self) -> Mirroring {
        // CIRAM selection is entirely supplied by `nametable_index` below.
        Mirroring::Vertical
    }

    fn nametable_index(&self, addr: u16) -> usize {
        let nametable_offset = (addr - 0x2000) & 0x0FFF;
        let (bank, _) = self
            .state
            .mmc3
            .chr_bank(nametable_offset)
            .expect("nametable offset is always in the first pattern table");
        let ciram_page = (bank >> 7) as usize;

        ciram_page * 0x0400 + (nametable_offset as usize & 0x03FF)
    }

    fn cpu_read(&self, addr: u16) -> Option<u8> {
        match addr {
            0x6000..=0x7FFF if self.state.prg_ram_control.enabled() => {
                Some(self.state.prg_ram[(addr - 0x6000) as usize])
            }
            0x6000..=0x7FFF => None,
            0x8000..=0xFFFF => {
                let offset = self
                    .state
                    .mmc3
                    .prg_rom_offset(addr, self.resources.prg_rom.len())?;
                Some(self.resources.prg_rom[offset])
            }
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8, _cpu_cycle: u64) {
        match addr {
            0x6000..=0x7FFF if self.state.prg_ram_control.writable() => {
                self.state.prg_ram[(addr - 0x6000) as usize] = value;
            }
            0xA001..=0xBFFF if addr & 1 == 1 => self.state.prg_ram_control.write(value),
            0x8000..=0xFFFF => self.state.mmc3.write_register(addr, value),
            _ => (),
        }
    }

    fn ppu_read(&self, addr: u16) -> Option<u8> {
        let offset = self.chr_offset(addr)?;
        Some(self.resources.chr_rom[offset])
    }

    fn ppu_write(&mut self, _addr: u16, _value: u8) {}

    fn observe_ppu_addr(&mut self, addr: u16, ppu_cycle: u64) {
        self.state.mmc3.observe_ppu_addr(addr, ppu_cycle);
    }

    fn irq_asserted(&self) -> bool {
        self.state.mmc3.irq_asserted()
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
    fn mirroring_register_is_absent_from_txsrom_state() {
        let prg_rom = vec![0; 0x8000];
        let chr_rom = vec![0; 0x2000];
        let mut board = TxSrom::new(&prg_rom, &chr_rom, Mirroring::Horizontal).unwrap();
        let before = board.state.clone();

        board.cpu_write(0xA000, 1, 0);

        assert_eq!(board.state, before);
    }
}
