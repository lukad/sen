use bincode::{Decode, Encode};

const DMC_RATE_TABLE: [u16; 16] = [
    428, 380, 340, 320, 286, 254, 226, 214, 190, 160, 142, 128, 106, 84, 72, 54,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Encode, Decode)]
pub(crate) enum DmcDmaKind {
    Load,
    Reload,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Encode, Decode)]
pub(crate) struct DmcDmaRequest {
    pub(crate) addr: u16,
    pub(crate) kind: DmcDmaKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub(crate) struct Dmc {
    irq_enabled: bool,
    loop_flag: bool,
    interrupt_flag: bool,
    timer_period: u16,
    timer_counter: u16,
    output_level: u8,
    shift_register: u8,
    bits_remaining: u8,
    silence: bool,
    sample_address: u16,
    sample_length: u16,
    current_address: u16,
    bytes_remaining: u16,
    sample_buffer: Option<u8>,
    dma_request: Option<DmcDmaRequest>,
}

impl Dmc {
    pub(crate) fn new() -> Self {
        Self {
            irq_enabled: false,
            loop_flag: false,
            interrupt_flag: false,
            timer_period: DMC_RATE_TABLE[0],
            timer_counter: DMC_RATE_TABLE[0] - 1,
            output_level: 0,
            shift_register: 0,
            bits_remaining: 8,
            silence: true,
            sample_address: 0xC000,
            sample_length: 1,
            current_address: 0xC000,
            bytes_remaining: 0,
            sample_buffer: None,
            dma_request: None,
        }
    }

    pub(crate) fn write_flags_rate(&mut self, value: u8) {
        self.irq_enabled = value & 0x80 != 0;
        self.loop_flag = value & 0x40 != 0;
        self.timer_period = DMC_RATE_TABLE[(value & 0x0F) as usize];

        if !self.irq_enabled {
            self.interrupt_flag = false;
        }
    }

    pub(crate) fn write_direct_load(&mut self, value: u8) {
        self.output_level = value & 0x7F;
    }

    pub(crate) fn write_sample_address(&mut self, value: u8) {
        self.sample_address = 0xC000 | ((value as u16) << 6);
    }

    pub(crate) fn write_sample_length(&mut self, value: u8) {
        self.sample_length = ((value as u16) << 4) | 1;
    }

    pub(crate) fn set_enabled(&mut self, enabled: bool) {
        self.interrupt_flag = false;

        if !enabled {
            self.bytes_remaining = 0;
            return;
        }

        if self.bytes_remaining == 0 {
            self.restart_sample();
            self.request_dma(DmcDmaKind::Load);
        }
    }

    fn restart_sample(&mut self) {
        self.current_address = self.sample_address;
        self.bytes_remaining = self.sample_length;
    }

    pub(crate) fn active(&self) -> bool {
        self.bytes_remaining > 0
    }

    pub(crate) fn interrupt_flag(&self) -> bool {
        self.interrupt_flag
    }

    fn request_dma(&mut self, kind: DmcDmaKind) {
        if self.sample_buffer.is_none() && self.bytes_remaining > 0 && self.dma_request.is_none() {
            self.dma_request = Some(DmcDmaRequest {
                addr: self.current_address,
                kind,
            });
        }
    }

    pub(crate) fn take_dma_request(&mut self) -> Option<DmcDmaRequest> {
        self.dma_request.take()
    }

    pub(crate) fn load_sample_buffer(&mut self, value: u8) {
        self.sample_buffer = Some(value);

        self.current_address = if self.current_address == 0xFFFF {
            0x8000
        } else {
            self.current_address + 1
        };

        self.bytes_remaining -= 1;

        if self.bytes_remaining == 0 {
            if self.loop_flag {
                self.restart_sample();
                self.request_dma(DmcDmaKind::Reload);
            } else if self.irq_enabled {
                self.interrupt_flag = true;
            }
        }
    }

    pub(crate) fn tick_timer(&mut self) {
        if self.timer_counter == 0 {
            self.timer_counter = self.timer_period - 1;
            self.clock_output_unit();
        } else {
            self.timer_counter -= 1;
        }
    }

    fn clock_output_unit(&mut self) {
        if !self.silence {
            if self.shift_register & 0x01 != 0 {
                if self.output_level <= 125 {
                    self.output_level += 2;
                }
            } else if self.output_level >= 2 {
                self.output_level -= 2;
            }
        }

        self.shift_register >>= 1;
        self.bits_remaining -= 1;

        if self.bits_remaining == 0 {
            self.bits_remaining = 8;

            if let Some(value) = self.sample_buffer.take() {
                self.silence = false;
                self.shift_register = value;
                self.request_dma(DmcDmaKind::Reload);
            } else {
                self.silence = true;
            }
        }
    }

    pub(crate) fn output(&self) -> u8 {
        self.output_level
    }
}
