use super::BootInfo;
use core::fmt;
use core::fmt::Write;
use font8x8::{BASIC_FONTS, UnicodeFonts};

pub static GLOBAL_WRITER: crate::interrupts::InterruptSpinlock<Option<Writer>> = crate::interrupts::InterruptSpinlock::new(None);

pub unsafe fn init_global_writer(info: BootInfo) {
    let mut writer = GLOBAL_WRITER.lock();
    *writer = Some(Writer::new(info));
}
pub struct Writer {
    framebuffer: *mut u8,
    info: BootInfo,
    x_pos: usize,
    y_pos: usize,
}

pub struct FramebufferInfo {
    pub base: *mut u32,
    pub stride: usize,
    pub width: usize,
    pub height: usize,
}

pub fn get_framebuffer_info() -> Option<FramebufferInfo> {
    let writer = GLOBAL_WRITER.lock();
    writer.as_ref().map(|w| FramebufferInfo {
        base:   w.framebuffer as *mut u32,
        stride: w.info.pixels_per_scanline as usize,
        width:  w.info.horizontal_resolution as usize,
        height: w.info.vertical_resolution as usize,
    })
}

unsafe impl Send for Writer {}

impl Writer {
    pub fn new(info: BootInfo) -> self::Writer {
        Self {
            framebuffer: info.framebuffer_base as *mut u8,
            info,
            x_pos: 0,
            y_pos: 0,
        }
    }

    pub fn write_char(&mut self, c: char) {
        // Echo to serial port (COM1 0x3F8)
        unsafe {
            crate::io::outb(0x3F8, c as u8);
            if c == '\n' {
                crate::io::outb(0x3F8, b'\r');
            }
        }

        match c {
            '\n' => self.new_line(),
            '\x08' => {
                if self.x_pos >= 8 {
                    self.x_pos -= 8;
                }
            }
            c => {
                if self.x_pos >= self.info.horizontal_resolution as usize {
                    self.new_line();
                }
                if self.y_pos >= self.info.vertical_resolution as usize {
                    self.clear_screen(); // Simple scrolling: clear and reset. Better: scroll up.
                    self.y_pos = 0;
                }

                let bitmap = match BASIC_FONTS.get(c) {
                    Some(bitmap) => bitmap,
                    None => return, // Unknown char
                };

                self.write_rendered_char(bitmap);
                self.x_pos += 8;
            }
        }
    }

    fn write_rendered_char(&mut self, bitmap: [u8; 8]) {
        for (y, row) in bitmap.iter().enumerate() {
            for x in 0..8 {
                if match row {
                    &byte => byte >> x & 1 == 1,
                } {
                    self.write_pixel(self.x_pos + x, self.y_pos + y, 0xFFFFFFFF); // White
                } else {
                    self.write_pixel(self.x_pos + x, self.y_pos + y, 0x00000000); // Black background
                }
            }
        }
    }

    fn write_pixel(&mut self, x: usize, y: usize, color: u32) {
        if x >= self.info.horizontal_resolution as usize
            || y >= self.info.vertical_resolution as usize
        {
            return;
        }

        let pixel_offset = y * self.info.pixels_per_scanline as usize + x;
        // Assume 4 bytes per pixel (BGR or RGB Reserved) for typical UEFI GOP 32ppp
        // BootInfo.pixel_format should be checked, but we assume default for now.
        let ptr = self.framebuffer as *mut u32;
        unsafe {
            *ptr.add(pixel_offset) = color;
        }
    }

    fn new_line(&mut self) {
        self.x_pos = 0;
        self.y_pos += 16; // 8x8 font, strictly 8 high, but let's give some line spacing if we want. Let's stick to 8 or 10. 
        // font8x8 is 8x8.
        // Let's use 8 for now.
    }

    pub fn clear_screen(&mut self) {
        // Optimization: fill by 32 bits or 64 bits if possible, but per-pixel is fine for now
        // Or simpler: memset
        let size = self.info.framebuffer_size;
        let ptr = self.framebuffer;
        unsafe {
            core::ptr::write_bytes(ptr, 0, size);
        }
        self.x_pos = 0;
        self.y_pos = 0;
    }
}

pub fn clear() {
    let mut writer = GLOBAL_WRITER.lock();
    if let Some(w) = writer.as_mut() {
        w.clear_screen();
    }
}

impl fmt::Write for Writer {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for c in s.chars() {
            self.write_char(c);
        }
        Ok(())
    }
}

pub fn _print(args: fmt::Arguments) {
    let mut writer = GLOBAL_WRITER.lock();
    if let Some(w) = writer.as_mut() {
        w.write_fmt(args).unwrap();
    }
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ($crate::writer::_print(format_args!($($arg)*)));
}

#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($($arg:tt)*) => ($crate::print!("{}\n", format_args!($($arg)*)));
}
