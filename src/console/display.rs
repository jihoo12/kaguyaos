use super::FramebufferInfo;
use crate::{
    BootInfo,
    drivers::{pci, virtio_gpu::Gpu},
    memory,
    sync::Spinlock,
};
use core::ptr::{addr_of_mut, read_volatile, write_volatile};

const MAX_PIXELS: usize = 1920 * 1080;
#[repr(C, align(4096))]
struct Pixels([u32; MAX_PIXELS]);
static mut PIXELS: Pixels = Pixels([0; MAX_PIXELS]);
pub(super) static DISPLAY: Spinlock<Option<Display>> = Spinlock::new(None);

pub(super) struct Display {
    buffer: usize,
    gop: usize,
    gop_stride: usize,
    pixel_format: u32,
    width: usize,
    height: usize,
    gpu: Option<Gpu>,
    dirty: Option<(usize, usize, usize, usize)>,
    failed: bool,
}

pub fn init(info: &BootInfo, allocator: &mut memory::FrameAllocator) -> Option<FramebufferInfo> {
    let width = info.horizontal_resolution as usize;
    let height = info.vertical_resolution as usize;
    if width == 0
        || height == 0
        || width.checked_mul(height)? > MAX_PIXELS
        || info.pixels_per_scanline < info.horizontal_resolution
        || info.pixel_format > 1
    {
        super::serial(format_args!(
            "display: unsupported GOP layout; keeping boot console\n"
        ));
        return None;
    }
    let base = addr_of_mut!(PIXELS) as *mut u32;
    let mut display = Display {
        buffer: base as usize,
        gop: info.framebuffer_base as usize,
        gop_stride: info.pixels_per_scanline as usize,
        pixel_format: info.pixel_format,
        width,
        height,
        gpu: None,
        dirty: Some((0, 0, width, height)),
        failed: false,
    };
    unsafe {
        for y in 0..height {
            for x in 0..width {
                let pixel =
                    read_volatile((display.gop as *const u32).add(y * display.gop_stride + x));
                *base.add(y * width + x) = convert(pixel, display.pixel_format);
            }
        }
    }
    if let Some(dev) = pci::get_gpu_device() {
        match unsafe { Gpu::init(dev, allocator, base as u64, width as u32, height as u32) } {
            Ok(gpu) => display.gpu = Some(gpu),
            Err(error) => super::serial(format_args!(
                "virtio-gpu: {}; retaining GOP console\n",
                error
            )),
        }
    } else {
        super::serial(format_args!("display: GOP backend\n"));
    }
    *DISPLAY.lock() = Some(display);
    Some(FramebufferInfo {
        base,
        stride: width,
        width,
        height,
    })
}

fn convert(pixel: u32, format: u32) -> u32 {
    if format == 0 {
        (pixel & 0xff00ff00) | ((pixel & 0xff) << 16) | ((pixel >> 16) & 0xff)
    } else {
        pixel
    }
}

pub fn present() {
    let mut state = DISPLAY.lock();
    let Some(display) = state.as_mut() else {
        return;
    };
    if display.failed {
        return;
    }
    let Some((x0, y0, x1, y1)) = display.dirty.take() else {
        return;
    };
    if let Some(gpu) = display.gpu.as_mut() {
        if let Err(error) = gpu.present() {
            display.failed = true;
            super::serial(format_args!(
                "virtio-gpu: {}; screen updates stopped\n",
                error
            ));
        }
    } else {
        unsafe {
            let base = display.buffer as *const u32;
            for y in y0..y1 {
                for x in x0..x1 {
                    write_volatile(
                        (display.gop as *mut u32).add(y * display.gop_stride + x),
                        convert(*base.add(y * display.width + x), display.pixel_format),
                    );
                }
            }
        }
    }
}

impl Display {
    pub fn mark(&mut self, x: usize, y: usize, width: usize, height: usize) {
        let right = x.saturating_add(width).min(self.width);
        let bottom = y.saturating_add(height).min(self.height);
        if x >= right || y >= bottom {
            return;
        }
        self.dirty = Some(match self.dirty {
            Some((x0, y0, x1, y1)) => (x.min(x0), y.min(y0), right.max(x1), bottom.max(y1)),
            None => (x, y, right, bottom),
        });
    }
}

/// Dedicated GOP surface with individually allocated frames; no PCI probing.
/// Reserved for the kernel lifetime, at a kernel-only virtual address.
pub fn init_gop(
    info: &BootInfo,
    allocator: &mut memory::FrameAllocator,
) -> Option<FramebufferInfo> {
    let width = info.horizontal_resolution as usize;
    let height = info.vertical_resolution as usize;
    let bytes = width.checked_mul(height)?.checked_mul(4)?;
    if bytes == 0 || bytes > 128 * 1024 * 1024 || info.pixel_format > 1 {
        return None;
    }
    const BASE: u64 = 0xffff_a000_0000_0000;
    let pages = bytes.div_ceil(4096);
    unsafe {
        for i in 0..pages {
            let frame = allocator.allocate_frame()?;
            memory::map_page(
                memory::get_table_mut(memory::current_pml4_phys()),
                BASE + i as u64 * 4096,
                frame,
                memory::PAGE_WRITABLE | memory::PAGE_NO_EXECUTE,
                allocator,
            );
        }
        let buffer = BASE as *mut u32;
        for y in 0..height {
            for x in 0..width {
                let pixel = read_volatile(
                    (info.framebuffer_base as *const u32)
                        .add(y * info.pixels_per_scanline as usize + x),
                );
                *buffer.add(y * width + x) = convert(pixel, info.pixel_format);
            }
        }
    }
    *DISPLAY.lock() = Some(Display {
        buffer: BASE as usize,
        gop: info.framebuffer_base as usize,
        gop_stride: info.pixels_per_scanline as usize,
        pixel_format: info.pixel_format,
        width,
        height,
        gpu: None,
        dirty: Some((0, 0, width, height)),
        failed: false,
    });
    Some(FramebufferInfo {
        base: BASE as *mut u32,
        stride: width,
        width,
        height,
    })
}
