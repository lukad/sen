#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ControllerButtons(u8);

impl ControllerButtons {
    const A: u8 = 1 << 0;
    const B: u8 = 1 << 1;
    const SELECT: u8 = 1 << 2;
    const START: u8 = 1 << 3;
    const UP: u8 = 1 << 4;
    const DOWN: u8 = 1 << 5;
    const LEFT: u8 = 1 << 6;
    const RIGHT: u8 = 1 << 7;

    pub fn from_bits(bits: u8) -> Self {
        Self(bits)
    }

    fn with_button(mut self, mask: u8, pressed: bool) -> Self {
        if pressed {
            self.0 |= mask;
        } else {
            self.0 &= !mask;
        }

        self
    }

    pub fn with_a(self, pressed: bool) -> Self {
        self.with_button(Self::A, pressed)
    }

    pub fn with_b(self, pressed: bool) -> Self {
        self.with_button(Self::B, pressed)
    }

    pub fn with_select(self, pressed: bool) -> Self {
        self.with_button(Self::SELECT, pressed)
    }

    pub fn with_start(self, pressed: bool) -> Self {
        self.with_button(Self::START, pressed)
    }

    pub fn with_up(self, pressed: bool) -> Self {
        self.with_button(Self::UP, pressed)
    }

    pub fn with_down(self, pressed: bool) -> Self {
        self.with_button(Self::DOWN, pressed)
    }

    pub fn with_left(self, pressed: bool) -> Self {
        self.with_button(Self::LEFT, pressed)
    }

    pub fn with_right(self, pressed: bool) -> Self {
        self.with_button(Self::RIGHT, pressed)
    }
}

impl From<u8> for ControllerButtons {
    fn from(value: u8) -> Self {
        Self::from_bits(value)
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ControllerPort {
    shift: u8,
    strobe: bool,
}

impl ControllerPort {
    pub(crate) fn write_strobe(&mut self, value: u8, buttons: ControllerButtons) {
        let new_strobe = value & 1 != 0;

        if self.strobe || new_strobe {
            self.shift = buttons.0;
        }

        self.strobe = new_strobe;
    }

    pub(crate) fn read(&mut self, buttons: ControllerButtons) -> u8 {
        if self.strobe {
            return buttons.0 & 1;
        }

        let bit = self.shift & 1;
        self.shift = (self.shift >> 1) | 0x80;
        bit
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controller_port_separates_live_buttons_from_latched_state() {
        let released = ControllerButtons::default();
        let a_pressed = ControllerButtons::from_bits(0x01);
        let mut port = ControllerPort::default();

        port.write_strobe(1, released);

        // High strobe reads the current host observation directly
        assert_eq!(port.read(a_pressed), 1);

        // Falling edge captures the latest observation
        port.write_strobe(0, a_pressed);

        // Once low, reads use the captured shift register rather than live input
        assert_eq!(port.read(released), 1);
        assert_eq!(port.read(a_pressed), 0);
    }
}
