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

pub struct Controller {
    buttons: ControllerButtons,
    shift: u8,
    strobe: bool,
}

impl Default for Controller {
    fn default() -> Self {
        Self::new()
    }
}

impl Controller {
    pub fn new() -> Self {
        Self {
            buttons: ControllerButtons::default(),
            shift: 0,
            strobe: false,
        }
    }

    pub fn set_buttons(&mut self, buttons: ControllerButtons) {
        self.buttons = buttons;

        if self.strobe {
            self.shift = self.buttons.0;
        }
    }

    pub fn write_strobe(&mut self, value: u8) {
        self.strobe = value & 1 != 0;

        if self.strobe {
            self.shift = self.buttons.0;
        }
    }

    pub fn read(&mut self) -> u8 {
        if self.strobe {
            return self.buttons.0 & 1;
        }

        let bit = self.shift & 1;
        self.shift = (self.shift >> 1) | 0x80;
        bit
    }
}
