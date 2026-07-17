pub const WIDTH: usize = 256;
pub const HEIGHT: usize = 240;

pub struct Frame {
    pixels: Vec<u8>,
}

impl Default for Frame {
    fn default() -> Self {
        Self::new()
    }
}

impl Frame {
    pub(crate) fn new() -> Self {
        Self {
            pixels: vec![0; WIDTH * HEIGHT * 3],
        }
    }

    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    pub(crate) fn set_pixel(&mut self, x: usize, y: usize, rgb: [u8; 3]) {
        if x >= WIDTH || y >= HEIGHT {
            return;
        }

        let offset = (y * WIDTH + x) * 3;
        self.pixels[offset..offset + 3].copy_from_slice(&rgb);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_pixel_writes_rgb_bytes() {
        let mut frame = Frame::new();
        frame.set_pixel(2, 3, [0x11, 0x22, 0x33]);
        let offset = (3 * WIDTH + 2) * 3;
        assert_eq!(&frame.pixels()[offset..offset + 3], &[0x11, 0x22, 0x33]);
    }

    #[test]
    fn set_pixel_ignores_out_of_bounds_coordinates() {
        let mut frame = Frame::new();
        frame.set_pixel(WIDTH, HEIGHT, [0xFF, 0xFF, 0xFF]);
        assert!(frame.pixels().iter().all(|byte| *byte == 0));
    }
}
