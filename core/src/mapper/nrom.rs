use bincode::{Decode, Encode};

use crate::{
    cartridge::CartridgeError,
    mapper::{ChrState, Mapper, Mirroring},
};

pub(crate) struct Nrom {
    resources: NromResources,
    pub(super) state: NromState,
}

struct NromResources {
    prg_rom: NromPrgRom,
    chr_rom: Option<Box<[u8; 0x2000]>>,
    mirroring: Mirroring,
}

enum NromPrgRom {
    Nrom128(Box<[u8; 0x4000]>),
    Nrom256(Box<[u8; 0x8000]>),
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub(crate) struct NromState {
    chr: ChrState,
}

impl Nrom {
    pub(crate) fn new(
        prg: &[u8],
        chr: &[u8],
        mirroring: Mirroring,
    ) -> Result<Self, CartridgeError> {
        let prg_rom = match prg.len() {
            0x4000 => NromPrgRom::Nrom128(Box::new(prg.try_into().unwrap())),
            0x8000 => NromPrgRom::Nrom256(Box::new(prg.try_into().unwrap())),
            other => return Err(CartridgeError::UnsupportedPrgRomSize(other)),
        };

        let (chr_rom, chr) = match chr.len() {
            0 => (None, ChrState::Ram(Box::new([0; 0x2000]))),
            0x2000 => (Some(Box::new(chr.try_into().unwrap())), ChrState::Rom),
            other => return Err(CartridgeError::UnsupportedChrRomSize(other)),
        };

        Ok(Self {
            resources: NromResources {
                prg_rom,
                chr_rom,
                mirroring,
            },
            state: NromState { chr },
        })
    }
}

impl Mapper for Nrom {
    fn mirroring(&self) -> Mirroring {
        self.resources.mirroring
    }

    fn cpu_read(&self, addr: u16) -> Option<u8> {
        let offset = match addr {
            0x8000..=0xFFFF => (addr - 0x8000) as usize,
            _ => return None,
        };

        match &self.resources.prg_rom {
            NromPrgRom::Nrom128(prg) => Some(prg[offset % 0x4000]),
            NromPrgRom::Nrom256(prg) => Some(prg[offset]),
        }
    }

    fn cpu_write(&mut self, _addr: u16, _value: u8, _cpu_cycle: u64) {}

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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chr_ram_is_cloneable_state_independent_of_the_live_board() {
        let prg_rom = vec![0; 0x4000];
        let mut nrom = Nrom::new(&prg_rom, &[], Mirroring::Horizontal).unwrap();

        nrom.ppu_write(0x0123, 0xAB);

        let captured = nrom.state.clone();

        nrom.ppu_write(0x0123, 0xCD);

        let ChrState::Ram(captured_chr_ram) = captured.chr else {
            panic!("expected CHR RAM");
        };
        assert_eq!(captured_chr_ram[0x0123], 0xAB);
        assert_eq!(nrom.ppu_read(0x0123), Some(0xCD));
    }

    #[test]
    fn chr_rom_bytes_are_an_immutable_resource_outside_nrom_state() {
        let prg_rom = vec![0; 0x4000];
        let mut chr_rom = vec![0; 0x2000];
        chr_rom[0x0123] = 0x5A;

        let mut nrom = Nrom::new(&prg_rom, &chr_rom, Mirroring::Horizontal).unwrap();
        assert!(matches!(&nrom.state.chr, ChrState::Rom));
        assert!(nrom.resources.chr_rom.is_some());

        nrom.ppu_write(0x0123, 0xA5);
        assert_eq!(nrom.ppu_read(0x0123), Some(0x5A));
    }
}
