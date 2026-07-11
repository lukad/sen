use crate::{
    cartridge::Cartridge, controller::ControllerButtons, cpu::Cpu, frame::Frame,
    mapper::SaveRamError, nes_bus::NesCpuBus,
};

const PPU_TICKS_PER_CPU_CYCLE: u8 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SchedulerPhase {
    ReadyForCpuCycle,
    CompletingCpuCycle { ppu_ticks_remaining: u8 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SchedulerEvent {
    CpuCycleComplete,
    FrameBoundary,
}

/// Controller buttons held throughout one emulated frame interval.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InputFrame {
    controller1: ControllerButtons,
    controller2: ControllerButtons,
}

impl InputFrame {
    pub fn new(controller1: ControllerButtons, controller2: ControllerButtons) -> Self {
        Self {
            controller1,
            controller2,
        }
    }
}

pub struct Nes {
    cpu: Cpu,
    bus: NesCpuBus,
    phase: SchedulerPhase,
}

impl Nes {
    pub fn new(cartridge: Cartridge) -> Self {
        let mut cpu = Cpu::new();
        let mut bus = NesCpuBus::new(cartridge);
        cpu.reset(&mut bus);

        Self {
            cpu,
            bus,
            phase: SchedulerPhase::ReadyForCpuCycle,
        }
    }

    pub fn new_with_sample_rate(cartridge: Cartridge, sample_rate: f64) -> Self {
        let mut cpu = Cpu::new();
        let mut bus = NesCpuBus::new_with_sample_rate(cartridge, sample_rate);
        cpu.reset(&mut bus);

        Self {
            cpu,
            bus,
            phase: SchedulerPhase::ReadyForCpuCycle,
        }
    }

    pub fn tick(&mut self) -> bool {
        let mut frame_complete = false;

        loop {
            match self.advance_scheduler() {
                SchedulerEvent::FrameBoundary => frame_complete = true,
                SchedulerEvent::CpuCycleComplete => return frame_complete,
            }
        }
    }

    fn begin_cpu_cycle(&mut self) {
        let dma_owns_cycle =
            self.bus.dma_running() || self.bus.try_start_dma_halt(self.cpu.next_cycle_kind());

        if dma_owns_cycle {
            self.bus.perform_dma_bus_action();
        } else {
            if self.cpu.can_start_interrupt() {
                self.service_interrupt();
            }

            self.cpu.tick(&mut self.bus);
        }

        self.phase = SchedulerPhase::CompletingCpuCycle {
            ppu_ticks_remaining: PPU_TICKS_PER_CPU_CYCLE,
        }
    }

    fn advance_scheduler(&mut self) -> SchedulerEvent {
        loop {
            match self.phase {
                SchedulerPhase::ReadyForCpuCycle => {
                    self.begin_cpu_cycle();
                }
                SchedulerPhase::CompletingCpuCycle {
                    ppu_ticks_remaining: 0,
                } => {
                    self.bus.finish_cpu_cycle();
                    self.phase = SchedulerPhase::ReadyForCpuCycle;
                    return SchedulerEvent::CpuCycleComplete;
                }
                SchedulerPhase::CompletingCpuCycle {
                    ppu_ticks_remaining,
                } => {
                    self.phase = SchedulerPhase::CompletingCpuCycle {
                        ppu_ticks_remaining: ppu_ticks_remaining - 1,
                    };

                    if self.bus.tick_ppu() {
                        return SchedulerEvent::FrameBoundary;
                    }
                }
            }
        }
    }

    fn service_interrupt(&mut self) {
        if self.bus.take_nmi() {
            self.cpu.start_nmi();
        } else if self.bus.irq_asserted() && !self.cpu.status.interrupt_disable {
            self.cpu.start_irq();
        }
    }

    pub fn run_frame(&mut self, input: InputFrame) {
        self.bus.set_controller1(input.controller1);
        self.bus.set_controller2(input.controller2);
        while self.advance_scheduler() != SchedulerEvent::FrameBoundary {}
    }

    pub fn frame(&self) -> &Frame {
        self.bus.frame()
    }

    pub fn pop_audio_sample(&mut self) -> Option<f32> {
        self.bus.pop_audio_sample()
    }

    pub fn save_ram(&self) -> Option<&[u8]> {
        self.bus.save_ram()
    }

    pub fn save_ram_mut(&mut self) -> Option<&mut [u8]> {
        self.bus.save_ram_mut()
    }

    pub fn load_save_ram(&mut self, data: &[u8]) -> Result<(), SaveRamError> {
        self.bus.load_save_ram(data)
    }
}

#[cfg(test)]
mod tests {
    use crate::bus::Bus;

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
        let mut stalled_cycles = 0;

        while !nes.bus.dma_running() {
            nes.tick();

            if nes.bus.dma_running() {
                stalled_cycles += 1;
            }
        }

        while nes.bus.dma_running() {
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

    #[test]
    fn scheduler_stops_at_frame_boundary_and_resumes_the_pending_tail() {
        let cartridge = nrom_with_program(&[
            0x4C, 0x00, 0x80, // JMP $8000
        ]);
        let mut nes = Nes::new(cartridge);

        loop {
            if nes.advance_scheduler() == SchedulerEvent::FrameBoundary {
                break;
            }
        }

        assert_eq!(
            nes.phase,
            SchedulerPhase::CompletingCpuCycle {
                ppu_ticks_remaining: 1,
            },
        );

        assert_eq!(nes.advance_scheduler(), SchedulerEvent::CpuCycleComplete,);
        assert_eq!(nes.phase, SchedulerPhase::ReadyForCpuCycle,);
    }

    #[test]
    fn run_until_frame_stops_before_rendering_the_next_frame() {
        const CYCLES_BEFORE_BOUNDARY_GROUP: usize = 29_780;

        let cartridge = nrom_with_program(&[
            0x4C, 0x00, 0x80, // JMP $8000
        ]);
        let mut nes = Nes::new(cartridge);

        // The first exact boundary occurs on the second PPU tick, leaving one.
        nes.run_frame(InputFrame::default());
        assert_eq!(
            nes.phase,
            SchedulerPhase::CompletingCpuCycle {
                ppu_ticks_remaining: 1,
            },
        );

        // Finish that pending tail, leaving the PPU at scanline 0, dot 1.
        assert_eq!(nes.advance_scheduler(), SchedulerEvent::CpuCycleComplete,);

        // Reach the final CPU-cycle group of the next frame. Its boundary will
        // occur on the first PPU tick, leaving two ticks pending.
        for _ in 0..CYCLES_BEFORE_BOUNDARY_GROUP {
            assert!(!nes.tick());
        }

        let completed_pixel: [u8; 3] = nes.frame().pixels()[0..3].try_into().unwrap();

        // Change the background color after visible rendering has finished.
        nes.bus.write(0x2006, 0x3F);
        nes.bus.write(0x2006, 0x00);
        nes.bus.write(0x2007, 0x30);

        assert_eq!(&nes.frame().pixels()[0..3], &completed_pixel);

        nes.run_frame(InputFrame::default());

        // The scheduler stopped immediately at the boundary.
        assert_eq!(
            nes.phase,
            SchedulerPhase::CompletingCpuCycle {
                ppu_ticks_remaining: 2,
            },
        );

        // Dot 1 of the next frame has not overwritten pixel (0, 0).
        assert_eq!(&nes.frame().pixels()[0..3], &completed_pixel);
    }

    #[test]
    fn run_frame_applies_input_before_frame_execution() {
        let cartridge = nrom_with_program(&[
            0xA9, 0x01, // LDA #$01
            0x8D, 0x16, 0x40, // STA $4016: strobe on
            0xA9, 0x00, // LDA #$00
            0x8D, 0x16, 0x40, // STA $4016: strobe off
            0xAD, 0x16, 0x40, // LDA $4016
            0x29, 0x01, // AND #$01
            0x85, 0x00, // STA $00
            0x4C, 0x00, 0x80, // JMP $8000
        ]);
        let mut nes = Nes::new(cartridge);

        nes.run_frame(InputFrame::new(
            ControllerButtons::default().with_a(true),
            ControllerButtons::default(),
        ));
        assert_eq!(nes.bus.read(0x0000), 1);

        nes.run_frame(InputFrame::default());
        assert_eq!(nes.bus.read(0x0000), 0);
    }
}
