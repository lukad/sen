use bincode::{Decode, Encode};

use crate::{
    apu::{Apu, DmcDmaKind, DmcDmaRequest},
    bus::Bus,
    cartridge::Cartridge,
    cheat::GameGenieCode,
    controller::{ControllerButtons, ControllerPort},
    cpu::CpuCycleKind,
    mapper::{BoardState, SaveRamError},
    ppu::{Ppu, PpuTickOutput},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Encode, Decode)]
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Encode, Decode)]
struct OamDma {
    page: u8,
    step: OamDmaStep,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Encode, Decode)]
enum OamDmaStep {
    PendingHalt,
    Halt,
    Read { offset: u8 },
    Write { offset: u8, value: u8 },
}

impl OamDma {
    fn is_running(&self) -> bool {
        match self.step {
            OamDmaStep::PendingHalt => false,
            OamDmaStep::Halt | OamDmaStep::Read { .. } | OamDmaStep::Write { .. } => true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Encode, Decode)]
struct DmcDma {
    addr: u16,
    step: DmcDmaStep,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Encode, Decode)]
enum DmcLoadDelay {
    One,
    Two,
    Three,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Encode, Decode)]
enum DmcDmaStep {
    LoadDelay(DmcLoadDelay),
    WaitForHaltPhase(DmaCycle),
    AttemptingHalt,
    Halt,
    Dummy,
    Get,
}

impl DmcDma {
    fn is_running(&self) -> bool {
        matches!(
            self.step,
            DmcDmaStep::Halt | DmcDmaStep::Dummy | DmcDmaStep::Get
        )
    }
}

#[derive(Clone, PartialEq, Encode, Decode)]
pub(crate) struct NesCpuBusState {
    ram: [u8; 0x0800],
    board: BoardState,
    cycle_count: u64,
    ppu: Ppu,
    dma_cycle: DmaCycle,
    oam_dma: Option<OamDma>,
    dmc_dma: Option<DmcDma>,
    controller_ports: [ControllerPort; 2],
    controller_inputs: [ControllerButtons; 2],
    apu: Apu,
}

