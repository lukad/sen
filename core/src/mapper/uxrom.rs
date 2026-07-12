use bincode::{Decode, Encode};

use crate::{
    cartridge::CartridgeError,
    mapper::{ChrState, Mapper, Mirroring},
};

pub(crate) struct Uxrom {
    resources: UxromResources,
    pub(super) state: UxromState,
}

struct UxromResources {
    prg_rom: Vec<u8>,
    chr_rom: Option<Box<[u8; 0x2000]>>,
    mirroring: Mirroring,
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub(crate) struct UxromState {
    bank_select: u8,
    chr: ChrState,
}

impl Uxrom {
    pub(crate) fn new(
        prg: &[u8],
        chr: &[u8],
        mirroring: Mirroring,
    ) -> Result<Self, CartridgeError> {
        if prg.len() < 0x8000 || !prg.len().is_multiple_of(0x4000) {
            return Err(CartridgeError::UnsupportedPrgRomSize(prg.len()));
        }

        let (chr_rom, chr) = match chr.len() {
            0 => (None, ChrState::Ram(Box::new([0; 0x2000]))),
            0x2000 => (Some(Box::new(chr.try_into().unwrap())), ChrState::Rom),
            other => return Err(CartridgeError::UnsupportedChrRomSize(other)),
        };

        Ok(Self {
            resources: UxromResources {
                prg_rom: prg.to_vec(),
                chr_rom,
                mirroring,
            },
            state: UxromState {
                bank_select: 0,
                chr,
            },
        })
    }
}

impl Mapper for Uxrom {
    fn cpu_read(&self, addr: u16) -> Option<u8> {
        let bank_count = self.resources.prg_rom.len() / 0x4000;

        let (bank, offset_in_bank) = match addr {
            0x8000..=0xBFFF => {
                let bank = self.state.bank_select as usize % bank_count;
                (bank, (addr - 0x8000) as usize)
            }
            0xC000..=0xFFFF => {
                let bank = bank_count - 1;
                (bank, (addr - 0xC000) as usize)
            }
            _ => return None,
        };

        Some(self.resources.prg_rom[bank * 0x4000 + offset_in_bank])
    }

    fn cpu_write(&mut self, addr: u16, value: u8, _cpu_cycle: u64) {
        if matches!(addr, 0x8000..=0xFFFF) {
            self.state.bank_select = value;
        }
    }

    fn ppu_read(&self, addr: u16) -> Option<u8> {
        if !(0x0000..=0x1FFF).contains(&addr) {
            return None;
        }

        match &self.state.chr {
            ChrState::Rom => Some(
                self.resources
                    .chr_rom
                    .as_ref()
                    .expect("CHR ROM state always has a CHR ROM resource")[addr as usize],
            ),
            ChrState::Ram(chr_ram) => Some(chr_ram[addr as usize]),
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        if let (0x0000..=0x1FFF, ChrState::Ram(chr_ram)) = (addr, &mut self.state.chr) {
            chr_ram[addr as usize] = value;
        }
    }

    fn mirroring(&self) -> Mirroring {
        self.resources.mirroring
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complete_state_is_cloneable_independently_of_the_live_board() {
        let prg_rom = vec![0; 0x8000];
        let mut uxrom = Uxrom::new(&prg_rom, &[], Mirroring::Horizontal).unwrap();

        uxrom.cpu_write(0x8000, 1, 0);
        uxrom.ppu_write(0x0123, 0xAB);

        let captured = uxrom.state.clone();

        uxrom.cpu_write(0x8000, 0, 1);
        uxrom.ppu_write(0x0123, 0xCD);

        assert_eq!(captured.bank_select, 1);
        let ChrState::Ram(captured_chr_ram) = captured.chr else {
            panic!("expected CHR RAM");
        };
        assert_eq!(captured_chr_ram[0x0123], 0xAB);
        assert_eq!(uxrom.state.bank_select, 0);
        assert_eq!(uxrom.ppu_read(0x0123), Some(0xCD));
    }

    #[test]
    fn chr_rom_bytes_are_an_immutable_resource_outside_uxrom_state() {
        let prg_rom = vec![0; 0x8000];
        let mut chr_rom = vec![0; 0x2000];
        chr_rom[0x0123] = 0x5A;
        let mut uxrom = Uxrom::new(&prg_rom, &chr_rom, Mirroring::Vertical).unwrap();

        assert!(matches!(&uxrom.state.chr, ChrState::Rom));
        assert!(uxrom.resources.chr_rom.is_some());

        uxrom.ppu_write(0x0123, 0xA5);
        assert_eq!(uxrom.ppu_read(0x0123), Some(0x5A));
    }
}
