use crate::cartridge::{Cartridge, Mirroring};

pub struct Ppu {
    /// $2000 PPUCTRL
    ctrl: u8,
    /// $2001 PPUMASK
    mask: u8,
    /// $2002 PPUSTATUS
    status: u8,
    /// $2003 OAMADDR
    oam_addr: u8,
    /// $2004 OAMDATA accessed through `oam_addr`
    oam: [u8; 0x100],
    /// $2007 PPUDATA read buffer
    read_buffer: u8,
    /// VRAM address
    v: u16,
    /// Temporary VRAM address / scroll target
    t: u16,
    /// Fine x scroll
    x: u8,
    /// Write latch
    w: bool,
    /// Internal nametable RAM
    vram: [u8; 0x0800],
    /// Internal palette RAM
    palette: [u8; 0x20],
}

impl Ppu {
    pub(crate) fn new() -> Self {
        Self {
            ctrl: 0,
            mask: 0,
            status: 0,
            oam_addr: 0,
            oam: [0; 0x100],
            read_buffer: 0,
            v: 0,
            t: 0,
            x: 0,
            w: false,
            vram: [0; 0x0800],
            palette: [0; 0x20],
        }
    }

    pub(crate) fn cpu_read(&mut self, addr: u16, cartridge: &mut Cartridge) -> u8 {
        match 0x2000 | (addr & 0x0007) {
            0x2002 => self.read_status(),
            0x2004 => self.read_oam_data(),
            0x2007 => self.read_data(cartridge),
            _ => 0,
        }
    }

    pub(crate) fn cpu_write(&mut self, addr: u16, value: u8, cartridge: &mut Cartridge) {
        match 0x2000 | (addr & 0x0007) {
            0x2000 => self.write_ctrl(value),
            0x2001 => self.mask = value,
            0x2003 => self.oam_addr = value,
            0x2004 => self.write_oam_data(value),
            0x2005 => self.write_scroll(value),
            0x2006 => self.write_addr(value),
            0x2007 => self.write_data(value, cartridge),
            _ => {}
        }
    }

    fn ppu_bus_read(&mut self, addr: u16, cartridge: &mut Cartridge) -> u8 {
        let addr = addr & 0x3FFF;

        match addr {
            0x0000..=0x1FFF => cartridge.ppu_read(addr).unwrap_or(0),
            0x2000..=0x3EFF => {
                let index = self.nametable_index(addr, cartridge.mirroring());
                self.vram[index]
            }
            0x3F00..=0x3FFF => {
                let index = palette_index(addr);
                self.palette[index]
            }
            _ => unreachable!(),
        }
    }

    fn ppu_bus_write(&mut self, addr: u16, value: u8, cartridge: &mut Cartridge) {
        let addr = addr & 0x3FFF;

        match addr {
            0x0000..=0x1FFF => cartridge.ppu_write(addr, value),
            0x2000..=0x3EFF => {
                let index = self.nametable_index(addr, cartridge.mirroring());
                self.vram[index] = value;
            }
            0x3F00..=0x3FFF => {
                let index = palette_index(addr);
                self.palette[index] = value & 0x3F;
            }
            _ => unreachable!(),
        }
    }

    fn nametable_index(&self, addr: u16, mirroring: Mirroring) -> usize {
        let offset = (addr - 0x2000) & 0x0FFF;
        let table = offset / 0x0400;
        let in_table = offset & 0x03FF;

        match mirroring {
            Mirroring::Vertical => match table {
                0 | 2 => in_table as usize,
                1 | 3 => 0x0400 + in_table as usize,
                _ => unreachable!(),
            },
            Mirroring::Horizontal => match table {
                0 | 1 => in_table as usize,
                2 | 3 => 0x0400 + in_table as usize,
                _ => unreachable!(),
            },
            Mirroring::FourScreen => todo!("four-screen mirroring needs 4 KiB nametable storage"),
        }
    }

    fn read_status(&mut self) -> u8 {
        let value = self.status;
        self.status &= !0x80; // clear vblank
        self.w = false; // reset write latch
        value
    }

    fn write_ctrl(&mut self, value: u8) {
        self.ctrl = value;
        self.t = (self.t & !0x0C00) | (((value as u16) & 0x03) << 10);
    }

    fn read_oam_data(&mut self) -> u8 {
        self.oam[self.oam_addr as usize]
    }

    fn write_oam_data(&mut self, value: u8) {
        self.oam[self.oam_addr as usize] = value;
        self.oam_addr = self.oam_addr.wrapping_add(1);
    }

    fn write_scroll(&mut self, value: u8) {
        if !self.w {
            self.t = (self.t & !0x001F) | ((value as u16) >> 3);
            self.x = value & 0x07;
            self.w = true;
        } else {
            self.t = (self.t & !0x73E0)
                | (((value as u16) & 0x07) << 12)
                | (((value as u16) & 0xF8) << 2);
            self.w = false;
        }
    }

    fn write_addr(&mut self, value: u8) {
        if !self.w {
            self.t = (self.t & 0x00FF) | (((value as u16) & 0x3F) << 8);
            self.w = true;
        } else {
            self.t = (self.t & 0x7F00) | value as u16;
            self.v = self.t;
            self.w = false;
        }
    }