pub struct NesCpuBus {
    ram: [u8; 0x0800],
    cartridge: Cartridge,
    cycle_count: u64,
    ppu: Ppu,
    dma_cycle: DmaCycle,
    oam_dma: Option<OamDma>,
    dmc_dma: Option<DmcDma>,
    controller_ports: [ControllerPort; 2],
    controller_inputs: [ControllerButtons; 2],
    apu: Apu,
    game_genie_codes: Vec<GameGenieCode>,
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
            controller_ports: [ControllerPort::default(); 2],
            controller_inputs: [ControllerButtons::default(); 2],
            apu: Apu::new(sample_rate),
            game_genie_codes: vec![],
        }
    }

    pub(crate) fn tick_ppu(&mut self) -> PpuTickOutput {
        self.ppu.tick(&mut self.cartridge)
    }

    pub(crate) fn finish_cpu_cycle(&mut self, emit_sample: impl FnMut(f32)) {
        self.apu.tick(emit_sample);

        if let Some(request) = self.apu.take_dmc_dma_request() {
            self.schedule_dmc_dma(request);
        }

        self.advance_dma_cycle(1);
        self.cycle_count = self.cycle_count.wrapping_add(1);
    }

    pub(crate) fn system_ram(&self) -> &[u8] {
        &self.ram
    }

    pub(crate) fn system_ram_mut(&mut self) -> &mut [u8] {
        &mut self.ram
    }

    pub(crate) fn set_game_genie_codes(&mut self, codes: Vec<GameGenieCode>) {
        self.game_genie_codes = codes;
    }

    fn apply_game_genie(&self, address: u16, original: u8) -> u8 {
        self.game_genie_codes
            .iter()
            .find_map(|code| code.apply(address, original))
            .unwrap_or(original)
    }

    pub(crate) fn dma_running(&self) -> bool {
        let oam_running = self.oam_dma.as_ref().is_some_and(OamDma::is_running);
        let dmc_running = self.dmc_dma.as_ref().is_some_and(DmcDma::is_running);

        oam_running || dmc_running
    }

    pub(crate) fn try_start_dma_halt(&mut self, cpu_cycle: CpuCycleKind) -> bool {
        self.advance_dmc_waiting();

        let oam_wants_halt = self
            .oam_dma
            .as_ref()
            .is_some_and(|dma| dma.step == OamDmaStep::PendingHalt);

        let dmc_wants_halt = self
            .dmc_dma
            .as_ref()
            .is_some_and(|dma| dma.step == DmcDmaStep::AttemptingHalt);

        if !oam_wants_halt && !dmc_wants_halt {
            return false;
        }

        if cpu_cycle == CpuCycleKind::Write {
            return false;
        }

        if let Some(oam) = &mut self.oam_dma
            && oam.step == OamDmaStep::PendingHalt
        {
            oam.step = OamDmaStep::Halt;
        }

        self.start_dmc_if_attempting();
        true
    }

    fn advance_dmc_waiting(&mut self) {
        let dma_cycle = self.dma_cycle;

        let Some(dma) = &mut self.dmc_dma else {
            return;
        };

        dma.step = match dma.step {
            DmcDmaStep::LoadDelay(DmcLoadDelay::Three) => DmcDmaStep::LoadDelay(DmcLoadDelay::Two),
            DmcDmaStep::LoadDelay(DmcLoadDelay::Two) => DmcDmaStep::LoadDelay(DmcLoadDelay::One),
            DmcDmaStep::LoadDelay(DmcLoadDelay::One) => DmcDmaStep::WaitForHaltPhase(DmaCycle::Get),
            DmcDmaStep::WaitForHaltPhase(phase) if dma_cycle == phase => DmcDmaStep::AttemptingHalt,
            step => step,
        };
    }

    fn start_dmc_if_attempting(&mut self) {
        if let Some(dma) = &mut self.dmc_dma
            && dma.step == DmcDmaStep::AttemptingHalt
        {
            dma.step = DmcDmaStep::Halt;
        }
    }

    pub(crate) fn perform_dma_bus_action(&mut self) {
        self.maybe_start_dmc_dma_if_cpu_already_halted();

        let dmc_used_bus = self.tick_dmc_dma_cycle();
        self.tick_oam_dma_cycle(dmc_used_bus);
    }

    fn tick_dmc_dma_cycle(&mut self) -> bool {
        let Some(mut dma) = self.dmc_dma.take() else {
            return false;
        };

        match dma.step {
            DmcDmaStep::Halt => {
                dma.step = DmcDmaStep::Dummy;
                self.dmc_dma = Some(dma);
                false
            }
            DmcDmaStep::Dummy => {
                dma.step = DmcDmaStep::Get;
                self.dmc_dma = Some(dma);
                false
            }
            DmcDmaStep::Get if self.dma_cycle == DmaCycle::Get => {
                let value = self.read_without_dma(dma.addr);
                self.apu.finish_dmc_dma(value);
                true
            }
            _ => {
                self.dmc_dma = Some(dma);
                false
            }
        }
    }

    fn tick_oam_dma_cycle(&mut self, dmc_used_bus: bool) {
        let Some(OamDma { page, step }) = self.oam_dma.take() else {
            return;
        };

        let next = match step {
            OamDmaStep::PendingHalt | OamDmaStep::Halt => Some(OamDma {
                page,
                step: OamDmaStep::Read { offset: 0 },
            }),
            OamDmaStep::Read { offset } if self.dma_cycle == DmaCycle::Get && !dmc_used_bus => {
                let addr = (u16::from(page) << 8) | u16::from(offset);
                let value = self.read_without_dma(addr);

                Some(OamDma {
                    page,
                    step: OamDmaStep::Write { offset, value },
                })
            }
            OamDmaStep::Write { offset, value } if self.dma_cycle == DmaCycle::Put => {
                self.ppu.write_oam_dma_byte(value);

                if offset == 0xFF {
                    None
                } else {
                    Some(OamDma {
                        page,
                        step: OamDmaStep::Read {
                            offset: offset.wrapping_add(1),
                        },
                    })
                }
            }
            step => Some(OamDma { page, step }),
        };

        self.oam_dma = next;
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
            step: OamDmaStep::PendingHalt,
        });
    }

    fn schedule_dmc_dma(&mut self, request: DmcDmaRequest) {
        if self.dmc_dma.is_some() {
            return;
        }

        let step = match request.kind {
            DmcDmaKind::Load => match self.dma_cycle {
                DmaCycle::Get => DmcDmaStep::LoadDelay(DmcLoadDelay::Three),
                DmaCycle::Put => DmcDmaStep::LoadDelay(DmcLoadDelay::Two),
            },
            DmcDmaKind::Reload => DmcDmaStep::WaitForHaltPhase(DmaCycle::Put),
        };

        self.dmc_dma = Some(DmcDma {
            addr: request.addr,
            step,
        });
    }

    fn maybe_start_dmc_dma_if_cpu_already_halted(&mut self) {
        let oam_running = self.oam_dma.as_ref().is_some_and(OamDma::is_running);
        let dmc_running = self.dmc_dma.as_ref().is_some_and(DmcDma::is_running);

        if !oam_running || dmc_running {
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
            0x4016 => 0x40 | self.controller_ports[0].read(self.controller_inputs[0]),
            0x4017 => 0x40 | self.controller_ports[1].read(self.controller_inputs[1]),
            0x4000..=0x401F => 0,
            0x4020..=0x7FFF => self.cartridge.cpu_read(addr).unwrap_or(0),
            0x8000..=0xFFFF => {
                let original = self.cartridge.cpu_read(addr).unwrap_or(0);
                self.apply_game_genie(addr, original)
            }
        }
    }

    pub(crate) fn set_controller1(&mut self, buttons: ControllerButtons) {
        self.controller_inputs[0] = buttons;
    }

    pub(crate) fn set_controller2(&mut self, buttons: ControllerButtons) {
        self.controller_inputs[1] = buttons;
    }

    pub(crate) fn save_ram(&self) -> Option<&[u8]> {
        self.cartridge.save_ram()
    }

    pub(crate) fn save_ram_mut(&mut self) -> Option<&mut [u8]> {
        self.cartridge.save_ram_mut()
    }

    pub(crate) fn load_save_ram(&mut self, data: &[u8]) -> Result<(), SaveRamError> {
        self.cartridge.load_save_ram(data)
    }

    pub(crate) fn ram_regions_mut(&mut self) -> (&mut [u8], Option<&mut [u8]>, bool) {
        let Self { ram, cartridge, .. } = self;
        let prg_is_battery_backed = cartridge.has_battery();
        let prg = cartridge.prg_ram_mut();
        (ram, prg, prg_is_battery_backed)
    }

    pub(crate) fn state(&self) -> NesCpuBusState {
        NesCpuBusState {
            ram: self.ram,
            board: self.cartridge.board_state(),
            cycle_count: self.cycle_count,
            ppu: self.ppu.clone(),
            dma_cycle: self.dma_cycle,
            oam_dma: self.oam_dma,
            dmc_dma: self.dmc_dma,
            controller_ports: self.controller_ports,
            controller_inputs: self.controller_inputs,
            apu: self.apu.clone(),
        }
    }

    pub(crate) fn restore_state(&mut self, state: NesCpuBusState) {
        let NesCpuBusState {
            ram,
            board,
            cycle_count,
            ppu,
            dma_cycle,
            oam_dma,
            dmc_dma,
            controller_ports,
            controller_inputs,
            apu,
        } = state;

        self.ram = ram;
        self.cartridge.restore_board_state(board);
        self.cycle_count = cycle_count;
        self.ppu = ppu;
        self.dma_cycle = dma_cycle;
        self.oam_dma = oam_dma;
        self.dmc_dma = dmc_dma;
        self.controller_ports = controller_ports;
        self.controller_inputs = controller_inputs;
        self.apu = apu;
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
                self.controller_ports[0].write_strobe(value, self.controller_inputs[0]);
                self.controller_ports[1].write_strobe(value, self.controller_inputs[1]);
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

        bus.perform_dma_bus_action();
        bus.finish_cpu_cycle(|_| {});
        cycles += 1;

        while bus.dma_running() {
            bus.perform_dma_bus_action();
            bus.finish_cpu_cycle(|_| {});
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
    fn game_genie_code_replaces_mapper_reads_until_cleared() {
        let mut prg_rom = vec![0; 0x4000];
        prg_rom[0x11DD] = 0xA5; // $D1DD in the upper mirror of 16 KiB NROM.
        let mut bus = bus_with_prg(&prg_rom);

        assert_eq!(bus.read(0xD1DD), 0xA5);

        bus.set_game_genie_codes(vec!["GOSSIP".parse().unwrap()]);
        assert_eq!(bus.read(0xD1DD), 0x14);

        bus.set_game_genie_codes(Vec::new());
        assert_eq!(bus.read(0xD1DD), 0xA5);
    }

    #[test]
    fn game_genie_compare_code_only_replaces_matching_mapper_data() {
        let mut matching_prg = vec![0; 0x4000];
        matching_prg[0x14A7] = 0x03; // $94A7 in the lower 16 KiB NROM window.
        let mut matching_bus = bus_with_prg(&matching_prg);
        matching_bus.set_game_genie_codes(vec!["ZEXPYGLA".parse().unwrap()]);

        let mut different_prg = matching_prg;
        different_prg[0x14A7] = 0x04;
        let mut different_bus = bus_with_prg(&different_prg);
        different_bus.set_game_genie_codes(vec!["ZEXPYGLA".parse().unwrap()]);

        assert_eq!(matching_bus.read(0x94A7), 0x02);
        assert_eq!(different_bus.read(0x94A7), 0x04);
    }

    #[test]
    fn restoring_machine_state_preserves_active_game_genie_codes() {
        let mut prg_rom = vec![0; 0x4000];
        prg_rom[0x11DD] = 0xA5;
        let mut bus = bus_with_prg(&prg_rom);
        bus.set_game_genie_codes(vec!["GOSSIP".parse().unwrap()]);
        let state = bus.state();

        bus.write(0x0123, 0x5A);
        bus.restore_state(state);

        assert_eq!(bus.read(0x0123), 0);
        assert_eq!(bus.read(0xD1DD), 0x14);
    }

    #[test]
    fn oam_dma_stalls_513_cycles_when_halt_lands_on_put_cycle() {
        let prg_rom = vec![0; 0x4000];
        let mut bus = bus_with_prg(&prg_rom);

        fill_ram_page(&mut bus, 0x02, 0xAB);

        bus.write(0x4014, 0x02);
        bus.finish_cpu_cycle(|_| {});

        assert_eq!(read_oam(&mut bus, 0x00), 0xFF);
        assert_eq!(drain_dma_from_next_read(&mut bus), 513);

        assert_eq!(read_oam(&mut bus, 0x00), 0xAB);
        assert_eq!(read_oam(&mut bus, 0xFF), 0xAB);
    }

    #[test]
    fn oam_dma_stalls_514_cycles_when_halt_lands_on_get_cycle() {
        let prg_rom = vec![0; 0x4000];
        let mut bus = bus_with_prg(&prg_rom);

        bus.finish_cpu_cycle(|_| {});
        fill_ram_page(&mut bus, 0x02, 0xCD);

        bus.write(0x4014, 0x02);
        bus.finish_cpu_cycle(|_| {});

        assert_eq!(read_oam(&mut bus, 0x00), 0xFF);
        assert_eq!(drain_dma_from_next_read(&mut bus), 514);

        assert_eq!(read_oam(&mut bus, 0x00), 0xCD);
        assert_eq!(read_oam(&mut bus, 0xFF), 0xCD);
    }

    #[test]
    fn oam_dma_step_encodes_the_next_bus_action() {
        let prg_rom = vec![0; 0x4000];
        let mut bus = bus_with_prg(&prg_rom);

        bus.write(0x0200, 0xAB);
        bus.finish_cpu_cycle(|_| {}); // Get -> Put

        bus.write(0x4014, 0x02);
        assert_eq!(bus.oam_dma.as_ref().unwrap().step, OamDmaStep::PendingHalt);
        assert!(!bus.dma_running());

        bus.finish_cpu_cycle(|_| {}); // Put -> Get

        assert!(bus.try_start_dma_halt(CpuCycleKind::Read));
        assert_eq!(bus.oam_dma.as_ref().unwrap().step, OamDmaStep::Halt);
        assert!(bus.dma_running());

        bus.perform_dma_bus_action();
        assert_eq!(
            bus.oam_dma.as_ref().unwrap().step,
            OamDmaStep::Read { offset: 0 }
        );
        bus.finish_cpu_cycle(|_| {}); // Get -> Put

        // Alignment cycle: OAM cannot read during Put
        bus.perform_dma_bus_action();
        assert_eq!(
            bus.oam_dma.as_ref().unwrap().step,
            OamDmaStep::Read { offset: 0 }
        );
        bus.finish_cpu_cycle(|_| {}); // Put -> Get

        bus.perform_dma_bus_action();
        assert_eq!(
            bus.oam_dma.as_ref().unwrap().step,
            OamDmaStep::Write {
                offset: 0,
                value: 0xAB,
            }
        );
        bus.finish_cpu_cycle(|_| {}); // Get -> Put

        bus.perform_dma_bus_action();
        assert_eq!(
            bus.oam_dma.as_ref().unwrap().step,
            OamDmaStep::Read { offset: 1 }
        );
        assert_eq!(read_oam(&mut bus, 0), 0xAB);
    }

    fn assert_dmc_step(bus: &NesCpuBus, addr: u16, step: DmcDmaStep) {
        assert_eq!(bus.dmc_dma, Some(DmcDma { addr, step }));
    }

    #[test]
    fn dmc_load_dma_step_encodes_the_exact_continuation() {
        let mut prg_rom = vec![0; 0x4000];
        prg_rom[0] = 0xA5; // $C000 in mirrored 16 KiB NROM
        let mut bus = bus_with_prg(&prg_rom);

        // One-byte sample at $C000, with IRQ on completion
        bus.write(0x4010, 0x80);
        bus.write(0x4012, 0x00);
        bus.write(0x4013, 0x00);
        bus.write(0x4015, 0x10);

        // Transfers the APU request to the bus on Get, then advances to Put
        bus.finish_cpu_cycle(|_| {});

        assert_eq!(bus.dma_cycle, DmaCycle::Put);
        assert_dmc_step(&bus, 0xC000, DmcDmaStep::LoadDelay(DmcLoadDelay::Three));
        assert!(!bus.dma_running());

        assert!(!bus.try_start_dma_halt(CpuCycleKind::Read));
        assert_dmc_step(&bus, 0xC000, DmcDmaStep::LoadDelay(DmcLoadDelay::Two));
        bus.finish_cpu_cycle(|_| {}); // Put -> Get

        assert!(!bus.try_start_dma_halt(CpuCycleKind::Read));
        assert_dmc_step(&bus, 0xC000, DmcDmaStep::LoadDelay(DmcLoadDelay::One));
        bus.finish_cpu_cycle(|_| {}); // Get -> Put

        assert!(!bus.try_start_dma_halt(CpuCycleKind::Read));
        assert_dmc_step(&bus, 0xC000, DmcDmaStep::WaitForHaltPhase(DmaCycle::Get));
        bus.finish_cpu_cycle(|_| {}); // Put -> Get

        // The phase is now eligible, but a CPU write prevents the halt
        assert!(!bus.try_start_dma_halt(CpuCycleKind::Write));
        assert_dmc_step(&bus, 0xC000, DmcDmaStep::AttemptingHalt);
        assert!(!bus.dma_running());
        bus.finish_cpu_cycle(|_| {}); // Get -> Put

        // The halt may succeed on the next CPU read regardless of the original target phase
        assert!(bus.try_start_dma_halt(CpuCycleKind::Read));
        assert_dmc_step(&bus, 0xC000, DmcDmaStep::Halt);
        assert!(bus.dma_running());

        bus.perform_dma_bus_action();
        assert_dmc_step(&bus, 0xC000, DmcDmaStep::Dummy);
        bus.finish_cpu_cycle(|_| {}); // Put -> Get

        bus.perform_dma_bus_action();
        assert_dmc_step(&bus, 0xC000, DmcDmaStep::Get);
        bus.finish_cpu_cycle(|_| {}); // Get -> Put

        // The fetch itself requires Get
        bus.perform_dma_bus_action();
        assert_dmc_step(&bus, 0xC000, DmcDmaStep::Get);
        assert!(!bus.irq_asserted());
        bus.finish_cpu_cycle(|_| {}); // Put -> Get

        bus.perform_dma_bus_action();

        assert!(bus.dmc_dma.is_none());
        assert!(!bus.dma_running());
        assert!(bus.irq_asserted());
    }

    #[test]
    fn dmc_reload_waits_for_put_before_attempting_the_halt() {
        let prg_rom = vec![0; 0x4000];
        let mut bus = bus_with_prg(&prg_rom);

        bus.schedule_dmc_dma(DmcDmaRequest {
            addr: 0xC123,
            kind: DmcDmaKind::Reload,
        });

        assert_eq!(bus.dma_cycle, DmaCycle::Get);
        assert_dmc_step(&bus, 0xC123, DmcDmaStep::WaitForHaltPhase(DmaCycle::Put));
        assert!(!bus.dma_running());

        // Reload cannot attempt its halt during Get
        assert!(!bus.try_start_dma_halt(CpuCycleKind::Read));
        assert_dmc_step(&bus, 0xC123, DmcDmaStep::WaitForHaltPhase(DmaCycle::Put));

        bus.finish_cpu_cycle(|_| {}); // Get -> Put

        assert!(bus.try_start_dma_halt(CpuCycleKind::Read));
        assert_dmc_step(&bus, 0xC123, DmcDmaStep::Halt);
        assert!(bus.dma_running());
    }
}
