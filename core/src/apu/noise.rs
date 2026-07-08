use crate::apu::{LENGTH_TABLE, envelope::Envelope};

const NOISE_PERIOD_TABLE: [u16; 16] = [
    4, 8, 16, 32, 64, 96, 128, 160, 202, 254, 380, 508, 762, 1016, 2034, 4068,
];

#[derive(Default)]
pub(crate) struct Noise {
    enabled: bool,
    envelope: Envelope,
    mode: bool,
    timer_period: u16,
    timer_counter: u16,
    shift_register: u16,
    length_counter: u8,
}

impl Noise {
    pub(crate) fn new() -> Self {
        Self {
            shift_register: 1,
            ..Default::default()
        }
    }

    pub(crate) fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;

        if !enabled {
            self.length_counter = 0;
        }
    }

    pub(crate) fn length_counter_active(&self) -> bool {
        self.length_counter > 0
    }

    pub(crate) fn write_control(&mut self, value: u8) {
        self.envelope.write_control(value);
    }

    pub(crate) fn write_period(&mut self, value: u8) {
        self.mode = value & 0x80 != 0;
        self.timer_period = NOISE_PERIOD_TABLE[(value & 0x0F) as usize];
    }

    pub(crate) fn write_length(&mut self, value: u8) {
        if self.enabled {
            self.length_counter = LENGTH_TABLE[(value >> 3) as usize];
        }

        self.envelope.restart();
    }

    pub(crate) fn tick_timer(&mut self) {
        if self.timer_counter == 0 {
            self.timer_counter = self.timer_period.saturating_sub(1);
            self.clock_shift_register();
        } else {
            self.timer_counter -= 1;
        }
    }

    fn clock_shift_register(&mut self) {
        let tap = if self.mode { 6 } else { 1 };
        let feedback = (self.shift_register & 0x01) ^ ((self.shift_register >> tap) & 0x01);

        self.shift_register >>= 1;
        self.shift_register |= feedback << 14;
    }

    pub(crate) fn clock_envelope(&mut self) {
        self.envelope.clock();
    }

    pub(crate) fn clock_length_counter(&mut self) {
        if !self.envelope.loop_flag() && self.length_counter > 0 {
            self.length_counter -= 1;
        }
    }

    pub(crate) fn output(&self) -> u8 {
        if self.length_counter == 0 || self.shift_register & 0x01 != 0 {
            0
        } else {
            self.envelope.output()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shift_register_starts_at_one() {
        let noise = Noise::new();
        assert_eq!(noise.shift_register, 1);
    }

    #[test]
    fn mode_clear_uses_bit_1_for_feedback() {
        let mut noise = Noise::new();
        noise.shift_register = 0b0000_0000_0000_0011;
        noise.write_period(0x00);
        noise.clock_shift_register();

        assert_eq!(noise.shift_register & (1 << 14), 0);
    }

    #[test]
    fn mode_set_uses_bit_6_for_feedback() {
        let mut noise = Noise::new();
        noise.shift_register = 0b0000_0000_0100_0001;
        noise.write_period(0x80);
        noise.clock_shift_register();

        assert_eq!(noise.shift_register & (1 << 14), 0);
    }
}
