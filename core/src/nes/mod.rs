mod state_image;

use std::collections::VecDeque;

use bincode::{Decode, Encode};

use crate::{
    cartridge::{Cartridge, CartridgeId},
    cheat::GameGenieCode,
    controller::ControllerButtons,
    cpu::Cpu,
    frame::Frame,
    nes_bus::{NesCpuBus, NesCpuBusState},
};

pub use crate::mapper::SaveRamError;
pub use state_image::StateImageError;

const PPU_TICKS_PER_CPU_CYCLE: u8 = 3;
const MAX_BUFFERED_SAMPLES: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum SchedulerPhase {
    ReadyForCpuCycle,
    CompletingCpuCycle { ppu_ticks_remaining: u8 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MachineCompatibility {
    cartridge: CartridgeId,
    sample_rate_bits: u64,
}

#[derive(Clone, PartialEq, Encode, Decode)]
struct MachineState {
    cpu: Cpu,
    bus: NesCpuBusState,
    phase: SchedulerPhase,
}

#[derive(Clone)]
pub struct FrameCheckpoint {
    compatibility: MachineCompatibility,
    state: MachineState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum FrameCheckpointError {
    #[error("not at a frame boundary")]
    NotAtFrameBoundary,
    #[error("checkpoint belongs to another machine")]
    IncompatibleMachine,
}

pub struct RamRegionsMut<'a> {
    pub system: &'a mut [u8],
    pub prg: Option<&'a mut [u8]>,
    pub prg_is_battery_backed: bool,
}

pub struct Nes {
    cpu: Cpu,
    bus: NesCpuBus,
    phase: SchedulerPhase,
    frame: Frame,
    audio_samples: VecDeque<f32>,
    compatibility: MachineCompatibility,
    at_frame_boundary: bool,
}

impl Nes {
    pub fn new(cartridge: Cartridge) -> Self {
        Self::new_with_sample_rate(cartridge, 44_100.0)
    }

    pub fn new_with_sample_rate(cartridge: Cartridge, sample_rate: f64) -> Self {
        let compatibility = MachineCompatibility {
            cartridge: cartridge.id(),
            sample_rate_bits: sample_rate.to_bits(),
        };

        let mut cpu = Cpu::new();
        let mut bus = NesCpuBus::new_with_sample_rate(cartridge, sample_rate);
        cpu.reset(&mut bus);

        Self {
            cpu,
            bus,
            phase: SchedulerPhase::ReadyForCpuCycle,
            frame: Frame::new(),
            audio_samples: VecDeque::with_capacity(MAX_BUFFERED_SAMPLES),
            compatibility,
            at_frame_boundary: false,
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
        self.at_frame_boundary = false;

        loop {
            match self.phase {
                SchedulerPhase::ReadyForCpuCycle => {
                    self.begin_cpu_cycle();
                }
                SchedulerPhase::CompletingCpuCycle {
                    ppu_ticks_remaining: 0,
                } => {
                    let audio_samples = &mut self.audio_samples;

                    self.bus.finish_cpu_cycle(|sample| {
                        if audio_samples.len() == MAX_BUFFERED_SAMPLES {
                            audio_samples.pop_front();
                        }

                        audio_samples.push_back(sample);
                    });

                    self.phase = SchedulerPhase::ReadyForCpuCycle;
                    return SchedulerEvent::CpuCycleComplete;
                }
                SchedulerPhase::CompletingCpuCycle {
                    ppu_ticks_remaining,
                } => {
                    self.phase = SchedulerPhase::CompletingCpuCycle {
                        ppu_ticks_remaining: ppu_ticks_remaining - 1,
                    };

                    let output = self.bus.tick_ppu();

                    if let Some(pixel) = output.pixel {
                        self.frame
                            .set_pixel(pixel.x.into(), pixel.y.into(), pixel.rgb);
                    }

                    if output.frame_complete {
                        self.at_frame_boundary = true;
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
        &self.frame
    }

    pub fn pop_audio_sample(&mut self) -> Option<f32> {
        self.audio_samples.pop_front()
    }

    pub fn system_ram(&self) -> &[u8] {
        self.bus.system_ram()
    }

    pub fn system_ram_mut(&mut self) -> &mut [u8] {
        self.bus.system_ram_mut()
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

    pub fn ram_regions_mut(&mut self) -> RamRegionsMut<'_> {
        let (system, prg, prg_is_battery_backed) = self.bus.ram_regions_mut();

        RamRegionsMut {
            system,
            prg,
            prg_is_battery_backed,
        }
    }

    pub fn set_game_genie_codes(&mut self, codes: Vec<GameGenieCode>) {
        self.bus.set_game_genie_codes(codes);
    }

    pub fn capture_frame_checkpoint(&self) -> Result<FrameCheckpoint, FrameCheckpointError> {
        if !self.at_frame_boundary {
            return Err(FrameCheckpointError::NotAtFrameBoundary);
        }

        Ok(FrameCheckpoint {
            compatibility: self.compatibility,
            state: MachineState {
                cpu: self.cpu.clone(),
                bus: self.bus.state(),
                phase: self.phase,
            },
        })
    }

    pub fn restore_frame_checkpoint(
        &mut self,
        checkpoint: &FrameCheckpoint,
    ) -> Result<(), FrameCheckpointError> {
        if self.compatibility != checkpoint.compatibility {
            return Err(FrameCheckpointError::IncompatibleMachine);
        }

        let MachineState { cpu, bus, phase } = checkpoint.state.clone();

        self.cpu = cpu;
        self.bus.restore_state(bus);
        self.phase = phase;

        self.frame = Frame::new();
        self.audio_samples.clear();
        self.at_frame_boundary = true;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::{bus::Bus, nes::state_image::StateImageError};

    use super::*;

    fn drain_audio(nes: &mut Nes) -> Vec<f32> {
        std::iter::from_fn(|| nes.pop_audio_sample()).collect()
    }

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

    fn battery_backed_txrom_with_chr_ram() -> Cartridge {
        let mut prg_rom = vec![0; 0x8000];
        prg_rom[..3].copy_from_slice(&[0x4C, 0x00, 0x80]);
        prg_rom[0x7FFC] = 0x00;
        prg_rom[0x7FFD] = 0x80;

        let mut rom = vec![0; 16];
        rom[0..4].copy_from_slice(b"NES\x1A");
        rom[4] = 2;
        rom[5] = 0;
        rom[6] = 0x42; // Mapper 4 with battery-backed RAM.
        rom.extend_from_slice(&prg_rom);

        Cartridge::from_ines(&rom).unwrap()
    }

    fn non_battery_mmc1_with_chr_rom() -> Cartridge {
        let mut prg_rom = vec![0; 0x8000];
        prg_rom[0x7FFC] = 0x00;
        prg_rom[0x7FFD] = 0x80;

        let mut rom = vec![0; 16];
        rom[0..4].copy_from_slice(b"NES\x1A");
        rom[4] = 2;
        rom[5] = 1;
        rom[6] = 0x10; // Mapper 1 without battery-backed RAM.
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

    #[test]
    fn run_frame_collects_emitted_audio_in_the_presentation_queue() {
        let cartridge = nrom_with_program(&[
            0x4C, 0x00, 0x80, // JMP $8000
        ]);
        let mut nes = Nes::new(cartridge);

        assert!(nes.pop_audio_sample().is_none());

        nes.run_frame(InputFrame::default());

        let mut sample_count = 0;
        while nes.pop_audio_sample().is_some() {
            sample_count += 1;
        }

        assert!(sample_count > 0);
        assert!(nes.pop_audio_sample().is_none());
    }

    #[test]
    fn frame_checkpoint_is_only_available_at_the_exact_boundary() {
        let cartridge = nrom_with_program(&[0x4C, 0x00, 0x80]);
        let mut nes = Nes::new(cartridge);

        assert!(matches!(
            nes.capture_frame_checkpoint(),
            Err(FrameCheckpointError::NotAtFrameBoundary)
        ));

        nes.run_frame(InputFrame::default());
        assert!(nes.capture_frame_checkpoint().is_ok());

        nes.tick();

        assert!(matches!(
            nes.capture_frame_checkpoint(),
            Err(FrameCheckpointError::NotAtFrameBoundary)
        ));
    }

    #[test]
    fn restored_frame_checkpoint_replays_identically() {
        let cartridge = nrom_with_program(&[
            0xA9, 0x01, 0x8D, 0x16, 0x40, 0xA9, 0x00, 0x8D, 0x16, 0x40, 0xAD, 0x16, 0x40, 0x29,
            0x01, 0x85, 0x00, 0x4C, 0x00, 0x80,
        ]);
        let mut nes = Nes::new(cartridge);

        nes.run_frame(InputFrame::default());
        let checkpoint = nes.capture_frame_checkpoint().unwrap();

        drain_audio(&mut nes);

        let input = InputFrame::new(
            ControllerButtons::default().with_a(true),
            ControllerButtons::default(),
        );

        nes.run_frame(input);

        let expected_state = nes.capture_frame_checkpoint().unwrap();
        let expected_frame = nes.frame().pixels().to_vec();
        let expected_audio = drain_audio(&mut nes);

        nes.restore_frame_checkpoint(&checkpoint).unwrap();

        assert!(nes.frame().pixels().iter().all(|&byte| byte == 0));
        assert!(nes.pop_audio_sample().is_none());

        nes.run_frame(input);

        let actual_state = nes.capture_frame_checkpoint().unwrap();
        let actual_frame = nes.frame().pixels().to_vec();
        let actual_audio = drain_audio(&mut nes);

        assert!(expected_state.state == actual_state.state);
        assert_eq!(expected_frame, actual_frame);
        assert_eq!(expected_audio, actual_audio);
    }

    #[test]
    fn checkpoint_can_restore_into_equivalent_machine() {
        let program = &[0x4C, 0x00, 0x80];

        let mut source = Nes::new(nrom_with_program(program));
        let mut target = Nes::new(nrom_with_program(program));

        source.run_frame(InputFrame::default());
        let checkpoint = source.capture_frame_checkpoint().unwrap();

        target.restore_frame_checkpoint(&checkpoint).unwrap();

        let restored = target.capture_frame_checkpoint().unwrap();
        assert!(restored.state == checkpoint.state);
    }

    #[test]
    fn checkpoint_rejects_different_cartridge_without_mutation() {
        let mut source = Nes::new(nrom_with_program(&[0x4C, 0x00, 0x80]));
        let mut target = Nes::new(nrom_with_program(&[0xEA, 0x4C, 0x00, 0x80]));

        source.run_frame(InputFrame::default());
        target.run_frame(InputFrame::default());

        let checkpoint = source.capture_frame_checkpoint().unwrap();
        let before = target.capture_frame_checkpoint().unwrap();

        assert_eq!(
            target.restore_frame_checkpoint(&checkpoint),
            Err(FrameCheckpointError::IncompatibleMachine)
        );

        let after = target.capture_frame_checkpoint().unwrap();
        assert!(before.state == after.state);
    }

    #[test]
    fn checkpoint_rejects_different_audio_profile() {
        let program = &[0x4C, 0x00, 0x80];

        let mut source = Nes::new_with_sample_rate(nrom_with_program(program), 44_100.0);
        let mut target = Nes::new_with_sample_rate(nrom_with_program(program), 48_000.0);

        source.run_frame(InputFrame::default());
        let checkpoint = source.capture_frame_checkpoint().unwrap();

        assert_eq!(
            target.restore_frame_checkpoint(&checkpoint),
            Err(FrameCheckpointError::IncompatibleMachine)
        );
    }

    #[test]
    fn serialized_state_round_trip_restores_causal_state() {
        let mut nes = Nes::new(nrom_with_program(&[0x4C, 0x00, 0x80]));

        nes.run_frame(InputFrame::default());

        let expected = nes.capture_frame_checkpoint().unwrap();
        let mut image = vec![0; nes.serialized_state_size()];

        nes.serialize_state(&mut image).unwrap();

        nes.run_frame(InputFrame::new(
            ControllerButtons::default().with_a(true),
            ControllerButtons::default(),
        ));

        nes.unserialize_state(&image).unwrap();

        let actual = nes.capture_frame_checkpoint().unwrap();

        assert!(actual.state == expected.state);
        assert!(nes.frame().pixels().iter().all(|&byte| byte == 0));
        assert!(nes.pop_audio_sample().is_none());
    }

    #[test]
    fn serialized_state_image_is_deterministic() {
        let mut nes = Nes::new(nrom_with_program(&[0x4C, 0x00, 0x80]));
        nes.run_frame(InputFrame::default());

        let mut first = vec![0; nes.serialized_state_size()];
        let mut second = vec![0; nes.serialized_state_size()];

        nes.serialize_state(&mut first).unwrap();
        nes.serialize_state(&mut second).unwrap();

        assert_eq!(first, second);
    }

    #[test]
    fn corrupted_serialized_state_is_rejected_without_mutation() {
        let mut nes = Nes::new(nrom_with_program(&[0x4C, 0x00, 0x80]));
        nes.run_frame(InputFrame::default());

        let before = nes.capture_frame_checkpoint().unwrap();
        let mut image = vec![0; nes.serialized_state_size()];
        nes.serialize_state(&mut image).unwrap();

        let last = image.len() - 1;
        image[last] ^= 1;

        assert_eq!(
            nes.unserialize_state(&image),
            Err(StateImageError::ChecksumMismatch)
        );

        let after = nes.capture_frame_checkpoint().unwrap();
        assert!(before.state == after.state);
    }

    #[test]
    fn serialized_state_restore_preserves_save_ram_address() {
        let mut nes = Nes::new(battery_backed_txrom_with_chr_ram());
        nes.run_frame(InputFrame::default());

        nes.save_ram_mut().unwrap()[0] = 0xA5;
        let original_address = nes.save_ram_mut().unwrap().as_mut_ptr();

        let mut image = vec![0; nes.serialized_state_size()];
        nes.serialize_state(&mut image).unwrap();

        nes.save_ram_mut().unwrap()[0] = 0x5A;
        nes.unserialize_state(&image).unwrap();

        let save_ram = nes.save_ram_mut().unwrap();

        assert_eq!(save_ram.as_mut_ptr(), original_address);
        assert_eq!(save_ram[0], 0xA5);
    }

    #[test]
    fn ram_regions_expose_system_and_battery_backed_prg_ram() {
        let mut nes = Nes::new(battery_backed_txrom_with_chr_ram());

        {
            let RamRegionsMut {
                system,
                prg,
                prg_is_battery_backed,
            } = nes.ram_regions_mut();

            assert_eq!(system.len(), 0x0800);
            assert!(prg_is_battery_backed);

            system[0x0123] = 0xA5;
            let prg = prg.expect("battery-backed TxROM has PRG RAM");
            assert_eq!(prg.len(), 0x2000);
            prg[0x0456] = 0x5A;
        }

        assert_eq!(nes.system_ram()[0x0123], 0xA5);
        assert_eq!(nes.save_ram().unwrap()[0x0456], 0x5A);
    }

    #[test]
    fn ram_regions_expose_nonpersistent_prg_work_ram() {
        let mut nes = Nes::new(non_battery_mmc1_with_chr_rom());

        {
            let regions = nes.ram_regions_mut();

            assert!(!regions.prg_is_battery_backed);
            let prg = regions.prg.expect("MMC1 has PRG work RAM");
            assert_eq!(prg.len(), 0x2000);
            prg[0x0123] = 0xA5;
        }

        assert!(nes.save_ram().is_none());
        assert_eq!(nes.bus.read(0x6123), 0xA5);
    }
}
