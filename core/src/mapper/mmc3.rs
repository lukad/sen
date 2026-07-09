use crate::{
    cartridge::CartridgeError,
    mapper::{Mapper, Mirroring},
};

enum Mmc3Chr {
    Rom(Vec<u8>),
    Ram(Vec<u8>),
}

pub(crate) struct Mmc3 {
    prg_rom: Vec<u8>,
    prg_ram: Box<[u8; 0x2000]>,
    chr: Mmc3Chr,
    mirroring: Mirroring,
    four_screen: bool,
    bank_select: u8,
    bank_registers: [u8; 8],
    prg_ram_enabled: bool,
    prg_ram_write_protect: bool,
    irq_latch: u8,
    irq_counter: u8,
    irq_reload: bool,
    irq_enabled: bool,
    irq_pending: bool,
    last_ppu_a12: bool,
    a12_low_since: Option<u64>,
}

impl Mmc3 {
    pub(crate) fn new(
        prg: &[u8],
        chr: &[u8],
        mirroring: Mirroring,
    ) -> Result<Self, CartridgeError> {
        if prg.len() < 0x8000 || prg.len() > 0x80000 || !prg.len().is_multiple_of(0x2000) {
            return Err(CartridgeError::UnsupportedPrgRomSize(prg.len()));
        }

        let chr = if chr.is_empty() {
            Mmc3Chr::Ram(vec![0; 0x2000])
        } else if chr.len().is_multiple_of(0x2000) && chr.len() <= 0x40000 {
            Mmc3Chr::Rom(chr.to_vec())
        } else {
            return Err(CartridgeError::UnsupportedChrRomSize(chr.len()));
        };

        Ok(Self {
            prg_rom: prg.to_vec(),
            prg_ram: Box::new([0; 0x2000]),
            chr,
            mirroring,
            four_screen: mirroring == Mirroring::FourScreen,
            bank_select: 0,
            bank_registers: [0; 8],
            prg_ram_enabled: true,
            prg_ram_write_protect: false,
            irq_latch: 0,
            irq_counter: 0,
            irq_reload: false,
            irq_enabled: false,
            irq_pending: false,
            last_ppu_a12: false,
            a12_low_since: None,
        })
    }

    fn prg_rom_offset(&self, addr: u16) -> Option<usize> {
        if !(0x8000..=0xFFFF).contains(&addr) {
            return None;
        }

        let bank_count = self.prg_rom.len() / 0x2000;
        let second_last_bank = bank_count - 2;
        let last_bank = bank_count - 1;

        let r6 = (self.bank_registers[6] & 0x3F) as usize % bank_count;
        let r7 = (self.bank_registers[7] & 0x3F) as usize % bank_count;

        let prg_mode = self.bank_select & 0x40 != 0;

        let bank = match addr {
            0x8000..=0x9FFF => {
                if prg_mode {
                    second_last_bank
                } else {
                    r6
                }
            }
            0xA000..=0xBFFF => r7,
            0xC000..=0xDFFF => {
                if prg_mode {
                    r6
                } else {
                    second_last_bank
                }
            }
            0xE000..=0xFFFF => last_bank,
            _ => unreachable!(),
        };

        let offset_in_bank = addr as usize & 0x1FFF;

        Some(bank * 0x2000 + offset_in_bank)
    }

    fn clock_irq_counter(&mut self) {
        if self.irq_counter == 0 || self.irq_reload {
            self.irq_counter = self.irq_latch;
            self.irq_reload = false;
        } else {
            self.irq_counter -= 1;
        }

        if self.irq_counter == 0 && self.irq_enabled {
            self.irq_pending = true;
        }
    }

    fn chr_1k_bank(&self, value: u8) -> usize {
        value as usize % self.chr_bank_count_1k()
    }

    fn chr_2k_bank(&self, value: u8) -> usize {
        (value as usize & !1) % self.chr_bank_count_1k()
    }

    fn chr_len(&self) -> usize {
        match &self.chr {
            Mmc3Chr::Rom(bytes) | Mmc3Chr::Ram(bytes) => bytes.len(),
        }
    }

    fn chr_bank_count_1k(&self) -> usize {
        self.chr_len() / 0x0400
    }

