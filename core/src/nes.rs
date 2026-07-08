use crate::{
    cartridge::Cartridge, controller::ControllerButtons, cpu::Cpu, frame::Frame, nes_bus::NesCpuBus,
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
        if self.bus.cpu_stalled() {
            return self.bus.tick_cpu_stall_cycle();
        }

        if self.cpu.can_start_interrupt() {
            self.service_interrupt();
        }

        self.cpu.tick(&mut self.bus);
        self.bus.tick_after_cpu_cycle()
    }

    fn service_interrupt(&mut self) {
        if self.bus.take_nmi() {
            self.cpu.start_nmi();
        } else if self.bus.irq_asserted() && !self.cpu.status.interrupt_disable {
            self.cpu.start_irq();
        }
    }

    pub fn run_until_frame(&mut self) {
        while !self.tick() {}
    }

    pub fn frame(&self) -> &Frame {
        self.bus.frame()
    }

    pub fn set_controller1(&mut self, buttons: ControllerButtons) {
        self.bus.set_controller1(buttons);
    }

    pub fn set_controller2(&mut self, buttons: ControllerButtons) {
        self.bus.set_controller2(buttons);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nrom_with_program(program: &[u8]) -> Cartridge {
        let mut prg_rom = vec![0; 0x4000];
        prg_rom[..program.len()].copy_from_slice(program);
        prg_rom[0x3FFC] = 0x00;
        prg_rom[0x3FFD] = 0x80;

        let mut rom = vec![0; 16];
        rom[0..4].copy_from_slice(b"NES\x1A");
        rom[4] = 1;
        rom[5] = 1;
        rom.extend_from_slice(&prg_rom);
        rom.extend_from_slice(&vec![0; 0x2000]);

        Cartridge::from_ines(&rom).unwrap()
    }

    #[test]
    fn oam_dma_stalls_cpu_before_next_instruction_executes() {
        let cartridge = nrom_with_program(&[
            0xA9, 0x02, // LDA #$02
            0x8D, 0x14, 0x40, // STA $4014
            0xE8, // INX
        ]);
        let mut nes = Nes::new(cartridge);

        while !nes.bus.cpu_stalled() {
            nes.tick();
        }

        let mut stalled_cycles = 0;
        while nes.bus.cpu_stalled() {
            assert_eq!(nes.cpu.x, 0);
            nes.tick();
            stalled_cycles += 1;
        }

        assert_eq!(stalled_cycles, 514);
        assert_eq!(nes.cpu.x, 0);

        nes.tick(); // Fetch INX after DMA finishes.
        assert_eq!(nes.cpu.x, 0);

        nes.tick(); // Execute INX.
        assert_eq!(nes.cpu.x, 1);
    }
}
