use super::FramebufferInfo;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Color(pub u32);

impl Color {
    pub const BLACK: Self = Self(0x00000000);
    pub const WHITE: Self = Self(0xFFFFFFFF);
    pub const DESKTOP: Self = Self(0x002B2F36);
    pub const WINDOW: Self = Self(0x00E8E8E8);
    pub const TITLE_BAR: Self = Self(0x004B6EAF);
    pub const BORDER: Self = Self(0x001A1A1A);
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

pub fn draw_window_demo(info: FramebufferInfo) -> (usize, usize, usize, usize) {
    let mut fb = Framebuffer::from_info(info);
    fb.clear(Color::DESKTOP);

    let margin = 64usize;
    let width = fb.width().saturating_sub(margin * 2).min(720);
    let height = fb.height().saturating_sub(margin * 2).min(440);
    let x = (fb.width().saturating_sub(width)) / 2;
    let y = (fb.height().saturating_sub(height)) / 2;

    // Border, client area, and title bar. Keeping this primitive-only makes
    // the first visual milestone independent of font/window-manager policy.
    fb.fill_rect(x, y, width, height, Color::BORDER);
    if width > 4 && height > 4 {
        fb.fill_rect(x + 2, y + 2, width - 4, height - 4, Color::WINDOW);
    }
    if width > 4 && height > 34 {
        fb.fill_rect(x + 2, y + 2, width - 4, 32, Color::TITLE_BAR);
    }

    // The client area doubles as the first GUI terminal surface.
    let client_x = x + 8;
    let client_y = y + 42;
    let client_w = width.saturating_sub(16);
    let client_h = height.saturating_sub(50);
    fb.fill_rect(client_x, client_y, client_w, client_h, Color::BLACK);

    // Simple close-button placeholder.
    if width >= 48 {
        fb.fill_rect(x + width - 30, y + 10, 14, 14, Color::WHITE);
    }

    (client_x, client_y, client_w, client_h)
}
