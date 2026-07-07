pub struct Cartridge {
    mapper: Mapper,
    chr: Chr,
    mirroring: Mirroring,
}

enum Mapper {
    Nrom(Nrom),
}

impl Mapper {
    fn cpu_read(&self, addr: u16) -> Option<u8> {
        match self {
            Mapper::Nrom(nrom) => nrom.cpu_read(addr),
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match self {
            Mapper::Nrom(nrom) => nrom.cpu_write(addr, value),
        }
    }
}

struct Nrom {
    prg_rom: NromPrgRom,
}

impl Nrom {
    pub fn cpu_read(&self, addr: u16) -> Option<u8> {
        let offset = match addr {
            0x8000..=0xFFFF => (addr - 0x8000) as usize,
            _ => return None,
        };

        match &self.prg_rom {
            NromPrgRom::Nrom128(prg) => Some(prg[offset % 0x4000]),
            NromPrgRom::Nrom256(prg) => Some(prg[offset]),
        }
    }

    pub fn cpu_write(&mut self, _addr: u16, _value: u8) {}
}

enum NromPrgRom {
    Nrom128(Box<[u8; 0x4000]>),
    Nrom256(Box<[u8; 0x8000]>),
}

enum Chr {
    Rom(Box<[u8; 0x2000]>),
    Ram(Box<[u8; 0x2000]>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mirroring {
    Horizontal,
    Vertical,
    FourScreen,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CartridgeError {
    #[error("invalid iNES header")]
    InvalidHeader,
    #[error("unsupported iNES version")]
    UnsupportedVersion,
    #[error("unsupported mapper {0}")]
    UnsupportedMapper(u8),
    #[error("unsupported PRG ROM size {0}")]
    UnsupportedPrgRomSize(usize),
    #[error("ROM is truncated")]
    Truncated,
    #[error("unsupported CHR ROM size {0}")]
    UnsupportedChrRomSize(usize),
}

impl Cartridge {
    pub fn from_ines(bytes: &[u8]) -> Result<Self, CartridgeError> {
        if bytes.len() < 16 || &bytes[0..4] != b"NES\x1A" {
            return Err(CartridgeError::InvalidHeader);
        }

        let prg_banks = bytes[4] as usize;
        let chr_banks = bytes[5] as usize;
        let flags6 = bytes[6];
        let flags7 = bytes[7];

        if flags7 & 0x0C == 0x08 {
            return Err(CartridgeError::UnsupportedVersion);
        }

        let mirroring = if flags6 & 0x08 != 0 {
            Mirroring::FourScreen
        } else if flags6 & 0x01 != 0 {
            Mirroring::Vertical
        } else {
            Mirroring::Horizontal
        };

        let mapper_id = (flags6 >> 4) | (flags7 & 0xF0);
        if mapper_id != 0 {
            return Err(CartridgeError::UnsupportedMapper(mapper_id));
        }

        let has_trainer = flags6 & 0x04 != 0;
        let prg_start = 16 + if has_trainer { 512 } else { 0 };
        let prg_len = prg_banks * 16 * 1024;
        let chr_len = chr_banks * 8 * 1024;

        let chr_start = prg_start + prg_len;
        let total_len = chr_start + chr_len;

        if bytes.len() < total_len {
            return Err(CartridgeError::Truncated);
        }

        let prg_slice = &bytes[prg_start..prg_start + prg_len];
        let chr_slice = &bytes[chr_start..chr_start + chr_len];

        let prg_rom = match prg_len {
            0x4000 => NromPrgRom::Nrom128(Box::new(prg_slice.try_into().unwrap())),
            0x8000 => NromPrgRom::Nrom256(Box::new(prg_slice.try_into().unwrap())),
            other => return Err(CartridgeError::UnsupportedPrgRomSize(other)),
        };

        let mapper = match mapper_id {
            0 => Mapper::Nrom(Nrom { prg_rom }),
            other => return Err(CartridgeError::UnsupportedMapper(other)),
        };

        let chr = match chr_len {
            0 => Chr::Ram(Box::new([0; 0x2000])),
            0x2000 => Chr::Rom(Box::new(chr_slice.try_into().unwrap())),
            other => return Err(CartridgeError::UnsupportedChrRomSize(other)),
        };

        Ok(Self {
            mapper,
            chr,
            mirroring,
        })
    }

    pub fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    pub fn cpu_read(&self, addr: u16) -> Option<u8> {
        self.mapper.cpu_read(addr)
    }

    pub fn cpu_write(&mut self, addr: u16, value: u8) {
        self.mapper.cpu_write(addr, value);
    }

    pub fn ppu_read(&self, addr: u16) -> Option<u8> {
        match addr {
            0x0000..=0x1FFF => match &self.chr {
                Chr::Rom(bytes) | Chr::Ram(bytes) => Some(bytes[addr as usize]),
            },
            _ => None,
        }
    }

    pub fn ppu_write(&mut self, addr: u16, value: u8) {
        if let (0x0000..=0x1FFF, Chr::Ram(bytes)) = (addr, &mut self.chr) {
            bytes[addr as usize] = value;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expect_err(result: Result<Cartridge, CartridgeError>) -> CartridgeError {
        match result {
            Ok(_) => panic!("expected Cartridge::from_ines to fail"),
            Err(err) => err,
        }
    }

    fn ines_rom(
        prg_banks: u8,
        chr_banks: u8,
        flags6: u8,
        flags7: u8,
        trainer: Option<&[u8]>,
        prg_rom: &[u8],
        chr_rom: &[u8],
    ) -> Vec<u8> {
        let mut rom = vec![0; 16];
        rom[0..4].copy_from_slice(b"NES\x1A");
        rom[4] = prg_banks;
        rom[5] = chr_banks;
        rom[6] = flags6;
        rom[7] = flags7;

        if let Some(trainer) = trainer {
            assert_eq!(trainer.len(), 512);
            rom.extend_from_slice(trainer);
        }

        rom.extend_from_slice(prg_rom);
        rom.extend_from_slice(chr_rom);
        rom
    }

    fn patterned_bytes(len: usize) -> Vec<u8> {
        (0..len).map(|i| (i & 0xFF) as u8).collect()
    }

    #[test]
    fn rejects_invalid_header() {
        let err = expect_err(Cartridge::from_ines(b"not a rom"));

        assert_eq!(err, CartridgeError::InvalidHeader);
    }

    #[test]
    fn rejects_nes_2_0_header() {
        let prg_rom = vec![0; 0x4000];
        let chr_rom = vec![0; 0x2000];
        let rom = ines_rom(1, 1, 0x00, 0x08, None, &prg_rom, &chr_rom);

        let err = expect_err(Cartridge::from_ines(&rom));

        assert_eq!(err, CartridgeError::UnsupportedVersion);
    }

    #[test]
    fn rejects_unsupported_mapper() {
        let prg_rom = vec![0; 0x4000];
        let chr_rom = vec![0; 0x2000];
        let rom = ines_rom(1, 1, 0x10, 0x00, None, &prg_rom, &chr_rom);

        let err = expect_err(Cartridge::from_ines(&rom));

        assert_eq!(err, CartridgeError::UnsupportedMapper(1));
    }

    #[test]
    fn rejects_truncated_rom_data() {
        let prg_rom = vec![0; 0x3FFF];
        let chr_rom = vec![0; 0x2000];
        let rom = ines_rom(1, 1, 0x00, 0x00, None, &prg_rom, &chr_rom);

        let err = expect_err(Cartridge::from_ines(&rom));

        assert_eq!(err, CartridgeError::Truncated);
    }

    #[test]
    fn maps_16k_nrom_prg_rom_with_upper_bank_mirrored() {
        let prg_rom = patterned_bytes(0x4000);
        let chr_rom = vec![0; 0x2000];
        let rom = ines_rom(1, 1, 0x00, 0x00, None, &prg_rom, &chr_rom);
        let cartridge = Cartridge::from_ines(&rom).unwrap();

        assert_eq!(cartridge.cpu_read(0x7FFF), None);
        assert_eq!(cartridge.cpu_read(0x8000), Some(prg_rom[0x0000]));
        assert_eq!(cartridge.cpu_read(0xBFFF), Some(prg_rom[0x3FFF]));
        assert_eq!(cartridge.cpu_read(0xC000), Some(prg_rom[0x0000]));
        assert_eq!(cartridge.cpu_read(0xFFFF), Some(prg_rom[0x3FFF]));
    }

    #[test]
    fn maps_32k_nrom_prg_rom_directly() {
        let prg_rom = patterned_bytes(0x8000);
        let chr_rom = vec![0; 0x2000];
        let rom = ines_rom(2, 1, 0x00, 0x00, None, &prg_rom, &chr_rom);
        let cartridge = Cartridge::from_ines(&rom).unwrap();

        assert_eq!(cartridge.cpu_read(0x8000), Some(prg_rom[0x0000]));
        assert_eq!(cartridge.cpu_read(0xBFFF), Some(prg_rom[0x3FFF]));
        assert_eq!(cartridge.cpu_read(0xC000), Some(prg_rom[0x4000]));
        assert_eq!(cartridge.cpu_read(0xFFFF), Some(prg_rom[0x7FFF]));
    }

    #[test]
    fn skips_trainer_before_prg_rom() {
        let trainer = vec![0xEE; 512];
        let mut prg_rom = vec![0; 0x4000];
        prg_rom[0] = 0x42;
        prg_rom[0x3FFF] = 0x99;
        let chr_rom = vec![0; 0x2000];
        let rom = ines_rom(1, 1, 0x04, 0x00, Some(&trainer), &prg_rom, &chr_rom);
        let cartridge = Cartridge::from_ines(&rom).unwrap();

        assert_eq!(cartridge.cpu_read(0x8000), Some(0x42));
        assert_eq!(cartridge.cpu_read(0xBFFF), Some(0x99));
        assert_eq!(cartridge.cpu_read(0xC000), Some(0x42));
    }
}
