use crate::mapper::{Mapper, Mirroring, cnrom::Cnrom, nrom::Nrom, uxrom::Uxrom};

pub struct Cartridge {
    mapper: Box<dyn Mapper>,
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

        let mapper: Box<dyn Mapper> = match mapper_id {
            0 => Box::new(Nrom::new(prg_slice, chr_slice, mirroring)?),
            2 => Box::new(Uxrom::new(prg_slice, chr_slice, mirroring)?),
            3 => Box::new(Cnrom::new(prg_slice, chr_slice, mirroring)?),
            other => return Err(CartridgeError::UnsupportedMapper(other)),
        };

        Ok(Self { mapper })
    }

    pub fn mirroring(&self) -> Mirroring {
        self.mapper.mirroring()
    }

    pub fn cpu_read(&self, addr: u16) -> Option<u8> {
        self.mapper.cpu_read(addr)
    }

    pub fn cpu_write(&mut self, addr: u16, value: u8) {
        self.mapper.cpu_write(addr, value);
    }

    pub fn ppu_read(&self, addr: u16) -> Option<u8> {
        self.mapper.ppu_read(addr)
    }

    pub fn ppu_write(&mut self, addr: u16, value: u8) {
        self.mapper.ppu_write(addr, value);
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

    fn prg_banks_with_ids(bank_count: usize) -> Vec<u8> {
        let mut prg_rom = Vec::with_capacity(bank_count * 0x4000);

        for bank in 0..bank_count {
            prg_rom.extend(std::iter::repeat_n(bank as u8, 0x4000));
        }

        prg_rom
    }

    fn chr_banks_with_ids(bank_count: usize) -> Vec<u8> {
        let mut chr_rom = Vec::with_capacity(bank_count * 0x2000);

        for bank in 0..bank_count {
            chr_rom.extend(std::iter::repeat_n(bank as u8, 0x2000));
        }

        chr_rom
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

    #[test]
    fn maps_uxrom_initial_switchable_bank_and_fixed_last_bank() {
        let prg_rom = prg_banks_with_ids(4);
        let rom = ines_rom(4, 0, 0x20, 0x00, None, &prg_rom, &[]);
        let cartridge = Cartridge::from_ines(&rom).unwrap();

        assert_eq!(cartridge.cpu_read(0x7FFF), None);
        assert_eq!(cartridge.cpu_read(0x8000), Some(0));
        assert_eq!(cartridge.cpu_read(0xBFFF), Some(0));
        assert_eq!(cartridge.cpu_read(0xC000), Some(3));
        assert_eq!(cartridge.cpu_read(0xFFFF), Some(3));
    }

    #[test]
    fn uxrom_cpu_write_switches_8000_window_and_keeps_c000_fixed() {
        let prg_rom = prg_banks_with_ids(4);
        let rom = ines_rom(4, 0, 0x20, 0x00, None, &prg_rom, &[]);
        let mut cartridge = Cartridge::from_ines(&rom).unwrap();

        cartridge.cpu_write(0x8000, 2);

        assert_eq!(cartridge.cpu_read(0x8000), Some(2));
        assert_eq!(cartridge.cpu_read(0xBFFF), Some(2));
        assert_eq!(cartridge.cpu_read(0xC000), Some(3));
        assert_eq!(cartridge.cpu_read(0xFFFF), Some(3));
    }

    #[test]
    fn uxrom_uses_full_8_bit_bank_select_register() {
        let prg_rom = prg_banks_with_ids(32);
        let rom = ines_rom(32, 0, 0x20, 0x00, None, &prg_rom, &[]);
        let mut cartridge = Cartridge::from_ines(&rom).unwrap();

        cartridge.cpu_write(0x8000, 0xFF);

        assert_eq!(cartridge.cpu_read(0x8000), Some(31));
        assert_eq!(cartridge.cpu_read(0xC000), Some(31));
    }

    #[test]
    fn uxrom_chr_ram_writes_through_ppu_bus() {
        let prg_rom = prg_banks_with_ids(2);
        let rom = ines_rom(2, 0, 0x20, 0x00, None, &prg_rom, &[]);
        let mut cartridge = Cartridge::from_ines(&rom).unwrap();

        assert_eq!(cartridge.ppu_read(0x0010), Some(0x00));

        cartridge.ppu_write(0x0010, 0xAB);

        assert_eq!(cartridge.ppu_read(0x0010), Some(0xAB));
    }

    #[test]
    fn cnrom_maps_fixed_16k_prg_rom_with_upper_bank_mirrored() {
        let prg_rom = patterned_bytes(0x4000);
        let chr_rom = chr_banks_with_ids(2);
        let rom = ines_rom(1, 2, 0x30, 0x00, None, &prg_rom, &chr_rom);
        let cartridge = Cartridge::from_ines(&rom).unwrap();

        assert_eq!(cartridge.cpu_read(0x7FFF), None);
        assert_eq!(cartridge.cpu_read(0x8000), Some(prg_rom[0x0000]));
        assert_eq!(cartridge.cpu_read(0xBFFF), Some(prg_rom[0x3FFF]));
        assert_eq!(cartridge.cpu_read(0xC000), Some(prg_rom[0x0000]));
        assert_eq!(cartridge.cpu_read(0xFFFF), Some(prg_rom[0x3FFF]));
    }

    #[test]
    fn cnrom_starts_with_chr_bank_zero_selected() {
        let prg_rom = vec![0xFF; 0x8000];
        let chr_rom = chr_banks_with_ids(4);
        let rom = ines_rom(2, 4, 0x30, 0x00, None, &prg_rom, &chr_rom);
        let cartridge = Cartridge::from_ines(&rom).unwrap();

        assert_eq!(cartridge.ppu_read(0x0000), Some(0));
        assert_eq!(cartridge.ppu_read(0x1FFF), Some(0));
        assert_eq!(cartridge.ppu_read(0x2000), None);
    }

    #[test]
    fn cnrom_cpu_write_switches_8k_chr_bank() {
        let prg_rom = vec![0xFF; 0x8000];
        let chr_rom = chr_banks_with_ids(4);
        let rom = ines_rom(2, 4, 0x30, 0x00, None, &prg_rom, &chr_rom);
        let mut cartridge = Cartridge::from_ines(&rom).unwrap();

        cartridge.cpu_write(0x8000, 2);

        assert_eq!(cartridge.ppu_read(0x0000), Some(2));
        assert_eq!(cartridge.ppu_read(0x1FFF), Some(2));
    }

    #[test]
    fn cnrom_bank_select_has_and_type_bus_conflict() {
        let mut prg_rom = vec![0xFF; 0x8000];
        prg_rom[0x0000] = 0x01;
        let chr_rom = chr_banks_with_ids(4);
        let rom = ines_rom(2, 4, 0x30, 0x00, None, &prg_rom, &chr_rom);
        let mut cartridge = Cartridge::from_ines(&rom).unwrap();

        cartridge.cpu_write(0x8000, 0x03);

        assert_eq!(cartridge.ppu_read(0x0000), Some(1));
    }

    #[test]
    fn cnrom_rejects_chr_ram() {
        let prg_rom = vec![0; 0x8000];
        let rom = ines_rom(2, 0, 0x30, 0x00, None, &prg_rom, &[]);

        let err = expect_err(Cartridge::from_ines(&rom));

        assert_eq!(err, CartridgeError::UnsupportedChrRomSize(0));
    }

    #[test]
    fn mapper_185_is_not_treated_as_plain_cnrom() {
        let prg_rom = vec![0; 0x8000];
        let chr_rom = vec![0; 0x2000];
        let rom = ines_rom(2, 1, 0x90, 0xB0, None, &prg_rom, &chr_rom);

        let err = expect_err(Cartridge::from_ines(&rom));

        assert_eq!(err, CartridgeError::UnsupportedMapper(185));
    }
}
