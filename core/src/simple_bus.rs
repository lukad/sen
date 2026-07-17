use crate::bus::Bus;

pub(crate) struct SimpleBus {
    pub(crate) mem: [u8; 0x10000], // 64 KiB
}

impl SimpleBus {
    pub(crate) fn new() -> Self {
        Self { mem: [0; 0x10000] }
    }

    /// Load a program (or data) into memory at `start`.
    pub(crate) fn load(&mut self, start: u16, data: &[u8]) {
        let mut addr = start as usize;
        for &byte in data {
            self.mem[addr] = byte;
            addr = (addr + 1) & 0xFFFF;
        }
    }

    /// Convenience: read without &mut, for tests / inspection.
    pub(crate) fn peek(&self, addr: u16) -> u8 {
        self.mem[addr as usize]
    }

    /// Convenience: write without &mut Bus (e.g. set reset vectors).
    pub(crate) fn poke(&mut self, addr: u16, value: u8) {
        self.mem[addr as usize] = value;
    }
}

impl Default for SimpleBus {
    fn default() -> Self {
        Self::new()
    }
}

impl Bus for SimpleBus {
    #[inline]
    fn read(&mut self, addr: u16) -> u8 {
        #[cfg(feature = "tracing")]
        tracing::trace!("Read from {:#06X}", addr);
        self.mem[addr as usize]
    }

    #[inline]
    fn write(&mut self, addr: u16, value: u8) {
        #[cfg(feature = "tracing")]
        tracing::trace!("Write to {:#06X} = {:#02X}", addr, value);
        self.mem[addr as usize] = value;
    }
}
