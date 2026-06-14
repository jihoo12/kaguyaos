use font8x8::{BASIC_FONTS, UnicodeFonts};

use crate::interrupts::InterruptSpinlock;

const CELL_W: usize = 8;
const CELL_H: usize = 16; // matches your new_line() stride

pub static GLOBAL_CELL_RENDERER: InterruptSpinlock<Option<CellRenderer>> =
    InterruptSpinlock::new(None);

pub fn init() {
    let info = crate::writer::get_framebuffer_info()
        .expect("writer must be initialized before term_kernel");

    let mut renderer = GLOBAL_CELL_RENDERER.lock();
    *renderer = Some(CellRenderer {
        framebuffer: info.base,
        stride:      info.stride,
        cols:        info.width  / CELL_W,
        rows:        info.height / CELL_H,
    });
}

pub struct CellRenderer {
    framebuffer: *mut u32,
    stride:      usize,   // pixels_per_scanline
    cols:        usize,   // horizontal_resolution / CELL_W
    rows:        usize,   // vertical_resolution / CELL_H
}

unsafe impl Send for CellRenderer {}

impl CellRenderer {
    pub fn write_cell(&mut self, row: usize, col: usize, ch: char, fg: u32, bg: u32) {
        if row >= self.rows || col >= self.cols { return; }

        let bitmap = match BASIC_FONTS.get(ch) {
            Some(b) => b,
            None    => return,
        };

        let x_base = col * CELL_W;
        let y_base = row * CELL_H;

        for (dy, byte) in bitmap.iter().enumerate() {
            for dx in 0..CELL_W {
                let color = if byte >> dx & 1 == 1 { fg } else { bg };
                let offset = (y_base + dy) * self.stride + (x_base + dx);
                unsafe { *self.framebuffer.add(offset) = color; }
            }
        }
    }

    /// Write a flat buffer of (char, fg, bg) cells starting at (row, col),
    /// wrapping across `width` columns for `height` rows.
    pub fn write_region(
        &mut self,
        row: usize, col: usize,
        cells: &[(char, u32, u32)],
        width: usize,
    ) {
        for (i, &(ch, fg, bg)) in cells.iter().enumerate() {
            let r = row + i / width;
            let c = col + i % width;
            self.write_cell(r, c, ch, fg, bg);
        }
    }
}