use crate::{
    cartridge::CartridgeError,
    mapper::{Chr, Mapper, Mirroring},
};

pub(crate) struct Nrom {
    prg_rom: NromPrgRom,
    chr: Chr,
    mirroring: Mirroring,
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

        let chr = Chr::new(chr)?;

        Ok(Self {
            prg_rom,
            chr,
            mirroring,
        })
    }
}

impl Mapper for Nrom {
    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn cpu_read(&self, addr: u16) -> Option<u8> {
        let offset = match addr {
            0x8000..=0xFFFF => (addr - 0x8000) as usize,
            _ => return None,
        };

        match &self.prg_rom {
            NromPrgRom::Nrom128(prg) => Some(prg[offset % 0x4000]),
            NromPrgRom::Nrom256(prg) => Some(prg[offset]),
        }
    }

    fn cpu_write(&mut self, _addr: u16, _value: u8) {}

    fn ppu_read(&self, addr: u16) -> Option<u8> {
        self.chr.read(addr)
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        self.chr.write(addr, value);
    }
}

enum NromPrgRom {
    Nrom128(Box<[u8; 0x4000]>),
    Nrom256(Box<[u8; 0x8000]>),
}
