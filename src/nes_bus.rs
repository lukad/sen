use crate::{bus::Bus, cartridge::Cartridge, ppu::Ppu};

pub struct NesCpuBus {
    ram: [u8; 0x0800],
    cartridge: Cartridge,
    ppu: Ppu,
}

impl NesCpuBus {
    pub fn new(cartridge: Cartridge) -> Self {
        Self {
            ram: [0; 0x0800],
            cartridge,
            ppu: Ppu::new(),
        }
    }
}

impl Bus for NesCpuBus {
    fn read(&mut self, addr: u16) -> u8 {
        #[cfg(feature = "tracing")]
        tracing::trace!(addr = format_args!("{:#06X}", addr), "cpu read");

        match addr {
            0x0000..=0x1FFF => self.ram[(addr & 0x07FF) as usize],
            0x2000..=0x3FFF => self.ppu.cpu_read(addr, &mut self.cartridge),
            0x4000..=0x401F => 0, // APU / IO
            0x4020..=0xFFFF => self.cartridge.cpu_read(addr).unwrap_or(0),
        }
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
}
