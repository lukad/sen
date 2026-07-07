use crate::{
    cartridge::{self, Cartridge},
    cpu::Cpu,
    frame::Frame,
    nes_bus::NesCpuBus,
};

pub struct Nes {
    cpu: Cpu,
    bus: NesCpuBus,
}

impl Nes {
    pub fn new(cartridge: Cartridge) -> Self {
        let mut cpu = Cpu::new();
        let mut bus = NesCpuBus::new(cartridge);
        cpu.reset(&mut bus);
        Self { cpu, bus }
    }

    pub fn tick(&mut self) -> bool {
        let instruction_done = self.cpu.tick(&mut self.bus);
        let frame_complete = self.bus.tick_after_cpu_cycle();

        if instruction_done {
            if self.bus.take_nmi() {
                self.cpu.start_nmi();
            } else if self.bus.irq_asserted() && !self.cpu.status.interrupt_disable {
                self.cpu.start_irq();
            }
        }

        frame_complete
    }

    pub fn run_until_frame(&mut self) {
        while !self.tick() {}
    }

    pub fn frame(&self) -> &Frame {
        self.bus.frame()
    }
}