    fn chr_offset(&self, addr: u16) -> Option<usize> {
        if !matches!(addr, 0x0000..=0x1FFF) {
            return None;
        }

        let chr_inverted = self.bank_select & 0x80 != 0;

        let (bank, offset_in_bank) = if !chr_inverted {
            match addr {
                0x0000..=0x07FF => (self.chr_2k_bank(self.bank_registers[0]), addr as usize),
                0x0800..=0x0FFF => (
                    self.chr_2k_bank(self.bank_registers[1]),
                    (addr - 0x0800) as usize,
                ),
                0x1000..=0x13FF => (
                    self.chr_1k_bank(self.bank_registers[2]),
                    (addr - 0x1000) as usize,
                ),
                0x1400..=0x17FF => (
                    self.chr_1k_bank(self.bank_registers[3]),
                    (addr - 0x1400) as usize,
                ),
                0x1800..=0x1BFF => (
                    self.chr_1k_bank(self.bank_registers[4]),
                    (addr - 0x1800) as usize,
                ),
                0x1C00..=0x1FFF => (
                    self.chr_1k_bank(self.bank_registers[5]),
                    (addr - 0x1C00) as usize,
                ),
                _ => unreachable!(),
            }
        } else {
            match addr {
                0x0000..=0x03FF => (self.chr_1k_bank(self.bank_registers[2]), addr as usize),
                0x0400..=0x07FF => (
                    self.chr_1k_bank(self.bank_registers[3]),
                    (addr - 0x0400) as usize,
                ),
                0x0800..=0x0BFF => (
                    self.chr_1k_bank(self.bank_registers[4]),
                    (addr - 0x0800) as usize,
                ),
                0x0C00..=0x0FFF => (
                    self.chr_1k_bank(self.bank_registers[5]),
                    (addr - 0x0C00) as usize,
                ),
                0x1000..=0x17FF => (
                    self.chr_2k_bank(self.bank_registers[0]),
                    (addr - 0x1000) as usize,
                ),
                0x1800..=0x1FFF => (
                    self.chr_2k_bank(self.bank_registers[1]),
                    (addr - 0x1800) as usize,
                ),
                _ => unreachable!(),
            }
        };

        Some(bank * 0x0400 + offset_in_bank)
    }
}

impl Mapper for Mmc3 {
    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn cpu_read(&self, addr: u16) -> Option<u8> {
        match addr {
            0x6000..=0x7FFF if self.prg_ram_enabled => Some(self.prg_ram[(addr - 0x6000) as usize]),
            0x6000..=0x7FFF => None,
            0x8000..=0xFFFF => {
                let offset = self.prg_rom_offset(addr)?;
                Some(self.prg_rom[offset])
            }
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8, _cpu_cycle: u64) {
        match addr {
            0x6000..=0x7FFF if self.prg_ram_enabled && !self.prg_ram_write_protect => {
                self.prg_ram[(addr - 0x6000) as usize] = value
            }
            0x8000..=0x9FFE if addr & 1 == 0 => {
                self.bank_select = value;
            }
            0x8001..=0x9FFF if addr & 1 == 1 => {
                let register = self.bank_select & 0x07;
                self.bank_registers[register as usize] = value;
            }
            0xA000..=0xBFFE if addr & 1 == 0 && !self.four_screen => {
                self.mirroring = if value & 1 == 0 {
                    Mirroring::Vertical
                } else {
                    Mirroring::Horizontal
                }
            }
            0xA001..=0xBFFF if addr & 1 == 1 => {
                self.prg_ram_enabled = value & 0x80 != 0;
                self.prg_ram_write_protect = value & 0x40 != 0;
            }
            0xC000..=0xDFFE if addr & 1 == 0 => {
                self.irq_latch = value;
            }
            0xC001..=0xDFFF if addr & 1 == 1 => {
                self.irq_counter = 0;
                self.irq_reload = true;
            }
            0xE000..=0xFFFE if addr & 1 == 0 => {
                self.irq_enabled = false;
                self.irq_pending = false;
            }
            0xE001..=0xFFFF if addr & 1 == 1 => {
                self.irq_enabled = true;
            }
            _ => (),
        }
    }

    fn ppu_read(&self, addr: u16) -> Option<u8> {
        let offset = self.chr_offset(addr)?;

        match &self.chr {
            Mmc3Chr::Rom(bytes) | Mmc3Chr::Ram(bytes) => Some(bytes[offset]),
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let Some(offset) = self.chr_offset(addr) else {
            return;
        };

        if let Mmc3Chr::Ram(bytes) = &mut self.chr {
            bytes[offset] = value;
        }
    }

    fn observe_ppu_addr(&mut self, addr: u16, ppu_cycle: u64) {
        let a12 = addr & 0x1000 != 0;

        if !a12 {
            if self.last_ppu_a12 || self.a12_low_since.is_none() {
                self.a12_low_since = Some(ppu_cycle);
            }

            self.last_ppu_a12 = false;
            return;
        }

        if !self.last_ppu_a12 {
            let low_long_enough = self
                .a12_low_since
                .is_some_and(|since| ppu_cycle.wrapping_sub(since) >= 8);

            if low_long_enough {
                self.clock_irq_counter();
            }
        }

        self.a12_low_since = None;
        self.last_ppu_a12 = a12;
    }

    fn irq_asserted(&self) -> bool {
        self.irq_pending
    }
}
