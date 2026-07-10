use crate::{
    apu::{Apu, DmcDmaKind, DmcDmaRequest},
    bus::Bus,
    cartridge::Cartridge,
    controller::{Controller, ControllerButtons},
    cpu::CpuCycleKind,
    frame::Frame,
    mapper::SaveRamError,
    ppu::Ppu,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DmaCycle {
    Get,
    Put,
}

impl DmaCycle {
    fn next(self) -> Self {
        match self {
            DmaCycle::Get => DmaCycle::Put,
            DmaCycle::Put => DmaCycle::Get,
        }
    }
}

struct OamDma {
    page: u8,
    offset: u8,
    latch: Option<u8>,
    state: OamDmaState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OamDmaState {
    PendingHalt,
    Halt,
    Transfer,
}

enum DmcDma {
    Waiting {
        request: DmcDmaRequest,
        halt_phase: DmaCycle,
        wait_cycles: u8,
    },
    AttemptingHalt {
        request: DmcDmaRequest,
    },
    Running {
        request: DmcDmaRequest,
        step: DmcDmaStep,
    },
}

impl DmcDma {
    fn is_running(&self) -> bool {
        matches!(self, Self::Running { .. })
    }
}

enum DmcDmaStep {
    Halt,
    Dummy,
    Get,
}

pub struct NesCpuBus {
    ram: [u8; 0x0800],
    cartridge: Cartridge,
    cycle_count: u64,
    ppu: Ppu,
    dma_cycle: DmaCycle,
    oam_dma: Option<OamDma>,
    dmc_dma: Option<DmcDma>,
    controller1: Controller,
    controller2: Controller,
    apu: Apu,
}

impl NesCpuBus {
    pub fn new(cartridge: Cartridge) -> Self {
        Self::new_with_sample_rate(cartridge, 44_100.0)
    }

    pub fn new_with_sample_rate(cartridge: Cartridge, sample_rate: f64) -> Self {
        Self {
            ram: [0; 0x0800],
            cartridge,
            cycle_count: 0,
            ppu: Ppu::new(),
            dma_cycle: DmaCycle::Get,
            oam_dma: None,
            dmc_dma: None,
            controller1: Default::default(),
            controller2: Default::default(),
            apu: Apu::new(sample_rate),
        }
    }

    pub(crate) fn frame(&self) -> &Frame {
        self.ppu.frame()
    }

    pub(crate) fn tick_ppu(&mut self, cycles: usize) -> bool {
        let mut frame_complete = false;

        for _ in 0..cycles {
            frame_complete |= self.ppu.tick(&mut self.cartridge)
        }

        frame_complete
    }

    pub(crate) fn tick_after_cpu_cycle(&mut self) -> bool {
        let frame_complete = self.tick_ppu(3);
        self.apu.tick();
        if let Some(request) = self.apu.take_dmc_dma_request() {
            self.schedule_dmc_dma(request);
        }
        self.advance_dma_cycle(1);
        self.cycle_count = self.cycle_count.wrapping_add(1);
        frame_complete
    }

    pub(crate) fn dma_running(&self) -> bool {
        let oam_running = self
            .oam_dma
            .as_ref()
            .is_some_and(|dma| matches!(dma.state, OamDmaState::Halt | OamDmaState::Transfer));

        let dmc_running = self.dmc_dma.as_ref().is_some_and(DmcDma::is_running);

        oam_running || dmc_running
    }

    pub(crate) fn try_start_dma_halt(&mut self, cpu_cycle: CpuCycleKind) -> bool {
        self.advance_dmc_waiting();

        let oam_wants_halt = self
            .oam_dma
            .as_ref()
            .is_some_and(|dma| dma.state == OamDmaState::PendingHalt);

        let dmc_wants_halt = matches!(self.dmc_dma, Some(DmcDma::AttemptingHalt { .. }));

        if !oam_wants_halt && !dmc_wants_halt {
            return false;
        }

        if cpu_cycle == CpuCycleKind::Write {
            return false;
        }

        if let Some(oam) = &mut self.oam_dma
            && oam.state == OamDmaState::PendingHalt
        {
            oam.state = OamDmaState::Halt;
        }

        self.start_dmc_if_attempting();
        true
    }

    fn advance_dmc_waiting(&mut self) {
        self.dmc_dma = match self.dmc_dma.take() {
            Some(DmcDma::Waiting {
                request,
                halt_phase,
                wait_cycles,
            }) if wait_cycles > 0 => Some(DmcDma::Waiting {
                request,
                halt_phase,
                wait_cycles: wait_cycles - 1,
            }),

            Some(DmcDma::Waiting {
                request,
                halt_phase,
                ..
            }) if self.dma_cycle == halt_phase => Some(DmcDma::AttemptingHalt { request }),

            other => other,
        };
    }

    fn start_dmc_if_attempting(&mut self) {
        self.dmc_dma = match self.dmc_dma.take() {
            Some(DmcDma::AttemptingHalt { request }) => Some(DmcDma::Running {
                request,
                step: DmcDmaStep::Halt,
            }),
            other => other,
        };
    }

    pub(crate) fn tick_dma_cycle(&mut self) -> bool {
        self.maybe_start_dmc_dma_if_cpu_already_halted();

        let dmc_used_bus = self.tick_dmc_dma_cycle();
        self.tick_oam_dma_cycle(dmc_used_bus);

        self.tick_after_cpu_cycle()
    }
    fn tick_dmc_dma_cycle(&mut self) -> bool {
        match self.dmc_dma.take() {
            Some(DmcDma::Running { request, step }) => match step {
                DmcDmaStep::Halt => {
                    self.dmc_dma = Some(DmcDma::Running {
                        request,
                        step: DmcDmaStep::Dummy,
                    });
                    false
                }
                DmcDmaStep::Dummy => {
                    self.dmc_dma = Some(DmcDma::Running {
                        request,
                        step: DmcDmaStep::Get,
                    });
                    false
                }
                DmcDmaStep::Get if self.dma_cycle == DmaCycle::Get => {
                    let value = self.read_without_dma(request.addr);
                    self.apu.finish_dmc_dma(value);
                    true
                }
                DmcDmaStep::Get => {
                    self.dmc_dma = Some(DmcDma::Running {
                        request,
                        step: DmcDmaStep::Get,
                    });
                    false
                }
            },
            other => {
                self.dmc_dma = other;
                false
            }
        }
    }

    fn tick_oam_dma_cycle(&mut self, dmc_used_bus: bool) {
        let Some(mut dma) = self.oam_dma.take() else {
            return;
        };

        match dma.state {
            OamDmaState::PendingHalt | OamDmaState::Halt => {
                dma.state = OamDmaState::Transfer;
                self.oam_dma = Some(dma);
            }
            OamDmaState::Transfer => {
                match self.dma_cycle {
                    DmaCycle::Get => {
                        if dmc_used_bus {
                            self.oam_dma = Some(dma);
                            return;
                        }

                        let addr = ((dma.page as u16) << 8) | dma.offset as u16;
                        dma.latch = Some(self.read_without_dma(addr));
                    }
                    DmaCycle::Put => {
                        if let Some(value) = dma.latch.take() {
                            self.ppu.write_oam_dma_byte(value);

                            if dma.offset == 0xFF {
                                return;
                            }

                            dma.offset = dma.offset.wrapping_add(1);
                        }
                    }
                }

                self.oam_dma = Some(dma);
            }
        }
    }

    pub(crate) fn take_nmi(&mut self) -> bool {
        self.ppu.take_nmi()
    }

    pub(crate) fn irq_asserted(&self) -> bool {
        self.apu.irq_asserted() || self.cartridge.irq_asserted()
    }

    fn schedule_oam_dma(&mut self, page: u8) {
        self.oam_dma = Some(OamDma {
            page,
            offset: 0,
            latch: None,
            state: OamDmaState::PendingHalt,
        })
    }

    fn schedule_dmc_dma(&mut self, request: DmcDmaRequest) {
        if self.dmc_dma.is_some() {
            return;
        }

        let (halt_phase, wait_cycles) = match request.kind {
            DmcDmaKind::Load => {
                let wait_cycles = match self.dma_cycle {
                    DmaCycle::Get => 3,
                    DmaCycle::Put => 2,
                };

                (DmaCycle::Get, wait_cycles)
            }
            DmcDmaKind::Reload => (DmaCycle::Put, 0),
        };

        self.dmc_dma = Some(DmcDma::Waiting {
            request,
            halt_phase,
            wait_cycles,
        });
    }

    fn maybe_start_dmc_dma_if_cpu_already_halted(&mut self) {
        let cpu_already_halted = self
            .oam_dma
            .as_ref()
            .is_some_and(|dma| matches!(dma.state, OamDmaState::Halt | OamDmaState::Transfer));

        if !cpu_already_halted || self.dmc_dma.as_ref().is_some_and(DmcDma::is_running) {
            return;
        }

        self.advance_dmc_waiting();
        self.start_dmc_if_attempting();
    }

    fn advance_dma_cycle(&mut self, cycles: usize) {
        if !cycles.is_multiple_of(2) {
            self.dma_cycle = self.dma_cycle.next();
        }
    }

    fn read_without_dma(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1FFF => self.ram[(addr & 0x07FF) as usize],
            0x2000..=0x3FFF => self.ppu.cpu_read(addr, &mut self.cartridge),
            0x4015 => self.apu.read_status(),
            0x4016 => 0x40 | self.controller1.read(),
            0x4017 => 0x40 | self.controller2.read(),
            0x4000..=0x401F => 0,
            0x4020..=0xFFFF => self.cartridge.cpu_read(addr).unwrap_or(0),
        }
    }

    pub(crate) fn set_controller1(&mut self, buttons: ControllerButtons) {
        self.controller1.set_buttons(buttons);
    }

    pub(crate) fn set_controller2(&mut self, buttons: ControllerButtons) {
        self.controller2.set_buttons(buttons);
    }

    pub(crate) fn pop_audio_sample(&mut self) -> Option<f32> {
        self.apu.pop_sample()
    }

    pub(crate) fn save_ram(&self) -> Option<&[u8]> {
        self.cartridge.save_ram()
    }

    pub(crate) fn load_save_ram(&mut self, data: &[u8]) -> Result<(), SaveRamError> {
        self.cartridge.load_save_ram(data)
    }
}

impl Bus for NesCpuBus {
    fn read(&mut self, addr: u16) -> u8 {
        #[cfg(feature = "tracing")]
        tracing::trace!(addr = format_args!("{:#06X}", addr), "cpu read");

        self.read_without_dma(addr)
    }

    fn write(&mut self, addr: u16, value: u8) {
        #[cfg(feature = "tracing")]
        tracing::trace!(
            addr = format_args!("{:#06X}", addr),
            value = format_args!("{:#04X}", value),
            "cpu write"
        );

        match addr {
            0x0000..=0x1FFF => self.ram[(addr & 0x07FF) as usize] = value,
            0x2000..=0x3FFF => self.ppu.cpu_write(addr, value, &mut self.cartridge),
            0x4014 => self.schedule_oam_dma(value),
            0x4016 => {
                self.controller1.write_strobe(value);
                self.controller2.write_strobe(value);
            }
            0x4000..=0x4013 | 0x4015 | 0x4017 => {
                self.apu.write_register(addr, value);
            }
            0x4000..=0x401F => (),
            0x4020..=0xFFFF => self.cartridge.cpu_write(addr, value, self.cycle_count),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cartridge_with_prg(prg_rom: &[u8]) -> Cartridge {
        assert!(matches!(prg_rom.len(), 0x4000 | 0x8000));

        let mut rom = vec![0; 16];
        rom[0..4].copy_from_slice(b"NES\x1A");
        rom[4] = (prg_rom.len() / 0x4000) as u8;
        rom[5] = 1;
        rom.extend_from_slice(prg_rom);
        rom.extend_from_slice(&vec![0; 0x2000]);

        Cartridge::from_ines(&rom).unwrap()
    }

    fn bus_with_prg(prg_rom: &[u8]) -> NesCpuBus {
        NesCpuBus::new(cartridge_with_prg(prg_rom))
    }

    fn fill_ram_page(bus: &mut NesCpuBus, page: u8, value: u8) {
        let base = (page as u16) << 8;

        for offset in 0..=0xFF {
            bus.write(base + offset, value);
        }
    }

    fn read_oam(bus: &mut NesCpuBus, addr: u8) -> u8 {
        bus.write(0x2003, addr);
        bus.read(0x2004)
    }

    fn drain_dma_from_next_read(bus: &mut NesCpuBus) -> usize {
        let mut cycles = 0;

        assert!(bus.try_start_dma_halt(CpuCycleKind::Read));

        bus.tick_dma_cycle();
        cycles += 1;

        while bus.dma_running() {
            bus.tick_dma_cycle();
            cycles += 1;
        }

        cycles
    }

    #[test]
    fn nes_cpu_bus_mirrors_internal_ram_every_2k() {
        let prg_rom = vec![0; 0x4000];
        let mut bus = bus_with_prg(&prg_rom);

        bus.write(0x0000, 0x12);

        assert_eq!(bus.read(0x0000), 0x12);
        assert_eq!(bus.read(0x0800), 0x12);
        assert_eq!(bus.read(0x1000), 0x12);
        assert_eq!(bus.read(0x1800), 0x12);

        bus.write(0x17FF, 0x34);

        assert_eq!(bus.read(0x07FF), 0x34);
        assert_eq!(bus.read(0x0FFF), 0x34);
        assert_eq!(bus.read(0x17FF), 0x34);
        assert_eq!(bus.read(0x1FFF), 0x34);
    }

    #[test]
    fn nes_cpu_bus_reads_16k_nrom_prg_rom_with_upper_bank_mirrored() {
        let mut prg_rom = vec![0; 0x4000];
        prg_rom[0x0000] = 0x11;
        prg_rom[0x3FFF] = 0x22;
        let mut bus = bus_with_prg(&prg_rom);

        assert_eq!(bus.read(0x8000), 0x11);
        assert_eq!(bus.read(0xBFFF), 0x22);
        assert_eq!(bus.read(0xC000), 0x11);
        assert_eq!(bus.read(0xFFFF), 0x22);
    }

    #[test]
    fn nes_cpu_bus_reads_32k_nrom_prg_rom_directly() {
        let mut prg_rom = vec![0; 0x8000];
        prg_rom[0x0000] = 0x11;
        prg_rom[0x3FFF] = 0x22;
        prg_rom[0x4000] = 0x33;
        prg_rom[0x7FFF] = 0x44;
        let mut bus = bus_with_prg(&prg_rom);

        assert_eq!(bus.read(0x8000), 0x11);
        assert_eq!(bus.read(0xBFFF), 0x22);
        assert_eq!(bus.read(0xC000), 0x33);
        assert_eq!(bus.read(0xFFFF), 0x44);
    }

    #[test]
    fn nes_cpu_bus_does_not_mutate_nrom_prg_rom_on_write() {
        let mut prg_rom = vec![0; 0x4000];
        prg_rom[0x0000] = 0x11;
        prg_rom[0x3FFF] = 0x22;
        let mut bus = bus_with_prg(&prg_rom);

        bus.write(0x8000, 0x99);
        bus.write(0xFFFF, 0x88);

        assert_eq!(bus.read(0x8000), 0x11);
        assert_eq!(bus.read(0xFFFF), 0x22);
    }

    #[test]
    fn oam_dma_stalls_513_cycles_when_halt_lands_on_put_cycle() {
        let prg_rom = vec![0; 0x4000];
        let mut bus = bus_with_prg(&prg_rom);

        fill_ram_page(&mut bus, 0x02, 0xAB);

        bus.write(0x4014, 0x02);
        bus.tick_after_cpu_cycle();

        assert_eq!(read_oam(&mut bus, 0x00), 0xFF);
        assert_eq!(drain_dma_from_next_read(&mut bus), 513);

        assert_eq!(read_oam(&mut bus, 0x00), 0xAB);
        assert_eq!(read_oam(&mut bus, 0xFF), 0xAB);
    }

    #[test]
    fn oam_dma_stalls_514_cycles_when_halt_lands_on_get_cycle() {
        let prg_rom = vec![0; 0x4000];
        let mut bus = bus_with_prg(&prg_rom);

        bus.tick_after_cpu_cycle();
        fill_ram_page(&mut bus, 0x02, 0xCD);

        bus.write(0x4014, 0x02);
        bus.tick_after_cpu_cycle();

        assert_eq!(read_oam(&mut bus, 0x00), 0xFF);
        assert_eq!(drain_dma_from_next_read(&mut bus), 514);

        assert_eq!(read_oam(&mut bus, 0x00), 0xCD);
        assert_eq!(read_oam(&mut bus, 0xFF), 0xCD);
    }
}
