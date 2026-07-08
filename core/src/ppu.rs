use crate::{cartridge::Cartridge, frame::Frame, mapper::Mirroring};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Control(u8);

impl Control {
    fn nmi_enabled(self) -> bool {
        self.0 & 0x80 != 0
    }

    fn vram_increment(self) -> u16 {
        if self.0 & 0x04 != 0 { 32 } else { 1 }
    }

    fn background_pattern_base(self) -> u16 {
        if self.0 & 0x10 != 0 { 0x1000 } else { 0x0000 }
    }

    fn tall_sprite(self) -> bool {
        self.0 & 0x20 != 0
    }

    fn sprite_pattern_base(self) -> u16 {
        if self.0 & 0x08 != 0 { 0x1000 } else { 0x0000 }
    }

    fn nametable_scroll_bits(self) -> u16 {
        ((self.0 as u16) & 0x03) << 10
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Mask(u8);

impl Mask {
    fn show_background(self) -> bool {
        self.0 & 0x08 != 0
    }

    fn show_sprites(self) -> bool {
        self.0 & 0x10 != 0
    }

    fn show_background_left(self) -> bool {
        self.0 & 0x02 != 0
    }

    fn show_sprites_left(self) -> bool {
        self.0 & 0x04 != 0
    }

    fn rendering_enabled(self) -> bool {
        self.0 & 0x18 != 0
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Status(u8);

impl Status {
    fn bits(self) -> u8 {
        self.0
    }

    fn set_vblank(&mut self) {
        self.0 |= 0x80;
    }

    fn clear_vblank(&mut self) {
        self.0 &= !0x80;
    }

    fn clear_render_flags(&mut self) {
        self.0 &= !0xE0;
    }

    fn set_sprite_zero_hit(&mut self) {
        self.0 |= 0x40;
    }
}

#[derive(Debug, Clone, Default)]
struct BackgroundPipeline {
    next_tile_id: u8,
    next_palette_id: u8,
    next_pattern_lo: u8,
    next_pattern_hi: u8,

    pattern_shift_lo: u16,
    pattern_shift_hi: u16,
    attr_shift_lo: u16,
    attr_shift_hi: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BgPixel {
    palette_id: u8,
    color_low_bits: u8,
}

impl BgPixel {
    fn transparent(self) -> bool {
        self.color_low_bits == 0
    }

    fn palette_addr(self) -> u16 {
        if self.transparent() {
            0x3F00
        } else {
            0x3F00 + (self.palette_id as u16) * 4 + self.color_low_bits as u16
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SpritePixel {
    palette_id: u8,
    color_low_bits: u8,
    behind_background: bool,
    sprite_zero: bool,
}

impl SpritePixel {
    fn transparent(self) -> bool {
        self.color_low_bits == 0
    }

    fn palette_addr(self) -> u16 {
        0x3F10 + (self.palette_id as u16) * 4 + self.color_low_bits as u16
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct SpriteSlot {
    x: u8,
    attr: u8,
    pattern_lo: u8,
    pattern_hi: u8,
    oam_index: u8,
}

pub(crate) struct Ppu {
    /// $2000 PPUCTRL
    ctrl: Control,
    /// $2001 PPUMASK
    mask: Mask,
    /// $2002 PPUSTATUS
    status: Status,
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
    vram: [u8; 0x1000],
    /// Internal palette RAM
    palette: [u8; 0x20],
    /// Cycle counter
    cycle: usize,
    /// Scanline counter
    scanline: usize,
    /// Non-maskable interrupt pending flag
    nmi_pending: bool,
    /// Background rendering state
    bg: BackgroundPipeline,
    /// Scanline sprites
    sprites: [Option<SpriteSlot>; 8],
    /// Whether the PPU is rendering an even or odd frame
    odd_frame: bool,
    /// Frame buffer
    frame: Frame,
}

const HORIZONTAL_SCROLL_BITS: u16 = 0x041F;
const VERTICAL_SCROLL_BITS: u16 = 0x7BE0;

impl Ppu {
    pub(crate) fn new() -> Self {
        Self {
            ctrl: Control(0),
            mask: Mask(0),
            status: Status(0),
            oam_addr: 0,
            oam: [0xFF; 0x100],
            read_buffer: 0,
            v: 0,
            t: 0,
            x: 0,
            w: false,
            vram: [0; 0x1000],
            palette: [0; 0x20],
            cycle: 0,
            scanline: 0,
            nmi_pending: false,
            bg: Default::default(),
            sprites: [None; 8],
            odd_frame: false,
            frame: Frame::new(),
        }
    }

    pub(crate) fn tick(&mut self, cartridge: &mut Cartridge) -> bool {
        if self.mask.rendering_enabled() && self.is_rendering_scanline() {
            if self.should_shift_background_pipeline() {
                self.shift_background_pipeline();
            }

            if self.is_background_fetch_cycle() {
                match (self.cycle - 1) % 8 {
                    0 => {
                        self.load_background_shifters();
                        self.bg.next_tile_id = self.ppu_bus_read(nametable_addr(self.v), cartridge);
                    }
                    2 => {
                        let attr = self.ppu_bus_read(attribute_addr(self.v), cartridge);
                        self.bg.next_palette_id = attribute_palette_bits(self.v, attr);
                    }
                    4 => self.bg.next_pattern_lo = self.fetch_bg_pattern_byte(0, cartridge),
                    6 => self.bg.next_pattern_hi = self.fetch_bg_pattern_byte(8, cartridge),
                    7 => self.increment_coarse_x(),
                    _ => (),
                }
            }

            if self.cycle == 256 {
                self.increment_y();
            }

            if self.cycle == 257 {
                self.load_background_shifters();
                self.copy_horizontal_scroll_bits();

                if self.scanline < 239 {
                    self.evaluate_sprites_for_scanline(self.scanline + 1, cartridge);
                } else {
                    self.sprites = [None; 8];
                }
            }

            if self.scanline == 261 && (280..=304).contains(&self.cycle) {
                self.copy_vertical_scroll_bits();
            }
        }

        if self.scanline < 240 && (1..=256).contains(&self.cycle) {
            self.render_pixel_from_pipeline(self.cycle - 1, self.scanline, cartridge);
        }

        if self.scanline == 241 && self.cycle == 1 {
            self.status.set_vblank();

            if self.ctrl.nmi_enabled() {
                self.nmi_pending = true;
            }
        }

        if self.scanline == 261 && self.cycle == 1 {
            self.status.clear_render_flags();
        }

        if self.scanline == 261
            && self.cycle == 339
            && self.odd_frame
            && self.mask.rendering_enabled()
        {
            self.cycle = 0;
            self.scanline = 0;
            self.odd_frame = false;
            return true;
        }

        self.cycle += 1;

        if self.cycle == 341 {
            self.cycle = 0;
            self.scanline += 1;

            if self.scanline == 262 {
                self.scanline = 0;
                self.odd_frame = !self.odd_frame;
                return true;
            }
        }

        false
    }

    pub(crate) fn frame(&self) -> &Frame {
        &self.frame
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
            0x2001 => self.mask = Mask(value),
            0x2003 => self.oam_addr = value,
            0x2004 => self.write_oam_data(value),
            0x2005 => self.write_scroll(value),
            0x2006 => self.write_addr(value),
            0x2007 => self.write_data(value, cartridge),
            _ => {}
        }
    }

    fn evaluate_sprites_for_scanline(&mut self, scanline: usize, cartridge: &mut Cartridge) {
        self.sprites = [None; 8];

        let sprite_height = if self.ctrl.0 & 0x20 != 0 { 16 } else { 8 };
        let mut slot = 0;

        for oam_index in 0..64 {
            let base = oam_index * 4;
            let sprite_y = self.oam[base] as i16 + 1;
            let row = scanline as i16 - sprite_y;

            if row < 0 || row >= sprite_height {
                continue;
            }

            if slot == 8 {
                self.status.0 |= 0x20; // sprite overflow, approximate for now
                break;
            }

            let tile_id = self.oam[base + 1];
            let attr = self.oam[base + 2];
            let x = self.oam[base + 3];
            let (pattern_lo, pattern_hi) =
                self.fetch_sprite_pattern_row(tile_id, attr, row as usize, cartridge);

            self.sprites[slot] = Some(SpriteSlot {
                x,
                attr,
                pattern_lo,
                pattern_hi,
                oam_index: oam_index as u8,
            });

            slot += 1;
        }
    }

    fn is_rendering_scanline(&self) -> bool {
        self.scanline < 240 || self.scanline == 261
    }

    fn is_background_fetch_cycle(&self) -> bool {
        (1..=256).contains(&self.cycle) || (321..=336).contains(&self.cycle)
    }

    fn should_shift_background_pipeline(&self) -> bool {
        (2..=257).contains(&self.cycle) || (322..=337).contains(&self.cycle)
    }

    fn fetch_bg_pattern_byte(&mut self, plane_offset: u16, cartridge: &mut Cartridge) -> u8 {
        let tile_id = self.bg.next_tile_id as u16;
        let fine_y = (self.v >> 12) & 0x07;
        let addr = self.ctrl.background_pattern_base() + tile_id * 16 + fine_y + plane_offset;

        self.ppu_bus_read(addr, cartridge)
    }

    fn load_background_shifters(&mut self) {
        self.bg.pattern_shift_lo =
            (self.bg.pattern_shift_lo & 0xFF00) | self.bg.next_pattern_lo as u16;
        self.bg.pattern_shift_hi =
            (self.bg.pattern_shift_hi & 0xFF00) | self.bg.next_pattern_hi as u16;

        let attr_lo = self.bg.next_palette_id & 0x01;
        let attr_hi = (self.bg.next_palette_id >> 1) & 0x01;

        self.bg.attr_shift_lo =
            (self.bg.attr_shift_lo & 0xFF00) | if attr_lo != 0 { 0x00FF } else { 0x0000 };
        self.bg.attr_shift_hi =
            (self.bg.attr_shift_hi & 0xFF00) | if attr_hi != 0 { 0x00FF } else { 0x0000 };
    }

    fn shift_background_pipeline(&mut self) {
        self.bg.pattern_shift_lo <<= 1;
        self.bg.pattern_shift_hi <<= 1;
        self.bg.attr_shift_lo <<= 1;
        self.bg.attr_shift_hi <<= 1;
    }

    fn background_pixel(&self) -> BgPixel {
        let bit = 0x8000 >> self.x;

        let pattern_lo = if self.bg.pattern_shift_lo & bit != 0 {
            1
        } else {
            0
        };

        let pattern_hi = if self.bg.pattern_shift_hi & bit != 0 {
            1
        } else {
            0
        };

        let attr_lo = if self.bg.attr_shift_lo & bit != 0 {
            1
        } else {
            0
        };

        let attr_hi = if self.bg.attr_shift_hi & bit != 0 {
            1
        } else {
            0
        };

        BgPixel {
            palette_id: (attr_hi << 1) | attr_lo,
            color_low_bits: (pattern_hi << 1) | pattern_lo,
        }
    }

    fn render_pixel_from_pipeline(&mut self, x: usize, y: usize, cartridge: &mut Cartridge) {
        let bg = if !self.mask.show_background() || (x < 8 && !self.mask.show_background_left()) {
            BgPixel {
                palette_id: 0,
                color_low_bits: 0,
            }
        } else {
            self.background_pixel()
        };

        let sprite = self.sprite_pixel_for_x(x);

        if x != 255
            && let Some(sprite) = sprite
            && sprite.sprite_zero
            && !sprite.transparent()
            && !bg.transparent()
        {
            self.status.set_sprite_zero_hit();
        }

        let palette_addr = final_palette_addr(bg, sprite);
        let rgb = self.palette_rgb(palette_addr, cartridge);
        self.frame.set_pixel(x, y, rgb);
    }

    fn sprite_pixel_for_x(&self, x: usize) -> Option<SpritePixel> {
        if !self.mask.show_sprites() || (x < 8 && !self.mask.show_sprites_left()) {
            return None;
        }

        for slot in self.sprites.iter().flatten() {
            let sprite_x = slot.x as usize;
            if x < sprite_x || x >= sprite_x + 8 {
                continue;
            }

            let col = x - sprite_x;
            let bit = if slot.attr & 0x40 != 0 { col } else { 7 - col };

            let lo = (slot.pattern_lo >> bit) & 1;
            let hi = (slot.pattern_hi >> bit) & 1;
            let color_low_bits = (hi << 1) | lo;

            if color_low_bits == 0 {
                continue;
            }

            return Some(SpritePixel {
                palette_id: slot.attr & 0x03,
                color_low_bits,
                behind_background: slot.attr & 0x20 != 0,
                sprite_zero: slot.oam_index == 0,
            });
        }

        None
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
            Mirroring::FourScreen => offset as usize,
        }
    }

    fn read_status(&mut self) -> u8 {
        let value = self.status;
        self.status.clear_vblank();
        self.w = false; // reset write latch
        value.0
    }

    fn write_ctrl(&mut self, value: u8) {
        let old_ctrl = self.ctrl;
        self.ctrl = Control(value);
        self.t = (self.t & !0x0C00) | self.ctrl.nametable_scroll_bits();
        if !old_ctrl.nmi_enabled() && self.ctrl.nmi_enabled() && self.status.bits() & 0x80 != 0 {
            self.nmi_pending = true;
        }
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
        let increment = self.ctrl.vram_increment();
        self.v = self.v.wrapping_add(increment) & 0x7FFF;
    }

    pub(crate) fn write_oam_dma_byte(&mut self, value: u8) {
        self.oam[self.oam_addr as usize] = value;
        self.oam_addr = self.oam_addr.wrapping_add(1);
    }

    pub(crate) fn take_nmi(&mut self) -> bool {
        let pending = self.nmi_pending;
        self.nmi_pending = false;
        pending
    }

    fn palette_rgb(&mut self, palette_addr: u16, cartridge: &mut Cartridge) -> [u8; 3] {
        let nes_color = self.ppu_bus_read(palette_addr, cartridge) & 0x3F;
        SYSTEM_PALETTE[nes_color as usize]
    }

    fn increment_coarse_x(&mut self) {
        if self.v & 0x001F == 31 {
            self.v &= !0x001F;
            self.v ^= 0x0400;
        } else {
            self.v += 1;
        }
    }

    fn increment_y(&mut self) {
        if self.v & 0x7000 != 0x7000 {
            self.v += 0x1000;
        } else {
            self.v &= !0x7000;

            let mut y = (self.v & 0x03E0) >> 5;
            if y == 29 {
                y = 0;
                self.v ^= 0x0800;
            } else if y == 31 {
                y = 0;
            } else {
                y += 1;
            }

            self.v = (self.v & !0x03E0) | (y << 5);
        }
    }

    fn copy_horizontal_scroll_bits(&mut self) {
        self.v = (self.v & !HORIZONTAL_SCROLL_BITS) | (self.t & HORIZONTAL_SCROLL_BITS);
    }

    fn copy_vertical_scroll_bits(&mut self) {
        self.v = (self.v & !VERTICAL_SCROLL_BITS) | (self.t & VERTICAL_SCROLL_BITS);
    }

    fn fetch_sprite_pattern_row(
        &mut self,
        tile_id: u8,
        attr: u8,
        row: usize,
        cartridge: &mut Cartridge,
    ) -> (u8, u8) {
        let flip_v = attr & 0x80 != 0;

        if !self.ctrl.tall_sprite() {
            let source_row = if flip_v { 7 - row } else { row };
            let tile_addr =
                self.ctrl.sprite_pattern_base() + tile_id as u16 * 16 + source_row as u16;
            let pattern_lo = self.ppu_bus_read(tile_addr, cartridge);
            let pattern_hi = self.ppu_bus_read(tile_addr + 8, cartridge);

            return (pattern_lo, pattern_hi);
        }

        let source_row = if flip_v { 15 - row } else { row };
        let pattern_base = if tile_id & 0x01 == 0 { 0x0000 } else { 0x1000 };

        let top_tile = (tile_id & 0xFE) as u16;
        let tile_offset = if source_row < 8 { 0 } else { 1 };
        let row_in_tile = source_row % 8;

        let tile_addr = pattern_base + (top_tile + tile_offset) * 16 + row_in_tile as u16;

        let pattern_lo = self.ppu_bus_read(tile_addr, cartridge);
        let pattern_hi = self.ppu_bus_read(tile_addr + 8, cartridge);

        (pattern_lo, pattern_hi)
    }
}

fn nametable_addr(v: u16) -> u16 {
    let coarse_x = v & 0x001F;
    let coarse_y = (v >> 5) & 0x001F;
    let nametable = (v >> 10) & 0x0003;

    0x2000 + nametable * 0x0400 + coarse_y * 32 + coarse_x
}

fn attribute_addr(v: u16) -> u16 {
    let coarse_x = v & 0x001F;
    let coarse_y = (v >> 5) & 0x001F;
    let nametable = (v >> 10) & 0x0003;

    0x2000 + nametable * 0x0400 + 0x03C0 + (coarse_y / 4) * 8 + (coarse_x / 4)
}

fn attribute_palette_bits(v: u16, attr: u8) -> u8 {
    let coarse_x = v & 0x001F;
    let coarse_y = (v >> 5) & 0x001F;

    let quadrant_x = (coarse_x % 4) / 2;
    let quadrant_y = (coarse_y % 4) / 2;
    let shift = (quadrant_y * 2 + quadrant_x) * 2;

    (attr >> shift) & 0x03
}

fn palette_index(addr: u16) -> usize {
    let mut index = (addr - 0x3F00) & 0x001F;

    if matches!(index, 0x10 | 0x14 | 0x18 | 0x1C) {
        index -= 0x10;
    }

    index as usize
}

fn final_palette_addr(bg: BgPixel, sprite: Option<SpritePixel>) -> u16 {
    match sprite {
        None => bg.palette_addr(),
        Some(sprite) if sprite.transparent() => bg.palette_addr(),
        Some(sprite) if bg.transparent() => sprite.palette_addr(),
        Some(sprite) if sprite.behind_background => bg.palette_addr(),
        Some(sprite) => sprite.palette_addr(),
    }
}

#[rustfmt::skip]
const SYSTEM_PALETTE: [[u8; 3]; 64] = [
    [0x66, 0x66, 0x66], [0x00, 0x2A, 0x88], [0x14, 0x12, 0xA7], [0x3B, 0x00, 0xA4],
    [0x5C, 0x00, 0x7E], [0x6E, 0x00, 0x40], [0x6C, 0x06, 0x00], [0x56, 0x1D, 0x00],
    [0x33, 0x35, 0x00], [0x0B, 0x48, 0x00], [0x00, 0x52, 0x00], [0x00, 0x4F, 0x08],
    [0x00, 0x40, 0x4D], [0x00, 0x00, 0x00], [0x00, 0x00, 0x00], [0x00, 0x00, 0x00],

    [0xAD, 0xAD, 0xAD], [0x15, 0x5F, 0xD9], [0x42, 0x40, 0xFF], [0x75, 0x27, 0xFE],
    [0xA0, 0x1A, 0xCC], [0xB7, 0x1E, 0x7B], [0xB5, 0x31, 0x20], [0x99, 0x4E, 0x00],
    [0x6B, 0x6D, 0x00], [0x38, 0x87, 0x00], [0x0C, 0x93, 0x00], [0x00, 0x8F, 0x32],
    [0x00, 0x7C, 0x8D], [0x00, 0x00, 0x00], [0x00, 0x00, 0x00], [0x00, 0x00, 0x00],

    [0xFF, 0xFE, 0xFF], [0x64, 0xB0, 0xFF], [0x92, 0x90, 0xFF], [0xC6, 0x76, 0xFF],
    [0xF3, 0x6A, 0xFF], [0xFE, 0x6E, 0xCC], [0xFE, 0x81, 0x70], [0xEA, 0x9E, 0x22],
    [0xBC, 0xBE, 0x00], [0x88, 0xD8, 0x00], [0x5C, 0xE4, 0x30], [0x45, 0xE0, 0x82],
    [0x48, 0xCD, 0xDE], [0x4F, 0x4F, 0x4F], [0x00, 0x00, 0x00], [0x00, 0x00, 0x00],

    [0xFF, 0xFE, 0xFF], [0xC0, 0xDF, 0xFF], [0xD3, 0xD2, 0xFF], [0xE8, 0xC8, 0xFF],
    [0xFB, 0xC2, 0xFF], [0xFE, 0xC4, 0xEA], [0xFE, 0xCC, 0xC5], [0xF7, 0xD8, 0xA5],
    [0xE4, 0xE5, 0x94], [0xCF, 0xEF, 0x96], [0xBD, 0xF4, 0xAB], [0xB3, 0xF3, 0xCC],
    [0xB5, 0xEB, 0xF2], [0xB8, 0xB8, 0xB8], [0x00, 0x00, 0x00], [0x00, 0x00, 0x00],
];

#[cfg(test)]
mod tests {
    use super::*;

    fn scroll_v(nametable: u16, coarse_y: u16, coarse_x: u16, fine_y: u16) -> u16 {
        (fine_y << 12) | (nametable << 10) | (coarse_y << 5) | coarse_x
    }

    fn cartridge_with_chr_ram() -> Cartridge {
        let mut rom = vec![0; 16];
        rom[0..4].copy_from_slice(b"NES\x1A");
        rom[4] = 1;
        rom[5] = 0;
        rom.extend_from_slice(&vec![0; 0x4000]);

        Cartridge::from_ines(&rom).unwrap()
    }

    fn ppu_with_visible_sprite_zero() -> (Ppu, Cartridge) {
        let mut ppu = Ppu::new();
        let cartridge = cartridge_with_chr_ram();

        ppu.mask = Mask(0x18); // show background + show sprites
        ppu.x = 0;

        ppu.bg.pattern_shift_lo = 0x8000;
        ppu.bg.pattern_shift_hi = 0x0000;

        ppu.sprites[0] = Some(SpriteSlot {
            x: 10,
            attr: 0,
            pattern_lo: 0x80,
            pattern_hi: 0,
            oam_index: 0,
        });

        (ppu, cartridge)
    }

    #[test]
    fn write_ctrl_updates_nametable_bits_without_destroying_other_t_bits() {
        let mut ppu = Ppu::new();
        ppu.t = 0x73EF;

        ppu.write_ctrl(0x02);

        assert_eq!(ppu.ctrl, Control(0x02));
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
        ppu.status = Status(0xE0);
        ppu.w = true;

        assert_eq!(ppu.read_status(), 0xE0);
        assert_eq!(ppu.status, Status(0x60));
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

    #[test]
    fn nametable_addr_uses_nametable_and_coarse_scroll_bits() {
        let v = scroll_v(2, 11, 18, 5);

        assert_eq!(nametable_addr(v), 0x2000 + 2 * 0x0400 + 11 * 32 + 18);
        assert_eq!(nametable_addr(scroll_v(3, 31, 31, 0)), 0x2FFF);
        assert_eq!(nametable_addr(scroll_v(3, 31, 31, 7)), 0x2FFF);
    }

    #[test]
    fn nametable_index_maps_four_screen_tables_directly() {
        let ppu = Ppu::new();

        assert_eq!(ppu.nametable_index(0x2000, Mirroring::FourScreen), 0x0000);
        assert_eq!(ppu.nametable_index(0x23FF, Mirroring::FourScreen), 0x03FF);
        assert_eq!(ppu.nametable_index(0x2400, Mirroring::FourScreen), 0x0400);
        assert_eq!(ppu.nametable_index(0x27FF, Mirroring::FourScreen), 0x07FF);
        assert_eq!(ppu.nametable_index(0x2800, Mirroring::FourScreen), 0x0800);
        assert_eq!(ppu.nametable_index(0x2BFF, Mirroring::FourScreen), 0x0BFF);
        assert_eq!(ppu.nametable_index(0x2C00, Mirroring::FourScreen), 0x0C00);
        assert_eq!(ppu.nametable_index(0x2FFF, Mirroring::FourScreen), 0x0FFF);
    }

    #[test]
    fn nametable_index_mirrors_3000_range_to_2000_range() {
        let ppu = Ppu::new();

        assert_eq!(ppu.nametable_index(0x3000, Mirroring::FourScreen), 0x0000);
        assert_eq!(ppu.nametable_index(0x33FF, Mirroring::FourScreen), 0x03FF);
    }

    #[test]
    fn attribute_addr_uses_one_byte_per_four_by_four_tile_block() {
        assert_eq!(
            attribute_addr(scroll_v(1, 9, 14, 6)),
            0x2000 + 0x0400 + 0x03C0 + 2 * 8 + 3
        );

        assert_eq!(attribute_addr(scroll_v(1, 8, 12, 0)), 0x27D3);
        assert_eq!(attribute_addr(scroll_v(1, 11, 15, 7)), 0x27D3);
    }

    #[test]
    fn attribute_palette_bits_selects_the_two_bit_quadrant() {
        let attr = 0b11_10_01_00;

        assert_eq!(attribute_palette_bits(scroll_v(0, 0, 0, 0), attr), 0);
        assert_eq!(attribute_palette_bits(scroll_v(0, 0, 2, 0), attr), 1);
        assert_eq!(attribute_palette_bits(scroll_v(0, 2, 0, 0), attr), 2);
        assert_eq!(attribute_palette_bits(scroll_v(0, 2, 2, 0), attr), 3);
    }

    #[test]
    fn increment_coarse_x_advances_within_current_nametable() {
        let mut ppu = Ppu::new();
        ppu.v = scroll_v(1, 7, 14, 2);

        ppu.increment_coarse_x();

        assert_eq!(ppu.v, scroll_v(1, 7, 15, 2));
    }

    #[test]
    fn increment_coarse_x_wraps_and_switches_horizontal_nametable() {
        let mut ppu = Ppu::new();
        ppu.v = scroll_v(0, 4, 31, 3);

        ppu.increment_coarse_x();

        assert_eq!(ppu.v, scroll_v(1, 4, 0, 3));
    }

    #[test]
    fn increment_y_advances_fine_y_before_touching_coarse_y() {
        let mut ppu = Ppu::new();
        ppu.v = scroll_v(2, 12, 7, 3);

        ppu.increment_y();

        assert_eq!(ppu.v, scroll_v(2, 12, 7, 4));
    }

    #[test]
    fn increment_y_wraps_fine_y_and_advances_coarse_y() {
        let mut ppu = Ppu::new();
        ppu.v = scroll_v(2, 12, 7, 7);

        ppu.increment_y();

        assert_eq!(ppu.v, scroll_v(2, 13, 7, 0));
    }

    #[test]
    fn increment_y_wraps_coarse_y_29_and_switches_vertical_nametable() {
        let mut ppu = Ppu::new();
        ppu.v = scroll_v(0, 29, 7, 7);

        ppu.increment_y();

        assert_eq!(ppu.v, scroll_v(2, 0, 7, 0));
    }

    #[test]
    fn increment_y_wraps_coarse_y_31_without_switching_vertical_nametable() {
        let mut ppu = Ppu::new();
        ppu.v = scroll_v(2, 31, 7, 7);

        ppu.increment_y();

        assert_eq!(ppu.v, scroll_v(2, 0, 7, 0));
    }

    #[test]
    fn background_pipeline_shifts_on_rendering_shift_dots() {
        let shift_cycles = [2, 8, 9, 257, 322, 329, 337];
        let no_shift_cycles = [0, 1, 258, 320, 321, 338, 340];

        for cycle in shift_cycles {
            let mut ppu = Ppu::new();
            ppu.cycle = cycle;
            assert!(ppu.should_shift_background_pipeline(), "cycle {cycle}");
        }

        for cycle in no_shift_cycles {
            let mut ppu = Ppu::new();
            ppu.cycle = cycle;
            assert!(!ppu.should_shift_background_pipeline(), "cycle {cycle}");
        }
    }

    #[test]
    fn load_background_shifters_loads_next_tile_into_low_bytes() {
        let mut ppu = Ppu::new();
        ppu.bg.pattern_shift_lo = 0x12AB;
        ppu.bg.pattern_shift_hi = 0x34CD;
        ppu.bg.attr_shift_lo = 0x5601;
        ppu.bg.attr_shift_hi = 0x7802;
        ppu.bg.next_pattern_lo = 0xA5;
        ppu.bg.next_pattern_hi = 0x3C;
        ppu.bg.next_palette_id = 0b10;

        ppu.load_background_shifters();

        assert_eq!(ppu.bg.pattern_shift_lo, 0x12A5);
        assert_eq!(ppu.bg.pattern_shift_hi, 0x343C);
        assert_eq!(ppu.bg.attr_shift_lo, 0x5600);
        assert_eq!(ppu.bg.attr_shift_hi, 0x78FF);
    }

    #[test]
    fn sprite_zero_hit_is_set_when_sprite_zero_overlaps_opaque_background() {
        let (mut ppu, mut cartridge) = ppu_with_visible_sprite_zero();

        ppu.render_pixel_from_pipeline(10, 0, &mut cartridge);

        assert_eq!(ppu.status.bits() & 0x40, 0x40);
    }

    #[test]
    fn sprite_zero_hit_ignores_sprite_priority() {
        let (mut ppu, mut cartridge) = ppu_with_visible_sprite_zero();
        ppu.sprites[0].as_mut().unwrap().attr = 0x20;

        ppu.render_pixel_from_pipeline(10, 0, &mut cartridge);

        assert_eq!(ppu.status.bits() & 0x40, 0x40);
    }

    #[test]
    fn sprite_zero_hit_is_not_set_when_background_pixel_is_transparent() {
        let (mut ppu, mut cartridge) = ppu_with_visible_sprite_zero();
        ppu.bg.pattern_shift_lo = 0;
        ppu.bg.pattern_shift_hi = 0;

        ppu.render_pixel_from_pipeline(10, 0, &mut cartridge);

        assert_eq!(ppu.status.bits() & 0x40, 0);
    }

    #[test]
    fn sprite_zero_hit_is_not_set_when_sprite_pixel_is_transparent() {
        let (mut ppu, mut cartridge) = ppu_with_visible_sprite_zero();
        ppu.sprites[0].as_mut().unwrap().pattern_lo = 0;
        ppu.sprites[0].as_mut().unwrap().pattern_hi = 0;

        ppu.render_pixel_from_pipeline(10, 0, &mut cartridge);

        assert_eq!(ppu.status.bits() & 0x40, 0);
    }

    #[test]
    fn sprite_zero_hit_is_not_set_for_nonzero_sprite() {
        let (mut ppu, mut cartridge) = ppu_with_visible_sprite_zero();
        ppu.sprites[0].as_mut().unwrap().oam_index = 1;

        ppu.render_pixel_from_pipeline(10, 0, &mut cartridge);

        assert_eq!(ppu.status.bits() & 0x40, 0);
    }

    #[test]
    fn sprite_zero_hit_is_not_set_at_x_255() {
        let (mut ppu, mut cartridge) = ppu_with_visible_sprite_zero();
        ppu.sprites[0].as_mut().unwrap().x = 255;

        ppu.render_pixel_from_pipeline(255, 0, &mut cartridge);

        assert_eq!(ppu.status.bits() & 0x40, 0);
    }
}
