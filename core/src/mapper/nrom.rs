use crate::{
    cartridge::CartridgeError,
    mapper::{Mapper, Mirroring},
};

pub(crate) struct Nrom {
    resources: NromResources,
    chr: NromChr,
}

struct NromResources {
    prg_rom: NromPrgRom,
    mirroring: Mirroring,
}

enum NromPrgRom {
    Nrom128(Box<[u8; 0x4000]>),
    Nrom256(Box<[u8; 0x8000]>),
}

enum NromChr {
    Rom(Box<[u8; 0x2000]>),
    Ram(NromState),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NromState {
    chr_ram: Box<[u8; 0x2000]>,
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

        let chr = match chr.len() {
            0 => NromChr::Ram(NromState {
                chr_ram: Box::new([0; 0x2000]),
            }),
            0x2000 => NromChr::Rom(Box::new(chr.try_into().unwrap())),
            other => return Err(CartridgeError::UnsupportedChrRomSize(other)),
        };

        Ok(Self {
            resources: NromResources { prg_rom, mirroring },
            chr,
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

        match &self.chr {
            NromChr::Rom(chr_rom) => Some(chr_rom[addr as usize]),
            NromChr::Ram(state) => Some(state.chr_ram[addr as usize]),
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        if let (0x0000..=0x1FFF, NromChr::Ram(state)) = (addr, &mut self.chr) {
            state.chr_ram[addr as usize] = value;
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

        let captured = match &nrom.chr {
            NromChr::Ram(state) => state.clone(),
            NromChr::Rom(_) => panic!("expected CHR RAM"),
        };

        nrom.ppu_write(0x0123, 0xCD);

        assert_eq!(captured.chr_ram[0x0123], 0xAB);
        assert_eq!(nrom.ppu_read(0x0123), Some(0xCD));
    }

    #[test]
    fn chr_rom_is_an_immutable_resource_without_nrom_state() {
        let prg_rom = vec![0; 0x4000];
        let mut chr_rom = vec![0; 0x2000];
        chr_rom[0x0123] = 0x5A;

        let mut nrom = Nrom::new(&prg_rom, &chr_rom, Mirroring::Horizontal).unwrap();
        assert!(matches!(&nrom.chr, NromChr::Rom(_)));

        nrom.ppu_write(0x0123, 0xA5);
        assert_eq!(nrom.ppu_read(0x0123), Some(0x5A));
    }
}
