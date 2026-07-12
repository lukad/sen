use crate::{
    cartridge::CartridgeError,
    mapper::{
        Mapper, Mirroring, SaveRamError,
        mmc3::{Mmc3PrgRamControl, Mmc3State},
    },
};

struct TxromResources {
    prg_rom: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TxromState {
    mmc3: Mmc3State,
    prg_ram: Box<[u8; 0x2000]>,
    prg_ram_control: Mmc3PrgRamControl,
}

enum TxromChr {
    Rom(Vec<u8>),
    Ram(TxromChrRamState),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TxromChrRamState {
    bytes: Box<[u8; 0x2000]>,
}

enum TxromMirroring {
    FourScreen,
    Programmable(TxromMirroringState),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TxromMirroringState {
    value: Mirroring,
}

impl TxromMirroring {
    fn new(initial: Mirroring) -> Self {
        if initial == Mirroring::FourScreen {
            Self::FourScreen
        } else {
            Self::Programmable(TxromMirroringState { value: initial })
        }
    }

    fn value(&self) -> Mirroring {
        match self {
            Self::FourScreen => Mirroring::FourScreen,
            Self::Programmable(state) => state.value,
        }
    }

    fn write(&mut self, value: u8) {
        let Self::Programmable(state) = self else {
            return;
        };

        state.value = if value & 1 == 0 {
            Mirroring::Vertical
        } else {
            Mirroring::Horizontal
        };
    }
}

pub(crate) struct Txrom {
    resources: TxromResources,
    state: TxromState,
    chr: TxromChr,
    mirroring: TxromMirroring,
}

impl Txrom {
    pub(crate) fn new(
        prg: &[u8],
        chr: &[u8],
        mirroring: Mirroring,
    ) -> Result<Self, CartridgeError> {
        validate_prg(prg, 0x80000)?;

        let chr = if chr.is_empty() {
            TxromChr::Ram(TxromChrRamState {
                bytes: Box::new([0; 0x2000]),
            })
        } else if chr.len().is_multiple_of(0x2000) && chr.len() <= 0x40000 {
            TxromChr::Rom(chr.to_vec())
        } else {
            return Err(CartridgeError::UnsupportedChrRomSize(chr.len()));
        };

        Ok(Self {
            resources: TxromResources {
                prg_rom: prg.to_vec(),
            },
            state: TxromState {
                mmc3: Mmc3State::new(),
                prg_ram: Box::new([0; 0x2000]),
                prg_ram_control: Mmc3PrgRamControl::new(),
            },
            chr,
            mirroring: TxromMirroring::new(mirroring),
        })
    }

    fn chr_len(&self) -> usize {
        match &self.chr {
            TxromChr::Rom(bytes) => bytes.len(),
            TxromChr::Ram(state) => state.bytes.len(),
        }
    }

    fn chr_offset(&self, addr: u16) -> Option<usize> {
        let (bank, offset) = self.state.mmc3.chr_bank(addr)?;
        let bank_count = self.chr_len() / 0x0400;
        Some(bank as usize % bank_count * 0x0400 + offset)
    }
}

impl Mapper for Txrom {
    fn mirroring(&self) -> Mirroring {
        self.mirroring.value()
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
            0xA000..=0xBFFE if addr & 1 == 0 => self.mirroring.write(value),
            0xA001..=0xBFFF if addr & 1 == 1 => self.state.prg_ram_control.write(value),
            0x8000..=0xFFFF => self.state.mmc3.write_register(addr, value),
            _ => (),
        }
    }

    fn ppu_read(&self, addr: u16) -> Option<u8> {
        let offset = self.chr_offset(addr)?;
        match &self.chr {
            TxromChr::Rom(bytes) => Some(bytes[offset]),
            TxromChr::Ram(state) => Some(state.bytes[offset]),
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let Some(offset) = self.chr_offset(addr) else {
            return;
        };

        if let TxromChr::Ram(state) = &mut self.chr {
            state.bytes[offset] = value;
        }
    }

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

pub(super) fn validate_prg(prg: &[u8], max_len: usize) -> Result<(), CartridgeError> {
    if prg.len() < 0x8000 || prg.len() > max_len || !prg.len().is_multiple_of(0x2000) {
        return Err(CartridgeError::UnsupportedPrgRomSize(prg.len()));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn four_screen_wiring_has_no_mutable_mirroring_state() {
        let prg_rom = vec![0; 0x8000];
        let chr_rom = vec![0; 0x2000];
        let mut board = Txrom::new(&prg_rom, &chr_rom, Mirroring::FourScreen).unwrap();
        let before = board.state.clone();

        board.cpu_write(0xA000, 1, 0);

        assert!(matches!(board.mirroring, TxromMirroring::FourScreen));
        assert_eq!(board.state, before);
        assert_eq!(board.mirroring(), Mirroring::FourScreen);
    }
}
