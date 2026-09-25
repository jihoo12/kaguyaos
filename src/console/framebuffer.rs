use super::FramebufferInfo;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Color(pub u32);

impl Color {
    pub const BLACK: Self = Self(0x00000000);
    pub const WHITE: Self = Self(0xFFFFFFFF);
}

pub struct Framebuffer {
    base: *mut u32,
    stride: usize,
    width: usize,
    height: usize,
}

unsafe impl Send for Framebuffer {}

impl Framebuffer {
    pub fn from_info(info: FramebufferInfo) -> Self {
        Self {
            base: info.base,
            stride: info.stride,
            width: info.width,
            height: info.height,
        }
    }

    #[inline]
    pub fn width(&self) -> usize { self.width }

    #[inline]
    pub fn height(&self) -> usize { self.height }

    pub fn put_pixel(&mut self, x: usize, y: usize, color: Color) {
        if x >= self.width || y >= self.height {
            return;
        }
        unsafe { *self.base.add(y * self.stride + x) = color.0; }
    }

    pub fn fill_rect(&mut self, x: usize, y: usize, width: usize, height: usize, color: Color) {
        let x_end = x.saturating_add(width).min(self.width);
        let y_end = y.saturating_add(height).min(self.height);
        for py in y..y_end {
            let row = unsafe { self.base.add(py * self.stride) };
            for px in x..x_end {
                unsafe { *row.add(px) = color.0; }
            }
        }
    }

    pub fn clear(&mut self, color: Color) {
        self.fill_rect(0, 0, self.width, self.height, color);
    }
}
