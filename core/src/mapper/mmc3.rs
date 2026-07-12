#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Mmc3State {
    bank_select: u8,
    bank_registers: [u8; 8],
    irq_latch: u8,
    irq_counter: u8,
    irq_reload: bool,
    irq_enabled: bool,
    irq_pending: bool,
    a12_filter: A12FilterState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum A12FilterState {
    NoLowInterval,
    LowSince(u64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Mmc3PrgRamControl {
    enabled: bool,
    write_protect: bool,
}

impl Mmc3PrgRamControl {
    pub(crate) fn new() -> Self {
        Self {
            enabled: true,
            write_protect: false,
        }
    }

    pub(crate) fn enabled(self) -> bool {
        self.enabled
    }

    pub(crate) fn writable(self) -> bool {
        self.enabled && !self.write_protect
    }

    pub(crate) fn write(&mut self, value: u8) {
        self.enabled = value & 0x80 != 0;
        self.write_protect = value & 0x40 != 0;
    }
}

impl Mmc3State {
    pub(crate) fn new() -> Self {
        Self {
            bank_select: 0,
            bank_registers: [0; 8],
            irq_latch: 0,
            irq_counter: 0,
            irq_reload: false,
            irq_enabled: false,
            irq_pending: false,
            a12_filter: A12FilterState::NoLowInterval,
        }
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
        let a12_high = addr & 0x1000 != 0;

        if !a12_high {
            if self.a12_filter == A12FilterState::NoLowInterval {
                self.a12_filter = A12FilterState::LowSince(ppu_cycle);
            }
            return;
        }

        if let A12FilterState::LowSince(since) = self.a12_filter
            && ppu_cycle.wrapping_sub(since) >= 8
        {
            self.clock_irq_counter();
        }

        self.a12_filter = A12FilterState::NoLowInterval;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cloneable_state_preserves_raw_banks_irq_and_a12_filter_progress() {
        let mut state = Mmc3State::new();

        state.write_register(0x8000, 0xC6);
        state.write_register(0x8001, 0xFF);
        state.write_register(0xC000, 2);
        state.write_register(0xC001, 0);
        state.write_register(0xE001, 0);
        state.observe_ppu_addr(0x0000, 10);

        let captured = state.clone();

        state.write_register(0x8001, 0x12);
        state.observe_ppu_addr(0x1000, 18);

        assert_eq!(captured.bank_select, 0xC6);
        assert_eq!(captured.bank_registers[6], 0xFF);
        assert_eq!(captured.irq_latch, 2);
        assert!(captured.irq_reload);
        assert!(captured.irq_enabled);
        assert_eq!(captured.a12_filter, A12FilterState::LowSince(10));

        assert_eq!(state.bank_registers[6], 0x12);
        assert_eq!(state.a12_filter, A12FilterState::NoLowInterval);
    }

    #[test]
    fn first_observed_high_level_does_not_clock_the_irq_counter() {
        let mut state = Mmc3State::new();
        state.write_register(0xC000, 0);
        state.write_register(0xC001, 0);
        state.write_register(0xE001, 0);

        state.observe_ppu_addr(0x1000, 100);

        assert!(!state.irq_asserted());
        assert_eq!(state.a12_filter, A12FilterState::NoLowInterval);
    }
}
