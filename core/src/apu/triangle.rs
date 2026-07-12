use bincode::{Decode, Encode};

use crate::apu::LENGTH_TABLE;

const TRIANGLE_SEQUENCE: [u8; 32] = [
    15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12,
    13, 14, 15,
];

#[derive(Debug, Clone, Default, PartialEq, Eq, Encode, Decode)]
pub(crate) struct Triangle {
    enabled: bool,
    control_flag: bool,
    linear_reload_value: u8,
    linear_counter: u8,
    linear_reload_flag: bool,
    length_counter: u8,
    timer_period: u16,
    timer_counter: u16,
    sequence_step: u8,
}

impl Triangle {
    pub(crate) fn new() -> Self {
        Self {
            sequence_step: 16,
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

    pub(crate) fn write_linear_counter(&mut self, value: u8) {
        self.control_flag = value & 0x80 != 0;
        self.linear_reload_value = value & 0x7F;
    }

    pub(crate) fn write_timer_low(&mut self, value: u8) {
        self.timer_period = (self.timer_period & 0x0700) | value as u16
    }

    pub(crate) fn write_timer_high_and_length(&mut self, value: u8) {
        self.timer_period = (self.timer_period & 0x00FF) | (((value & 0x07) as u16) << 8);

        if self.enabled {
            self.length_counter = LENGTH_TABLE[(value >> 3) as usize];
        }

        self.linear_reload_flag = true;
    }

    pub(crate) fn tick_timer(&mut self) {
        if self.timer_counter == 0 {
            self.timer_counter = self.timer_period;

            if self.length_counter > 0 && self.linear_counter > 0 && self.timer_period >= 2 {
                self.sequence_step = (self.sequence_step + 1) & 0x1F;
            }
        } else {
            self.timer_counter -= 1;
        }
    }

    pub(crate) fn clock_linear_counter(&mut self) {
        if self.linear_reload_flag {
            self.linear_counter = self.linear_reload_value;
        } else if self.linear_counter > 0 {
            self.linear_counter -= 1;
        }

        if !self.control_flag {
            self.linear_reload_flag = false;
        }
    }

    pub(crate) fn clock_length_counter(&mut self) {
        if !self.control_flag && self.length_counter > 0 {
            self.length_counter -= 1;
        }
    }

    pub(crate) fn output(&self) -> f32 {
        TRIANGLE_SEQUENCE[self.sequence_step as usize] as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enable_running_triangle(timer_period: u16) -> Triangle {
        let mut triangle = Triangle::new();
        triangle.set_enabled(true);
        triangle.write_linear_counter(0x81);
        triangle.write_timer_low((timer_period & 0x00FF) as u8);
        triangle.write_timer_high_and_length(((timer_period >> 8) & 0x07) as u8);
        triangle.clock_linear_counter();
        triangle
    }

    #[test]
    fn output_holds_current_level_when_channel_is_not_running() {
        let mut triangle = Triangle::new();
        triangle.sequence_step = 3;

        assert_eq!(triangle.output(), 12.0);
    }

    #[test]
    fn ultrasonic_timer_holds_current_level() {
        for timer_period in [0, 1] {
            let mut triangle = enable_running_triangle(timer_period);
            triangle.sequence_step = 5;

            for _ in 0..64 {
                triangle.tick_timer();
                assert_eq!(triangle.output(), 10.0);
            }
        }
    }

    #[test]
    fn audible_timer_outputs_current_sequence_value() {
        let mut triangle = enable_running_triangle(2);
        triangle.tick_timer();

        assert_eq!(triangle.output(), 1.0);
    }
}
