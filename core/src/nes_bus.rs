use crate::{
    bus::Bus,
    cartridge::Cartridge,
    controller::{Controller, ControllerButtons},
    frame::Frame,
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
    halted: bool,
}

pub struct NesCpuBus {
    ram: [u8; 0x0800],
    cartridge: Cartridge,
    ppu: Ppu,
    dma_cycle: DmaCycle,
    oam_dma: Option<OamDma>,
    controller1: Controller,
    controller2: Controller,
}

impl NesCpuBus {
    pub fn new(cartridge: Cartridge) -> Self {
        Self {
            ram: [0; 0x0800],
            cartridge,
            ppu: Ppu::new(),
            dma_cycle: DmaCycle::Get,
            oam_dma: None,
            controller1: Default::default(),
            controller2: Default::default(),
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
        self.advance_dma_cycle(1);
        frame_complete
    }

    pub(crate) fn cpu_stalled(&self) -> bool {
        self.oam_dma.is_some()
    }

    pub(crate) fn tick_cpu_stall_cycle(&mut self) -> bool {
        let mut dma = self.oam_dma.take().expect("DMA stall without DMA state");
        let mut complete = false;

        if !dma.halted {
            dma.halted = true;
        } else {
            match self.dma_cycle {
                DmaCycle::Get => {
                    let addr = ((dma.page as u16) << 8) | dma.offset as u16;
                    dma.latch = Some(self.read_without_dma(addr));
                }
                DmaCycle::Put => {
                    if let Some(value) = dma.latch.take() {
                        self.ppu.write_oam_dma_byte(value);

                        complete = dma.offset == 0xFF;
                        dma.offset = dma.offset.wrapping_add(1);
                    }
                }
            }
        }

        if !complete {
            self.oam_dma = Some(dma);
        }

        self.tick_after_cpu_cycle()
    }

    pub(crate) fn take_nmi(&mut self) -> bool {
        self.ppu.take_nmi()
    }

    pub(crate) fn irq_asserted(&self) -> bool {
        false
    }

    fn schedule_oam_dma(&mut self, page: u8) {
        self.oam_dma = Some(OamDma {
            page,
            offset: 0,
            latch: None,
            halted: false,
        })
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
            0x4016 => 0x40 | self.controller1.read(),
            0x4017 => 0x40 | self.controller2.read(),
            0x4000..=0x401F => 0, // APU / IO
            0x4020..=0xFFFF => self.cartridge.cpu_read(addr).unwrap_or(0),
        }
    }

    pub(crate) fn set_controller1(&mut self, buttons: ControllerButtons) {
        self.controller1.set_buttons(buttons);
    }

    pub(crate) fn set_controller2(&mut self, buttons: ControllerButtons) {
        self.controller2.set_buttons(buttons);
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
            0x4000..=0x401F => (), // APU / IO
            0x4020..=0xFFFF => self.cartridge.cpu_write(addr, value),
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

    fn drain_cpu_stall(bus: &mut NesCpuBus) -> usize {
        let mut cycles = 0;

        while bus.cpu_stalled() {
            bus.tick_cpu_stall_cycle();
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
        assert_eq!(drain_cpu_stall(&mut bus), 513);

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
        assert_eq!(drain_cpu_stall(&mut bus), 514);

        assert_eq!(read_oam(&mut bus, 0x00), 0xCD);
        assert_eq!(read_oam(&mut bus, 0xFF), 0xCD);
    }
}