    fn read_data(&mut self, cartridge: &mut Cartridge) -> u8 {
        let addr = self.v & 0x3FFF;
        let value = self.ppu_bus_read(addr, cartridge);

        let result = if addr >= 0x3F00 {
            value
        } else {
            let buffered = self.read_buffer;
            self.read_buffer = value;
            buffered
        };

        self.increment_v();

        result
    }

    fn write_data(&mut self, value: u8, cartridge: &mut Cartridge) {
        self.ppu_bus_write(self.v & 0x3FFF, value, cartridge);
        self.increment_v();
    }

    fn increment_v(&mut self) {
        let increment = if self.ctrl & 0x04 != 0 { 32 } else { 1 };
        self.v = self.v.wrapping_add(increment) & 0x7FFF;
    }
}
fn palette_index(addr: u16) -> usize {
    let mut index = (addr - 0x3F00) & 0x001F;

    if matches!(index, 0x10 | 0x14 | 0x18 | 0x1C) {
        index -= 0x10;
    }

    index as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cartridge_with_chr_ram() -> Cartridge {
        let mut rom = vec![0; 16];
        rom[0..4].copy_from_slice(b"NES\x1A");
        rom[4] = 1;
        rom[5] = 0;
        rom.extend_from_slice(&vec![0; 0x4000]);

        Cartridge::from_ines(&rom).unwrap()
    }

    #[test]
    fn write_ctrl_updates_nametable_bits_without_destroying_other_t_bits() {
        let mut ppu = Ppu::new();
        ppu.t = 0x73EF;

        ppu.write_ctrl(0x02);

        assert_eq!(ppu.ctrl, 0x02);
        assert_eq!(ppu.t, 0x7BEF);
    }

    #[test]
    fn write_addr_uses_two_writes_to_load_vram_address() {
        let mut ppu = Ppu::new();

        ppu.write_addr(0x23);
        assert!(ppu.w);
        assert_eq!(ppu.v, 0x0000);

        ppu.write_addr(0x45);
        assert!(!ppu.w);
        assert_eq!(ppu.v, 0x2345);
        assert_eq!(ppu.t, 0x2345);
    }

    #[test]
    fn read_status_clears_vblank_and_resets_write_latch() {
        let mut ppu = Ppu::new();
        ppu.status = 0xE0;
        ppu.w = true;

        assert_eq!(ppu.read_status(), 0xE0);
        assert_eq!(ppu.status, 0x60);
        assert!(!ppu.w);
    }

    #[test]
    fn write_oam_data_stores_at_oam_addr_and_increments_addr() {
        let mut ppu = Ppu::new();
        ppu.oam_addr = 0xFE;

        ppu.write_oam_data(0x12);
        ppu.write_oam_data(0x34);

        assert_eq!(ppu.oam[0xFE], 0x12);
        assert_eq!(ppu.oam[0xFF], 0x34);
        assert_eq!(ppu.oam_addr, 0x00);
    }

    #[test]
    fn ppudata_writes_to_chr_ram_and_increments_by_one_by_default() {
        let mut ppu = Ppu::new();
        let mut cartridge = cartridge_with_chr_ram();

        ppu.cpu_write(0x2006, 0x00, &mut cartridge);
        ppu.cpu_write(0x2006, 0x10, &mut cartridge);
        ppu.cpu_write(0x2007, 0xAB, &mut cartridge);

        assert_eq!(cartridge.ppu_read(0x0010), Some(0xAB));
        assert_eq!(ppu.v, 0x0011);
    }

    #[test]
    fn ppudata_increments_by_32_when_ctrl_increment_bit_is_set() {
        let mut ppu = Ppu::new();
        let mut cartridge = cartridge_with_chr_ram();

        ppu.cpu_write(0x2000, 0x04, &mut cartridge);
        ppu.cpu_write(0x2006, 0x20, &mut cartridge);
        ppu.cpu_write(0x2006, 0x00, &mut cartridge);
        ppu.cpu_write(0x2007, 0xAB, &mut cartridge);

        assert_eq!(ppu.v, 0x2020);
    }

    #[test]
    fn ppudata_reads_chr_ram_through_read_buffer() {
        let mut ppu = Ppu::new();
        let mut cartridge = cartridge_with_chr_ram();
        cartridge.ppu_write(0x0000, 0x12);
        cartridge.ppu_write(0x0001, 0x34);

        ppu.cpu_write(0x2006, 0x00, &mut cartridge);
        ppu.cpu_write(0x2006, 0x00, &mut cartridge);

        assert_eq!(ppu.cpu_read(0x2007, &mut cartridge), 0x00);
        assert_eq!(ppu.cpu_read(0x2007, &mut cartridge), 0x12);
        assert_eq!(ppu.cpu_read(0x2007, &mut cartridge), 0x34);
    }

    #[test]
    fn palette_addresses_mirror_every_32_bytes_with_background_mirrors() {
        assert_eq!(palette_index(0x3F00), 0x00);
        assert_eq!(palette_index(0x3F10), 0x00);
        assert_eq!(palette_index(0x3F14), 0x04);
        assert_eq!(palette_index(0x3F1F), 0x1F);
        assert_eq!(palette_index(0x3F20), 0x00);
        assert_eq!(palette_index(0x3FFF), 0x1F);
    }
}
