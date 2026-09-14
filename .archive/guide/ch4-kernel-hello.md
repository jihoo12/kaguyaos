# Chapter 4: Kernel Hello World

Now we're bare-metal. No UEFI, no firmware text output. If we want to print anything, we need to write directly to the framebuffer. This chapter builds a `println!` macro that works in a `#![no_std]` kernel.

## The Framebuffer After ExitBootServices

The good news: the framebuffer memory is still mapped at the same physical address after `ExitBootServices()`. The `BootInfo` tells us exactly where it is and how it's organized.

kaguyaOS gets this from UEFI:
```rust
// src/main.rs:94-98
println!("Resolution: {}x{}",
    boot_info.horizontal_resolution, boot_info.vertical_resolution);
println!("Framebuffer: {:#x}", boot_info.framebuffer_base);
```

## Drawing Text: font8x8

To render characters, we need a font. [font8x8](https://docs.rs/font8x8) provides 8×8 bitmap glyphs for Unicode characters. Each glyph is an array of 8 bytes, where each bit represents a pixel.

```rust
use font8x8::{BASIC_FONTS, UnicodeFonts};

let bitmap = BASIC_FONTS.get('A').unwrap();
// bitmap is [u8; 8] — 8 rows, 8 pixels per row

for (y, row) in bitmap.iter().enumerate() {
    for x in 0..8 {
        if (row >> x) & 1 == 1 {
            // Draw white pixel at (x_pos + x, y_pos + y)
            write_pixel(x_pos + x, y_pos + y, 0xFFFFFFFF);
        } else {
            // Draw black pixel (background)
            write_pixel(x_pos + x, y_pos + y, 0x00000000);
        }
    }
}
```

## Building the Writer

kaguyaOS's `Writer` (in `src/console/mod.rs`) wraps all this into a Rust `fmt::Write` implementation:

```rust
// src/console/mod.rs (simplified)
pub struct Writer {
    framebuffer: *mut u8,
    horizontal_resolution: usize,
    vertical_resolution: usize,
    pixels_per_scanline: usize,
    x_pos: usize,
    y_pos: usize,
}

impl Writer {
    pub fn write_char(&mut self, c: char) {
        match c {
            '\n' => {
                self.x_pos = 0;
                self.y_pos += 8;  // Line height = 8 pixels
            }
            c => {
                if self.x_pos >= self.horizontal_resolution {
                    self.x_pos = 0;
                    self.y_pos += 8;
                }
                if self.y_pos >= self.vertical_resolution {
                    self.clear_screen();
                    self.y_pos = 0;
                }

                let bitmap = BASIC_FONTS.get(c).unwrap();
                for (dy, row) in bitmap.iter().enumerate() {
                    for x in 0..8 {
                        let color = if (row >> x) & 1 == 1 {
                            0xFFFFFFFF  // White
                        } else {
                            0x00000000  // Black
                        };
                        self.write_pixel(self.x_pos + x, self.y_pos + dy, color);
                    }
                }
                self.x_pos += 8;
            }
        }
    }

    fn write_pixel(&mut self, x: usize, y: usize, color: u32) {
        let offset = y * self.pixels_per_scanline + x;
        let ptr = self.framebuffer as *mut u32;
        unsafe {
            *ptr.add(offset) = color;
        }
    }
}
```

Note: pixels are written as `u32` values. UEFI GOP typically uses 32-bit BGRA format.

## Making It Global

To use `println!` anywhere in the kernel, we need a global `Writer`:

```rust
// src/console/mod.rs
pub static GLOBAL_WRITER: crate::sync::Spinlock<Option<Writer>> =
    crate::sync::Spinlock::new(None);

pub unsafe fn init_global_writer(info: BootInfo) {
    let mut writer = GLOBAL_WRITER.lock();
    *writer = Some(Writer::new(info));
}
```

Why `Option<Writer>`? Because the `Writer` can't be initialized at compile time (it needs runtime framebuffer addresses). We initialize it once during boot.

Why `Spinlock`? Because the timer interrupt (Chapter 6) can fire at any time, and the interrupt handler might want to print. Without a spinlock, the writer's cursor position would be corrupted.

## The println! Macro

Rust's `macro_rules!` lets us define `println!` like this:

```rust
// src/console/mod.rs
pub fn _print(args: fmt::Arguments) {
    let mut writer = GLOBAL_WRITER.lock();
    if let Some(w) = writer.as_mut() {
        w.write_fmt(args).unwrap();
    }
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ($crate::console::_print(format_args!($($arg)*)));
}

#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($($arg:tt)*) => ($crate::print!("{}\n", format_args!($($arg)*)));
}
```

**Important gotcha:** `#[macro_export]` places macros at the **crate root**, so `$crate::console::_print` is correct (not `$crate::writer::_print`).

## The `--oformat=binary` Gotcha

kaguyaOS compiles user programs with `--oformat=binary` (flat binary, no ELF headers). This means the compiler/linker doesn't process string literals normally — they can get corrupted when `print!` is inlined into loop bodies.

**The fix:** Mark print-related functions as `#[inline(never)]`:

```rust
#[inline(never)]
pub fn _print(args: fmt::Arguments) {
    // ...
}
```

Without this, string literal metadata (pointers, lengths) gets corrupted by the compiler's inlining decisions, producing garbage output.

## Writing to Serial Too

kaguyaOS's Writer also echoes every character to the COM1 serial port (0x3F8), which QEMU captures with `-serial stdio`:

```rust
// src/console/mod.rs (inside write_char)
unsafe {
    crate::io::outb(0x3F8, c as u8);
    if c == '\n' {
        crate::io::outb(0x3F8, b'\r');  // CR after LF
    }
}
```

This gives you dual output: framebuffer for the graphical display, serial for the terminal. Invaluable for debugging when the framebuffer doesn't work.

## Using It in kernel_main

The very first thing `kernel_main()` does is set up the writer:

```rust
// src/main.rs:87-90
unsafe {
    console::init_global_writer(*boot_info);
}
```

After this, `println!` works. The rest of the kernel init can print status messages.

## The I/O Module

The serial port uses x86 port I/O instructions (`in`/`out`). kaguyaOS wraps these in `src/io.rs`:

```rust
// src/io.rs
pub unsafe fn outb(port: u16, val: u8) {
    unsafe {
        core::arch::asm!("out dx, al", in(dx) port, in(al) val, options(nostack, preserves_flags));
    }
}

pub unsafe fn inb(port: u16) -> u8 {
    let val: u8;
    unsafe {
        core::arch::asm!("in al, dx", out(al) val, in(dx) port, options(nostack, preserves_flags));
    }
    val
}
```

## What You Should See

After this chapter, running your OS should produce:

**Serial output (QEMU terminal):**
```
Hello World from Kernel!
Resolution: 1280x800
Framebuffer: 0x80000000
```

**QEMU graphical window:**
White text on black background at the top-left corner of the screen.

## kaguyaOS Reference

| File | Lines | What it does |
|------|-------|-------------|
| `src/console/mod.rs` | 1-166 | Writer, framebuffer rendering, println! macro |
| `src/io.rs` | 1-46 | Port I/O helpers (outb, inb, outl, inl) |
| `src/main.rs` | 87-98 | Writer init and first println! calls |

---

**Next:** [Chapter 5 — GDT, TSS, and IDT](ch5-gdt-idt.md) — Setting up the CPU's protection and interrupt infrastructure.
