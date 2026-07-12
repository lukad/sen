use bincode::{Decode, Encode};

use crate::apu::{LENGTH_TABLE, envelope::Envelope};

const DUTY_TABLE: [[u8; 8]; 4] = [
    [0, 1, 0, 0, 0, 0, 0, 0],
    [0, 1, 1, 0, 0, 0, 0, 0],
    [0, 1, 1, 1, 1, 0, 0, 0],
    [1, 0, 0, 1, 1, 1, 1, 1],
];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Encode, Decode)]
pub(crate) enum SweepNegateMode {
    #[default]
    OnesComplement,
    TwosComplement,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Encode, Decode)]
pub(crate) struct Pulse {
    enabled: bool,
    duty: u8,
    envelope: Envelope,
    timer_period: u16,
    timer_counter: u16,
    sequence_step: u8,
    length_counter: u8,
    sweep_negate_mode: SweepNegateMode,
    sweep_enabled: bool,
    sweep_period: u8,
    sweep_negate: bool,
    sweep_shift: u8,
    sweep_divider: u8,
    sweep_reload: bool,
}

impl Pulse {
    pub(crate) fn new(sweep_negate_mode: SweepNegateMode) -> Self {
        Self {
            sweep_negate_mode,
            ..Default::default()
        }
    }

    pub(crate) fn write_control(&mut self, value: u8) {
        self.duty = value >> 6;
        self.envelope.write_control(value);
    }

    pub(crate) fn write_sweep(&mut self, value: u8) {
        self.sweep_enabled = value & 0x80 != 0;
        self.sweep_period = (value >> 4) & 0x07;
        self.sweep_negate = value & 0x08 != 0;
        self.sweep_shift = value & 0x07;
        self.sweep_reload = true;
    }

    pub(crate) fn sweep_target_period(&self) -> u16 {
        let change = (self.timer_period >> self.sweep_shift) as i32;
        let current = self.timer_period as i32;

        let target = if self.sweep_negate {
            let extra = match self.sweep_negate_mode {
                SweepNegateMode::OnesComplement => 1,
                SweepNegateMode::TwosComplement => 0,
            };

            current - change - extra
        } else {
            current + change
        };

        target.clamp(0, i32::from(u16::MAX)) as u16
    }

    fn sweep_muting(&self) -> bool {
        self.timer_period < 8 || self.sweep_target_period() > 0x07FF
    }

    pub(crate) fn clock_sweep(&mut self) {
        if self.sweep_divider == 0
            && self.sweep_enabled
            && self.sweep_shift != 0
            && !self.sweep_muting()
        {
            self.timer_period = self.sweep_target_period();
        }

        if self.sweep_divider == 0 || self.sweep_reload {
            self.sweep_divider = self.sweep_period;
            self.sweep_reload = false;
        } else {
            self.sweep_divider -= 1;
        }
    }

    pub(crate) fn write_timer_low(&mut self, value: u8) {
        self.timer_period = (self.timer_period & 0x0700) | value as u16;
    }

    pub(crate) fn write_timer_high(&mut self, value: u8) {
        self.timer_period = (self.timer_period & 0x00FF) | (((value & 0x07) as u16) << 8);
        self.sequence_step = 0;
        self.envelope.restart();

        if self.enabled {
            self.length_counter = LENGTH_TABLE[(value >> 3) as usize];
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

    pub(crate) fn clock_length_counter(&mut self) {
        if !self.envelope.loop_flag() && self.length_counter > 0 {
            self.length_counter -= 1;
        }
    }

    pub(crate) fn tick_timer(&mut self) {
        if self.timer_counter == 0 {
            self.timer_counter = self.timer_period;
            self.sequence_step = (self.sequence_step + 1) & 0x07;
        } else {
            self.timer_counter -= 1;
        }
    }

    pub(crate) fn clock_envelope(&mut self) {
        self.envelope.clock();
    }

    pub(crate) fn output(&self) -> u8 {
        if !self.enabled || self.length_counter == 0 || self.timer_period < 8 {
            return 0;
        }

        if DUTY_TABLE[self.duty as usize][self.sequence_step as usize] == 0 {
            return 0;
        }

        self.envelope.output()
    }
}
