use crate::mapper::{
    Board, Mirroring, SaveRamError, cnrom::Cnrom, mmc1::Mmc1, nrom::Nrom, tqrom::Tqrom,
    txrom::Txrom, txsrom::TxSrom, uxrom::Uxrom,
};

pub struct Cartridge {
    board: Board,
    has_battery: bool,
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

        let has_battery = flags6 & 0x02 != 0;

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

        let board = match mapper_id {
            0 => Board::Nrom(Nrom::new(prg_slice, chr_slice, mirroring)?),
            1 => Board::Mmc1(Mmc1::new(prg_slice, chr_slice)?),
            2 => Board::Uxrom(Uxrom::new(prg_slice, chr_slice, mirroring)?),
            3 => Board::Cnrom(Cnrom::new(prg_slice, chr_slice, mirroring)?),
            4 => Board::Txrom(Txrom::new(prg_slice, chr_slice, mirroring)?),
            118 => Board::TxSrom(TxSrom::new(prg_slice, chr_slice, mirroring)?),
            119 => Board::Tqrom(Tqrom::new(prg_slice, chr_slice, mirroring)?),
            other => return Err(CartridgeError::UnsupportedMapper(other)),
        };

        Ok(Self { board, has_battery })
    }

    pub(crate) fn save_ram(&self) -> Option<&[u8]> {
        self.has_battery
            .then(|| self.board.as_mapper().save_ram())
            .flatten()
    }

    pub(crate) fn save_ram_mut(&mut self) -> Option<&mut [u8]> {
        self.has_battery
            .then(|| self.board.as_mapper_mut().save_ram_mut())
            .flatten()
    }

    pub(crate) fn load_save_ram(&mut self, data: &[u8]) -> Result<(), SaveRamError> {
        if !self.has_battery {
            return Err(SaveRamError::NotBatteryBacked);
        }

        self.board.as_mapper_mut().load_save_ram(data)
    }

    pub(crate) fn nametable_index(&self, addr: u16) -> usize {
        self.board.as_mapper().nametable_index(addr)
    }

    pub(crate) fn cpu_read(&self, addr: u16) -> Option<u8> {
        self.board.as_mapper().cpu_read(addr)
    }

    pub(crate) fn cpu_write(&mut self, addr: u16, value: u8, cycle_count: u64) {
        self.board
            .as_mapper_mut()
            .cpu_write(addr, value, cycle_count);
    }

    pub(crate) fn ppu_read(&self, addr: u16) -> Option<u8> {
        self.board.as_mapper().ppu_read(addr)
    }

    pub(crate) fn ppu_write(&mut self, addr: u16, value: u8) {
        self.board.as_mapper_mut().ppu_write(addr, value);
    }

    pub(crate) fn irq_asserted(&self) -> bool {
        self.board.as_mapper().irq_asserted()
    }

    pub(crate) fn observe_ppu_addr(&mut self, addr: u16, ppu_cycle: u64) {
        self.board.as_mapper_mut().observe_ppu_addr(addr, ppu_cycle);
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

    fn prg_8k_banks_with_ids(bank_count: usize) -> Vec<u8> {
        let mut prg_rom = Vec::with_capacity(bank_count * 0x2000);

        for bank in 0..bank_count {
            prg_rom.extend(std::iter::repeat_n(bank as u8, 0x2000));
        }

        prg_rom
    }

    fn chr_1k_banks_with_ids(bank_count: usize) -> Vec<u8> {
        let mut chr_rom = Vec::with_capacity(bank_count * 0x0400);

        for bank in 0..bank_count {
            chr_rom.extend(std::iter::repeat_n(bank as u8, 0x0400));
        }

        chr_rom
    }

    fn mmc3_rom(prg_rom: &[u8], chr_rom: &[u8], flags6_low: u8) -> Vec<u8> {
        ines_rom(
            (prg_rom.len() / 0x4000) as u8,
            (chr_rom.len() / 0x2000) as u8,
            0x40 | flags6_low,
            0x00,
            None,
            prg_rom,
            chr_rom,
        )
    }

    fn mmc1_rom(prg_rom: &[u8], chr_rom: &[u8], battery_backed: bool) -> Vec<u8> {
        let battery_flag = if battery_backed { 0x02 } else { 0x00 };

        ines_rom(
            (prg_rom.len() / 0x4000) as u8,
            (chr_rom.len() / 0x2000) as u8,
            0x10 | battery_flag,
            0x00,
            None,
            prg_rom,
            chr_rom,
        )
    }

    fn tqrom_rom(prg_rom: &[u8], chr_rom: &[u8]) -> Vec<u8> {
        ines_rom(
            (prg_rom.len() / 0x4000) as u8,
            (chr_rom.len() / 0x2000) as u8,
            0x70,
            0x70,
            None,
            prg_rom,
            chr_rom,
        )
    }

    fn txsrom_rom(prg_rom: &[u8], chr_rom: &[u8]) -> Vec<u8> {
        ines_rom(
            (prg_rom.len() / 0x4000) as u8,
            (chr_rom.len() / 0x2000) as u8,
            0x60,
            0x70,
            None,
            prg_rom,
            chr_rom,
        )
    }

    fn write_mmc3_bank(cartridge: &mut Cartridge, register: u8, value: u8) {
        cartridge.cpu_write(0x8000, register, 0);
        cartridge.cpu_write(0x8001, value, 0);
    }

    fn clock_mmc3_a12_rising_edge(cartridge: &mut Cartridge) {
        for cycle in 0..3 {
            cartridge.observe_ppu_addr(0x0000, cycle);
        }

        cartridge.observe_ppu_addr(0x1000, 8);
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
        let rom = ines_rom(1, 1, 0xF0, 0x00, None, &prg_rom, &chr_rom);

        let err = expect_err(Cartridge::from_ines(&rom));

        assert_eq!(err, CartridgeError::UnsupportedMapper(15));
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

        cartridge.cpu_write(0x8000, 2, 0);

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

        cartridge.cpu_write(0x8000, 0xFF, 0);

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

        cartridge.cpu_write(0x8000, 2, 0);

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

        cartridge.cpu_write(0x8000, 0x03, 0);

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

    #[test]
    fn mmc3_prg_mode_0_maps_r6_r7_and_fixed_banks() {
        let prg_rom = prg_8k_banks_with_ids(8);
        let chr_rom = vec![0; 0x2000];
        let rom = mmc3_rom(&prg_rom, &chr_rom, 0x00);
        let mut cartridge = Cartridge::from_ines(&rom).unwrap();

        write_mmc3_bank(&mut cartridge, 6, 2);
        write_mmc3_bank(&mut cartridge, 7, 4);

        assert_eq!(cartridge.cpu_read(0x8000), Some(2));
        assert_eq!(cartridge.cpu_read(0x9FFF), Some(2));
        assert_eq!(cartridge.cpu_read(0xA000), Some(4));
        assert_eq!(cartridge.cpu_read(0xBFFF), Some(4));
        assert_eq!(cartridge.cpu_read(0xC000), Some(6));
        assert_eq!(cartridge.cpu_read(0xDFFF), Some(6));
        assert_eq!(cartridge.cpu_read(0xE000), Some(7));
        assert_eq!(cartridge.cpu_read(0xFFFF), Some(7));
    }

    #[test]
    fn mmc3_prg_mode_1_swaps_r6_and_second_last_bank() {
        let prg_rom = prg_8k_banks_with_ids(8);
        let chr_rom = vec![0; 0x2000];
        let rom = mmc3_rom(&prg_rom, &chr_rom, 0x00);
        let mut cartridge = Cartridge::from_ines(&rom).unwrap();

        cartridge.cpu_write(0x8000, 0x40 | 6, 0);
        cartridge.cpu_write(0x8001, 2, 0);
        cartridge.cpu_write(0x8000, 0x40 | 7, 0);
        cartridge.cpu_write(0x8001, 4, 0);

        assert_eq!(cartridge.cpu_read(0x8000), Some(6));
        assert_eq!(cartridge.cpu_read(0x9FFF), Some(6));
        assert_eq!(cartridge.cpu_read(0xA000), Some(4));
        assert_eq!(cartridge.cpu_read(0xBFFF), Some(4));
        assert_eq!(cartridge.cpu_read(0xC000), Some(2));
        assert_eq!(cartridge.cpu_read(0xDFFF), Some(2));
        assert_eq!(cartridge.cpu_read(0xE000), Some(7));
        assert_eq!(cartridge.cpu_read(0xFFFF), Some(7));
    }

    #[test]
    fn mmc3_chr_normal_mode_maps_two_2k_and_four_1k_windows() {
        let prg_rom = prg_8k_banks_with_ids(4);
        let chr_rom = chr_1k_banks_with_ids(16);
        let rom = mmc3_rom(&prg_rom, &chr_rom, 0x00);
        let mut cartridge = Cartridge::from_ines(&rom).unwrap();

        write_mmc3_bank(&mut cartridge, 0, 3);
        write_mmc3_bank(&mut cartridge, 1, 5);
        write_mmc3_bank(&mut cartridge, 2, 6);
        write_mmc3_bank(&mut cartridge, 3, 7);
        write_mmc3_bank(&mut cartridge, 4, 8);
        write_mmc3_bank(&mut cartridge, 5, 9);

        assert_eq!(cartridge.ppu_read(0x0000), Some(2));
        assert_eq!(cartridge.ppu_read(0x03FF), Some(2));
        assert_eq!(cartridge.ppu_read(0x0400), Some(3));
        assert_eq!(cartridge.ppu_read(0x07FF), Some(3));
        assert_eq!(cartridge.ppu_read(0x0800), Some(4));
        assert_eq!(cartridge.ppu_read(0x0C00), Some(5));
        assert_eq!(cartridge.ppu_read(0x1000), Some(6));
        assert_eq!(cartridge.ppu_read(0x1400), Some(7));
        assert_eq!(cartridge.ppu_read(0x1800), Some(8));
        assert_eq!(cartridge.ppu_read(0x1FFF), Some(9));
    }

    #[test]
    fn mmc3_chr_inverted_mode_swaps_2k_and_1k_windows() {
        let prg_rom = prg_8k_banks_with_ids(4);
        let chr_rom = chr_1k_banks_with_ids(16);
        let rom = mmc3_rom(&prg_rom, &chr_rom, 0x00);
        let mut cartridge = Cartridge::from_ines(&rom).unwrap();

        cartridge.cpu_write(0x8000, 0x80, 0);
        cartridge.cpu_write(0x8001, 3, 0);
        cartridge.cpu_write(0x8000, 0x80 | 1, 0);
        cartridge.cpu_write(0x8001, 5, 0);
        cartridge.cpu_write(0x8000, 0x80 | 2, 0);
        cartridge.cpu_write(0x8001, 6, 0);
        cartridge.cpu_write(0x8000, 0x80 | 3, 0);
        cartridge.cpu_write(0x8001, 7, 0);
        cartridge.cpu_write(0x8000, 0x80 | 4, 0);
        cartridge.cpu_write(0x8001, 8, 0);
        cartridge.cpu_write(0x8000, 0x80 | 5, 0);
        cartridge.cpu_write(0x8001, 9, 0);

        assert_eq!(cartridge.ppu_read(0x0000), Some(6));
        assert_eq!(cartridge.ppu_read(0x0400), Some(7));
        assert_eq!(cartridge.ppu_read(0x0800), Some(8));
        assert_eq!(cartridge.ppu_read(0x0C00), Some(9));
        assert_eq!(cartridge.ppu_read(0x1000), Some(2));
        assert_eq!(cartridge.ppu_read(0x13FF), Some(2));
        assert_eq!(cartridge.ppu_read(0x1400), Some(3));
        assert_eq!(cartridge.ppu_read(0x1800), Some(4));
        assert_eq!(cartridge.ppu_read(0x1C00), Some(5));
    }

    #[test]
    fn mmc3_chr_rom_ignores_ppu_writes() {
        let prg_rom = prg_8k_banks_with_ids(4);
        let chr_rom = chr_1k_banks_with_ids(8);
        let rom = mmc3_rom(&prg_rom, &chr_rom, 0x00);
        let mut cartridge = Cartridge::from_ines(&rom).unwrap();

        write_mmc3_bank(&mut cartridge, 2, 6);
        assert_eq!(cartridge.ppu_read(0x1000), Some(6));

        cartridge.ppu_write(0x1000, 0xEE);

        assert_eq!(cartridge.ppu_read(0x1000), Some(6));
    }

    #[test]
    fn mmc3_chr_ram_writes_to_selected_chr_bank() {
        let prg_rom = prg_8k_banks_with_ids(4);
        let rom = mmc3_rom(&prg_rom, &[], 0x00);
        let mut cartridge = Cartridge::from_ines(&rom).unwrap();

        write_mmc3_bank(&mut cartridge, 2, 3);
        cartridge.ppu_write(0x1005, 0xAB);

        write_mmc3_bank(&mut cartridge, 3, 3);

        assert_eq!(cartridge.ppu_read(0x1405), Some(0xAB));
    }

    #[test]
    fn mmc3_mirroring_register_switches_horizontal_and_vertical() {
        let prg_rom = prg_8k_banks_with_ids(4);
        let chr_rom = vec![0; 0x2000];
        let rom = mmc3_rom(&prg_rom, &chr_rom, 0x00);
        let mut cartridge = Cartridge::from_ines(&rom).unwrap();

        cartridge.cpu_write(0xA000, 0, 0);
        assert_eq!(cartridge.nametable_index(0x2400), 0x0400);
        assert_eq!(cartridge.nametable_index(0x2800), 0x0000);

        cartridge.cpu_write(0xA000, 1, 0);
        assert_eq!(cartridge.nametable_index(0x2400), 0x0000);
        assert_eq!(cartridge.nametable_index(0x2800), 0x0400);
    }

    #[test]
    fn mmc3_four_screen_mirroring_ignores_mirroring_register() {
        let prg_rom = prg_8k_banks_with_ids(4);
        let chr_rom = vec![0; 0x2000];
        let rom = mmc3_rom(&prg_rom, &chr_rom, 0x08);
        let mut cartridge = Cartridge::from_ines(&rom).unwrap();

        cartridge.cpu_write(0xA000, 1, 0);
        assert_eq!(cartridge.nametable_index(0x2400), 0x0400);
        assert_eq!(cartridge.nametable_index(0x2800), 0x0800);

        cartridge.cpu_write(0xA000, 0, 0);
        assert_eq!(cartridge.nametable_index(0x2400), 0x0400);
        assert_eq!(cartridge.nametable_index(0x2800), 0x0800);
    }

    #[test]
    fn mmc3_prg_ram_reads_and_writes_when_enabled() {
        let prg_rom = prg_8k_banks_with_ids(4);
        let chr_rom = vec![0; 0x2000];
        let rom = mmc3_rom(&prg_rom, &chr_rom, 0x00);
        let mut cartridge = Cartridge::from_ines(&rom).unwrap();

        cartridge.cpu_write(0x6000, 0xAB, 0);

        assert_eq!(cartridge.cpu_read(0x6000), Some(0xAB));
    }

    #[test]
    fn battery_backed_mmc1_exposes_8k_of_save_ram() {
        let prg_rom = vec![0; 0x8000];
        let chr_rom = vec![0; 0x2000];
        let rom = mmc1_rom(&prg_rom, &chr_rom, true);
        let mut cartridge = Cartridge::from_ines(&rom).unwrap();

        assert_eq!(cartridge.save_ram().map(<[u8]>::len), Some(0x2000));
        assert_eq!(cartridge.save_ram_mut().map(|m| m.len()), Some(0x2000));
    }

    #[test]
    fn battery_backed_mmc1_cpu_writes_are_exported_as_save_ram() {
        let prg_rom = vec![0; 0x8000];
        let chr_rom = vec![0; 0x2000];
        let rom = mmc1_rom(&prg_rom, &chr_rom, true);
        let mut cartridge = Cartridge::from_ines(&rom).unwrap();

        cartridge.cpu_write(0x6000, 0xAB, 0);
        cartridge.cpu_write(0x7FFF, 0xCD, 0);

        let save_ram = cartridge.save_ram().unwrap();
        assert_eq!(save_ram[0x0000], 0xAB);
        assert_eq!(save_ram[0x1FFF], 0xCD);
    }

    #[test]
    fn loading_mmc1_save_ram_changes_cpu_reads() {
        let prg_rom = vec![0; 0x8000];
        let chr_rom = vec![0; 0x2000];
        let rom = mmc1_rom(&prg_rom, &chr_rom, true);
        let mut cartridge = Cartridge::from_ines(&rom).unwrap();
        let mut save_ram = vec![0; 0x2000];
        save_ram[0x0000] = 0x12;
        save_ram[0x1FFF] = 0x34;

        cartridge.load_save_ram(&save_ram).unwrap();

        assert_eq!(cartridge.cpu_read(0x6000), Some(0x12));
        assert_eq!(cartridge.cpu_read(0x7FFF), Some(0x34));
    }

    #[test]
    fn invalid_mmc1_save_ram_size_is_rejected_without_modifying_ram() {
        let prg_rom = vec![0; 0x8000];
        let chr_rom = vec![0; 0x2000];
        let rom = mmc1_rom(&prg_rom, &chr_rom, true);
        let mut cartridge = Cartridge::from_ines(&rom).unwrap();
        let original = vec![0xA5; 0x2000];
        cartridge.load_save_ram(&original).unwrap();

        let err = cartridge.load_save_ram(&vec![0x5A; 0x1FFF]).unwrap_err();

        assert_eq!(
            err,
            SaveRamError::InvalidSize {
                expected: 0x2000,
                actual: 0x1FFF,
            }
        );
        assert_eq!(cartridge.save_ram(), Some(original.as_slice()));
    }

    #[test]
    fn non_battery_backed_mmc1_does_not_expose_persistent_ram() {
        let prg_rom = vec![0; 0x8000];
        let chr_rom = vec![0; 0x2000];
        let rom = mmc1_rom(&prg_rom, &chr_rom, false);
        let mut cartridge = Cartridge::from_ines(&rom).unwrap();

        cartridge.cpu_write(0x6000, 0xAB, 0);

        assert_eq!(cartridge.cpu_read(0x6000), Some(0xAB));
        assert!(cartridge.save_ram().is_none());
        assert!(cartridge.save_ram_mut().is_none());
        assert_eq!(
            cartridge.load_save_ram(&vec![0; 0x2000]),
            Err(SaveRamError::NotBatteryBacked)
        );
    }

    #[test]
    fn loading_mmc3_save_ram_bypasses_cpu_write_protection() {
        let prg_rom = prg_8k_banks_with_ids(4);
        let chr_rom = vec![0; 0x2000];
        let rom = mmc3_rom(&prg_rom, &chr_rom, 0x02);
        let mut cartridge = Cartridge::from_ines(&rom).unwrap();
        cartridge.load_save_ram(&vec![0xA5; 0x2000]).unwrap();

        cartridge.cpu_write(0xA001, 0xC0, 0);
        cartridge.cpu_write(0x6000, 0xFF, 0);
        assert_eq!(cartridge.cpu_read(0x6000), Some(0xA5));

        cartridge.load_save_ram(&vec![0x5A; 0x2000]).unwrap();
        assert_eq!(cartridge.cpu_read(0x6000), Some(0x5A));
    }

    #[test]
    fn mmc3_irq_asserts_after_filtered_a12_edges() {
        let prg_rom = prg_8k_banks_with_ids(4);
        let chr_rom = vec![0; 0x2000];
        let rom = mmc3_rom(&prg_rom, &chr_rom, 0x00);
        let mut cartridge = Cartridge::from_ines(&rom).unwrap();

        cartridge.cpu_write(0xC000, 2, 0);
        cartridge.cpu_write(0xC001, 0, 0);
        cartridge.cpu_write(0xE001, 0, 0);

        clock_mmc3_a12_rising_edge(&mut cartridge);
        assert!(!cartridge.irq_asserted());

        clock_mmc3_a12_rising_edge(&mut cartridge);
        assert!(!cartridge.irq_asserted());

        clock_mmc3_a12_rising_edge(&mut cartridge);
        assert!(cartridge.irq_asserted());

        cartridge.cpu_write(0xE000, 0, 0);
        assert!(!cartridge.irq_asserted());
    }

    #[test]
    fn tqrom_uses_mmc3_prg_banking() {
        let prg_rom = prg_8k_banks_with_ids(8);
        let chr_rom = chr_1k_banks_with_ids(64);
        let rom = tqrom_rom(&prg_rom, &chr_rom);
        let mut cartridge = Cartridge::from_ines(&rom).unwrap();

        write_mmc3_bank(&mut cartridge, 6, 3);
        write_mmc3_bank(&mut cartridge, 7, 5);

        assert_eq!(cartridge.cpu_read(0x8000), Some(3));
        assert_eq!(cartridge.cpu_read(0xA000), Some(5));
        assert_eq!(cartridge.cpu_read(0xC000), Some(6));
        assert_eq!(cartridge.cpu_read(0xE000), Some(7));
    }

    #[test]
    fn tqrom_chr_bank_bit_6_selects_chr_ram_instead_of_chr_rom() {
        let prg_rom = prg_8k_banks_with_ids(8);
        let chr_rom = chr_1k_banks_with_ids(64);
        let rom = tqrom_rom(&prg_rom, &chr_rom);
        let mut cartridge = Cartridge::from_ines(&rom).unwrap();

        write_mmc3_bank(&mut cartridge, 2, 5);
        assert_eq!(cartridge.ppu_read(0x1000), Some(5));

        write_mmc3_bank(&mut cartridge, 3, 0x40 | 5);
        assert_eq!(cartridge.ppu_read(0x1400), Some(0));
        cartridge.ppu_write(0x1400, 0xAB);
        assert_eq!(cartridge.ppu_read(0x1400), Some(0xAB));

        write_mmc3_bank(&mut cartridge, 4, 0x40 | 5);
        assert_eq!(cartridge.ppu_read(0x1800), Some(0xAB));
    }

    #[test]
    fn tqrom_ignores_writes_to_chr_rom_banks() {
        let prg_rom = prg_8k_banks_with_ids(8);
        let chr_rom = chr_1k_banks_with_ids(64);
        let rom = tqrom_rom(&prg_rom, &chr_rom);
        let mut cartridge = Cartridge::from_ines(&rom).unwrap();

        write_mmc3_bank(&mut cartridge, 2, 12);
        cartridge.ppu_write(0x1000, 0xEE);

        assert_eq!(cartridge.ppu_read(0x1000), Some(12));
    }

    #[test]
    fn tqrom_uses_mmc3_irq_counter() {
        let prg_rom = prg_8k_banks_with_ids(8);
        let chr_rom = chr_1k_banks_with_ids(64);
        let rom = tqrom_rom(&prg_rom, &chr_rom);
        let mut cartridge = Cartridge::from_ines(&rom).unwrap();

        cartridge.cpu_write(0xC000, 1, 0);
        cartridge.cpu_write(0xC001, 0, 0);
        cartridge.cpu_write(0xE001, 0, 0);

        clock_mmc3_a12_rising_edge(&mut cartridge);
        assert!(!cartridge.irq_asserted());

        clock_mmc3_a12_rising_edge(&mut cartridge);
        assert!(cartridge.irq_asserted());
    }

    #[test]
    fn txsrom_chr_bank_msb_selects_each_nametable_page() {
        let prg_rom = prg_8k_banks_with_ids(8);
        let chr_rom = chr_1k_banks_with_ids(128);
        let rom = txsrom_rom(&prg_rom, &chr_rom);
        let mut cartridge = Cartridge::from_ines(&rom).unwrap();

        for (register, value) in [(2, 0x80), (3, 0x00), (4, 0x80), (5, 0x00)] {
            cartridge.cpu_write(0x8000, 0x80 | register, 0);
            cartridge.cpu_write(0x8001, value, 0);
        }

        assert_eq!(cartridge.nametable_index(0x2000), 0x0400);
        assert_eq!(cartridge.nametable_index(0x23FF), 0x07FF);
        assert_eq!(cartridge.nametable_index(0x2400), 0x0000);
        assert_eq!(cartridge.nametable_index(0x2800), 0x0400);
        assert_eq!(cartridge.nametable_index(0x2C00), 0x0000);
        assert_eq!(cartridge.nametable_index(0x3000), 0x0400);
    }

    #[test]
    fn txsrom_mirroring_register_does_not_override_chr_controlled_nametables() {
        let prg_rom = prg_8k_banks_with_ids(8);
        let chr_rom = chr_1k_banks_with_ids(128);
        let rom = txsrom_rom(&prg_rom, &chr_rom);
        let mut cartridge = Cartridge::from_ines(&rom).unwrap();

        write_mmc3_bank(&mut cartridge, 0, 0x00);
        write_mmc3_bank(&mut cartridge, 1, 0x80);
        cartridge.cpu_write(0xA000, 0, 0);

        assert_eq!(cartridge.nametable_index(0x2000), 0x0000);
        assert_eq!(cartridge.nametable_index(0x2400), 0x0000);
        assert_eq!(cartridge.nametable_index(0x2800), 0x0400);
        assert_eq!(cartridge.nametable_index(0x2C00), 0x0400);
    }

    #[test]
    fn txsrom_chr_bank_msb_aliases_with_128k_chr_rom() {
        let prg_rom = prg_8k_banks_with_ids(8);
        let chr_rom = chr_1k_banks_with_ids(128);
        let rom = txsrom_rom(&prg_rom, &chr_rom);
        let mut cartridge = Cartridge::from_ines(&rom).unwrap();

        write_mmc3_bank(&mut cartridge, 2, 0x80 | 5);

        assert_eq!(cartridge.ppu_read(0x1000), Some(5));
    }
}
