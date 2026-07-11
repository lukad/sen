#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Envelope {
    loop_flag: bool,
    constant_volume: bool,
    volume: u8,
    start: bool,
    divider: u8,
    decay: u8,
}

impl Envelope {
    pub(crate) fn write_control(&mut self, value: u8) {
        self.loop_flag = value & 0x20 != 0;
        self.constant_volume = value & 0x10 != 0;
        self.volume = value & 0x0F;
    }

    pub(crate) fn restart(&mut self) {
        self.start = true;
    }

    pub(crate) fn clock(&mut self) {
        if self.start {
            self.start = false;
            self.decay = 15;
            self.divider = self.volume;
        } else if self.divider == 0 {
            self.divider = self.volume;

            if self.decay > 0 {
                self.decay -= 1;
            } else if self.loop_flag {
                self.decay = 15;
            }
        } else {
            self.divider -= 1;
        }
    }

    pub(crate) fn output(&self) -> u8 {
        if self.constant_volume {
            self.volume
        } else {
            self.decay
        }
    }

    pub(crate) fn loop_flag(&self) -> bool {
        self.loop_flag
    }
}
