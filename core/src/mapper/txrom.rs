use crate::{
    cartridge::CartridgeError,
    mapper::{
        ChrState, Mapper, Mirroring, SaveRamError,
        mmc3::{Mmc3PrgRamControl, Mmc3State},
    },
};

struct TxromResources {
    prg_rom: Vec<u8>,
    chr_rom: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TxromState {
    mmc3: Mmc3State,
    prg_ram: Box<[u8; 0x2000]>,
    prg_ram_control: Mmc3PrgRamControl,
    chr: ChrState,
    mirroring: TxromMirroringState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TxromMirroringState {
    FourScreen,
    Programmable(Mirroring),
}

impl TxromMirroringState {
    fn new(initial: Mirroring) -> Self {
        if initial == Mirroring::FourScreen {
            Self::FourScreen
        } else {
            Self::Programmable(initial)
        }
    }

    fn value(&self) -> Mirroring {
        match self {
            Self::FourScreen => Mirroring::FourScreen,
            Self::Programmable(value) => *value,
        }
    }

    fn write(&mut self, value: u8) {
        let Self::Programmable(mirroring) = self else {
            return;
        };

        *mirroring = if value & 1 == 0 {
            Mirroring::Vertical
        } else {
            Mirroring::Horizontal
        };
    }
}

pub(crate) struct Txrom {
    resources: TxromResources,
    pub(super) state: TxromState,
}

impl Txrom {
    pub(crate) fn new(
        prg: &[u8],
        chr: &[u8],
        mirroring: Mirroring,
    ) -> Result<Self, CartridgeError> {
        validate_prg(prg, 0x80000)?;

        let (chr_rom, chr) = if chr.is_empty() {
            (None, ChrState::Ram(Box::new([0; 0x2000])))
        } else if chr.len().is_multiple_of(0x2000) && chr.len() <= 0x40000 {
            (Some(chr.to_vec()), ChrState::Rom)
        } else {
            return Err(CartridgeError::UnsupportedChrRomSize(chr.len()));
        };

        Ok(Self {
            resources: TxromResources {
                prg_rom: prg.to_vec(),
                chr_rom,
            },
            state: TxromState {
                mmc3: Mmc3State::new(),
                prg_ram: Box::new([0; 0x2000]),
                prg_ram_control: Mmc3PrgRamControl::new(),
                chr,
                mirroring: TxromMirroringState::new(mirroring),
            },
        })
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
        let (bank, offset) = self.state.mmc3.chr_bank(addr)?;
        let bank_count = self.chr_len() / 0x0400;
        Some(bank as usize % bank_count * 0x0400 + offset)
    }
}

impl Mapper for Txrom {
    fn mirroring(&self) -> Mirroring {
        self.state.mirroring.value()
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
            0xA000..=0xBFFE if addr & 1 == 0 => self.state.mirroring.write(value),
            0xA001..=0xBFFF if addr & 1 == 1 => self.state.prg_ram_control.write(value),
            0x8000..=0xFFFF => self.state.mmc3.write_register(addr, value),
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
    fn complete_state_is_cloneable_independently_of_the_live_board() {
        let prg_rom = vec![0; 0x8000];
        let mut board = Txrom::new(&prg_rom, &[], Mirroring::Vertical).unwrap();

        board.cpu_write(0x6000, 0xAB, 0);
        board.cpu_write(0xA000, 1, 0);
        board.ppu_write(0x0123, 0xBC);

        let captured = board.state.clone();

        board.cpu_write(0x6000, 0xCD, 0);
        board.cpu_write(0xA000, 0, 0);
        board.ppu_write(0x0123, 0xDE);

        assert_eq!(captured.prg_ram[0], 0xAB);
        let ChrState::Ram(captured_chr_ram) = captured.chr else {
            panic!("expected CHR RAM");
        };
        assert_eq!(captured_chr_ram[0x0123], 0xBC);
        assert_eq!(
            captured.mirroring,
            TxromMirroringState::Programmable(Mirroring::Horizontal)
        );

        assert_eq!(board.cpu_read(0x6000), Some(0xCD));
        assert_eq!(board.ppu_read(0x0123), Some(0xDE));
        assert_eq!(board.mirroring(), Mirroring::Vertical);
    }

    #[test]
    fn four_screen_wiring_has_no_mutable_mirroring_state() {
        let prg_rom = vec![0; 0x8000];
        let chr_rom = vec![0; 0x2000];
        let mut board = Txrom::new(&prg_rom, &chr_rom, Mirroring::FourScreen).unwrap();
        let before = board.state.clone();

        board.cpu_write(0xA000, 1, 0);

        assert!(matches!(
            board.state.mirroring,
            TxromMirroringState::FourScreen
        ));
        assert_eq!(board.state, before);
        assert_eq!(board.mirroring(), Mirroring::FourScreen);
    }
}
