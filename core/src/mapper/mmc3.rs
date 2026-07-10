use crate::mapper::Mirroring;

pub(crate) struct Mmc3 {
    bank_select: u8,
    bank_registers: [u8; 8],
    mirroring: Mirroring,
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
    pub(crate) fn new(mirroring: Mirroring) -> Self {
        Self {
            bank_select: 0,
            bank_registers: [0; 8],
            mirroring,
            prg_ram_enabled: true,
            prg_ram_write_protect: false,
            irq_latch: 0,
            irq_counter: 0,
            irq_reload: false,
            irq_enabled: false,
            irq_pending: false,
            last_ppu_a12: false,
            a12_low_since: None,
        }
    }

    pub(crate) fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    pub(crate) fn prg_ram_enabled(&self) -> bool {
        self.prg_ram_enabled
    }

    pub(crate) fn prg_ram_writable(&self) -> bool {
        self.prg_ram_enabled && !self.prg_ram_write_protect
    }

    pub(crate) fn prg_rom_offset(&self, addr: u16, prg_len: usize) -> Option<usize> {
        if !(0x8000..=0xFFFF).contains(&addr) {
            return None;
        }

        let bank_count = prg_len / 0x2000;
        let second_last_bank = bank_count - 2;
        let last_bank = bank_count - 1;

        let r6 = (self.bank_registers[6] & 0x3F) as usize % bank_count;
        let r7 = (self.bank_registers[7] & 0x3F) as usize % bank_count;
        let prg_mode = self.bank_select & 0x40 != 0;

        let bank = match addr {
            0x8000..=0x9FFF if prg_mode => second_last_bank,
            0x8000..=0x9FFF => r6,
            0xA000..=0xBFFF => r7,
            0xC000..=0xDFFF if prg_mode => r6,
            0xC000..=0xDFFF => second_last_bank,
            0xE000..=0xFFFF => last_bank,
            _ => unreachable!(),
        };

        Some(bank * 0x2000 + (addr as usize & 0x1FFF))
    }

    /// Returns the raw MMC3 1 KiB CHR bank output and the offset within it.
    /// Board wiring decides which memory chip and physical bank that output selects.
    pub(crate) fn chr_bank(&self, addr: u16) -> Option<(u8, usize)> {
        if !matches!(addr, 0x0000..=0x1FFF) {
            return None;
        }

        let chr_inverted = self.bank_select & 0x80 != 0;
        let (register, bank_offset, offset_in_bank) = match (chr_inverted, addr) {
            (false, 0x0000..=0x07FF) => (0, (addr / 0x0400) as u8, addr as usize & 0x03FF),
            (false, 0x0800..=0x0FFF) => {
                (1, ((addr - 0x0800) / 0x0400) as u8, addr as usize & 0x03FF)
            }
            (false, 0x1000..=0x13FF) => (2, 0, addr as usize & 0x03FF),
            (false, 0x1400..=0x17FF) => (3, 0, addr as usize & 0x03FF),
            (false, 0x1800..=0x1BFF) => (4, 0, addr as usize & 0x03FF),
            (false, 0x1C00..=0x1FFF) => (5, 0, addr as usize & 0x03FF),
            (true, 0x0000..=0x03FF) => (2, 0, addr as usize & 0x03FF),
            (true, 0x0400..=0x07FF) => (3, 0, addr as usize & 0x03FF),
            (true, 0x0800..=0x0BFF) => (4, 0, addr as usize & 0x03FF),
            (true, 0x0C00..=0x0FFF) => (5, 0, addr as usize & 0x03FF),
            (true, 0x1000..=0x17FF) => {
                (0, ((addr - 0x1000) / 0x0400) as u8, addr as usize & 0x03FF)
            }
            (true, 0x1800..=0x1FFF) => {
                (1, ((addr - 0x1800) / 0x0400) as u8, addr as usize & 0x03FF)
            }
            _ => unreachable!(),
        };

        let value = self.bank_registers[register];
        let bank = if register < 2 {
            (value & !1).wrapping_add(bank_offset)
        } else {
            value
        };

        Some((bank, offset_in_bank))
    }

    pub(crate) fn write_register(&mut self, addr: u16, value: u8) {
        match addr {
            0x8000..=0x9FFE if addr & 1 == 0 => self.bank_select = value,
            0x8001..=0x9FFF if addr & 1 == 1 => {
                self.bank_registers[(self.bank_select & 0x07) as usize] = value;
            }
            0xA000..=0xBFFE if addr & 1 == 0 => {
                self.mirroring = if value & 1 == 0 {
                    Mirroring::Vertical
                } else {
                    Mirroring::Horizontal
                };
            }
            0xA001..=0xBFFF if addr & 1 == 1 => {
                self.prg_ram_enabled = value & 0x80 != 0;
                self.prg_ram_write_protect = value & 0x40 != 0;
            }
            0xC000..=0xDFFE if addr & 1 == 0 => self.irq_latch = value,
            0xC001..=0xDFFF if addr & 1 == 1 => {
                self.irq_counter = 0;
                self.irq_reload = true;
            }
            0xE000..=0xFFFE if addr & 1 == 0 => {
                self.irq_enabled = false;
                self.irq_pending = false;
            }
            0xE001..=0xFFFF if addr & 1 == 1 => self.irq_enabled = true,
            _ => (),
        }
    }

    pub(crate) fn observe_ppu_addr(&mut self, addr: u16, ppu_cycle: u64) {
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
        self.last_ppu_a12 = true;
    }

    pub(crate) fn irq_asserted(&self) -> bool {
        self.irq_pending
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
}
