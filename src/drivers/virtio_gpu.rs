//! Minimal modern PCI virtio-gpu, split control queue, one 2D scanout.
//! DMA storage lives in the identity-mapped kernel image for its entire lifetime.
use crate::{drivers::pci, memory};
use core::ptr::{addr_of_mut, read_volatile, write_volatile};
use core::sync::atomic::{Ordering, fence};

const N: usize = 8;
const POLL_LIMIT: usize = 10_000_000;
#[repr(C, align(4096))]
struct Dma([u8; 16384]);
static mut DMA: Dma = Dma([0; 16384]);

pub struct Gpu {
    common: usize,
    notify: usize,
    dma: usize,
    index: u16,
    width: u32,
    height: u32,
    healthy: bool,
}

unsafe fn rd<T: Copy>(base: usize, offset: usize) -> T {
    unsafe { read_volatile((base + offset) as *const T) }
}
unsafe fn wr<T>(base: usize, offset: usize, value: T) {
    unsafe { write_volatile((base + offset) as *mut T, value) }
}

impl Gpu {
    pub unsafe fn init(
        dev: pci::PciDevice,
        allocator: &mut memory::FrameAllocator,
        buffer: u64,
        width: u32,
        height: u32,
    ) -> Result<Self, &'static str> {
        unsafe {
            let cfg8 = |o| pci::read_config_8(dev.bus, dev.device, dev.function, o);
            let cfg32 = |o| pci::read_config_32(dev.bus, dev.device, dev.function, o);
            let mut common = 0;
            let mut notify = 0;
            let mut notify_len = 0;
            let mut multiplier = 0;
            let mut cap = cfg8(0x34) & !3;
            let mut visited = [false; 256];
            while cap != 0 {
                if cap < 0x40 || cap > 0xfc || visited[cap as usize] {
                    return Err("invalid PCI capability chain");
                }
                visited[cap as usize] = true;
                if cfg8(cap) == 9 {
                    if cap > 0xf0 || cfg8(cap + 2) < 16 {
                        return Err("short virtio capability");
                    }
                    let kind = cfg8(cap + 3);
                    if kind == 1 || kind == 2 {
                        let bar = cfg8(cap + 4);
                        if bar > 5 {
                            return Err("invalid BAR");
                        }
                        let low = cfg32(0x10 + bar * 4);
                        if low & 1 != 0 {
                            return Err("I/O BAR unsupported");
                        }
                        let mut base = (low & !15) as u64;
                        if low & 6 == 4 {
                            if bar == 5 {
                                return Err("invalid 64-bit BAR");
                            }
                            base |= (cfg32(0x14 + bar * 4) as u64) << 32;
                        }
                        if base == 0 {
                            return Err("unassigned BAR");
                        }
                        let offset = cfg32(cap + 8) as u64;
                        let len = cfg32(cap + 12) as u64;
                        if len == 0 || len > 0x100000 {
                            return Err("invalid capability length");
                        }
                        let start = base.checked_add(offset).ok_or("BAR overflow")?;
                        let end = start.checked_add(len).ok_or("BAR overflow")?;
                        let pml4 = memory::get_table_mut(memory::current_pml4_phys());
                        for page in ((start & !4095)..end).step_by(4096) {
                            memory::map_page(
                                pml4,
                                page,
                                page,
                                memory::PAGE_WRITABLE | memory::PAGE_CACHE_DISABLE,
                                allocator,
                            );
                            core::arch::asm!("invlpg [{}]", in(reg) page, options(nostack));
                        }
                        if kind == 1 && common == 0 {
                            if len < 56 || start & 3 != 0 {
                                return Err("invalid common config");
                            }
                            common = start as usize;
                        } else if kind == 2 && notify == 0 {
                            if cap > 0xec || cfg8(cap + 2) < 20 {
                                return Err("short notify capability");
                            }
                            notify = start as usize;
                            notify_len = len;
                            multiplier = cfg32(cap + 16) as u64;
                        }
                    }
                }
                cap = cfg8(cap + 1) & !3;
            }
            if common == 0 || notify == 0 {
                return Err("missing virtio capabilities");
            }
            // Disable INTx: this first implementation polls the used ring.
            let cmd = pci::read_config_16(dev.bus, dev.device, dev.function, 4) | 6 | (1 << 10);
            pci::write_config_32(dev.bus, dev.device, dev.function, 4, cmd as u32);
            wr(common, 20, 0u8);
            let mut reset = false;
            for _ in 0..POLL_LIMIT {
                if rd::<u8>(common, 20) == 0 {
                    reset = true;
                    break;
                }
                core::hint::spin_loop();
            }
            if !reset {
                return Err("reset timeout");
            }
            let mut gpu = Self {
                common,
                notify,
                dma: addr_of_mut!(DMA) as usize,
                index: 0,
                width,
                height,
                healthy: true,
            };
            let result = gpu.setup(notify_len, multiplier, buffer);
            if let Err(error) = result {
                gpu.fail();
                return Err(error);
            }
            Ok(gpu)
        }
    }

    unsafe fn setup(
        &mut self,
        notify_len: u64,
        multiplier: u64,
        buffer: u64,
    ) -> Result<(), &'static str> {
        unsafe {
            let c = self.common;
            wr(c, 20, 3u8); // ACKNOWLEDGE | DRIVER
            wr(c, 0, 1u32);
            if rd::<u32>(c, 4) & 1 == 0 {
                return Err("VIRTIO_F_VERSION_1 missing");
            }
            wr(c, 8, 0u32);
            wr(c, 12, 0u32);
            wr(c, 8, 1u32);
            wr(c, 12, 1u32);
            wr(c, 20, 11u8); // FEATURES_OK
            if rd::<u8>(c, 20) & 8 == 0 {
                return Err("features rejected");
            }
            wr(c, 22, 0u16);
            if rd::<u16>(c, 24) < N as u16 {
                return Err("control queue too small");
            }
            wr(c, 24, N as u16);
            wr(c, 26, 0xffffu16);
            let offset = rd::<u16>(c, 30) as u64 * multiplier;
            if offset + 2 > notify_len || (self.notify + offset as usize) & 1 != 0 {
                return Err("notify out of bounds");
            }
            self.notify += offset as usize;
            core::ptr::write_bytes(self.dma as *mut u8, 0, 16384);
            // Descriptor table, available ring, used ring, request, response.
            wr(self.dma, 256, 1u16); // NO_INTERRUPT
            wr(c, 32, self.dma as u64);
            wr(c, 40, (self.dma + 256) as u64);
            wr(c, 48, (self.dma + 512) as u64);
            wr(c, 28, 1u16);
            wr(c, 20, 15u8); // DRIVER_OK
            self.command(0x100, &[], 0x1101)?; // GET_DISPLAY_INFO
            let mut scanout = None;
            for i in 0..16 {
                if rd::<u32>(self.dma + 8192, 24 + i * 24 + 16) != 0 {
                    scanout = Some(i as u32);
                    break;
                }
            }
            let scanout = scanout.ok_or("no enabled scanout")?;
            self.command(0x101, &[1, 2, self.width, self.height], 0x1100)?;
            self.command(
                0x106,
                &[
                    1,
                    1,
                    buffer as u32,
                    (buffer >> 32) as u32,
                    self.width * self.height * 4,
                    0,
                ],
                0x1100,
            )?;
            self.transfer()?;
            self.command(0x103, &[0, 0, self.width, self.height, scanout, 1], 0x1100)?;
            self.present()?;
            crate::console::serial(format_args!(
                "virtio-gpu: scanout {} active {}x{}\n",
                scanout, self.width, self.height
            ));
            Ok(())
        }
    }

    fn fail(&mut self) {
        self.healthy = false;
        unsafe {
            wr(self.common, 20, rd::<u8>(self.common, 20) | 128);
        }
        // Retain DMA memory even on timeout; the device may still reference it.
    }

    fn command(&mut self, kind: u32, words: &[u32], expected: u32) -> Result<(), &'static str> {
        unsafe {
            if !self.healthy {
                return Err("GPU disabled");
            }
            let req = self.dma + 4096;
            let resp = self.dma + 8192;
            core::ptr::write_bytes(req as *mut u8, 0, 4096);
            core::ptr::write_bytes(resp as *mut u8, 0, 4096);
            wr(req, 0, kind);
            wr(req, 4, 1u32); // FENCE: completion means the command has finished.
            wr(req, 8, self.index as u64 + 1);
            for (i, value) in words.iter().enumerate() {
                wr(req, 24 + i * 4, *value);
            }
            wr(self.dma, 0, req as u64);
            wr(self.dma, 8, (24 + words.len() * 4) as u32);
            wr(self.dma, 12, 1u16);
            wr(self.dma, 14, 1u16);
            wr(self.dma, 16, resp as u64);
            wr(self.dma, 24, 4096u32);
            wr(self.dma, 28, 2u16);
            wr(self.dma, 260 + (self.index as usize % N) * 2, 0u16);
            fence(Ordering::SeqCst);
            let next = self.index.wrapping_add(1);
            wr(self.dma, 258, next);
            fence(Ordering::SeqCst);
            wr(self.notify, 0, 0u16);
            for _ in 0..POLL_LIMIT {
                if rd::<u16>(self.dma, 514) == next {
                    fence(Ordering::SeqCst);
                    let used = 516 + (self.index as usize % N) * 8;
                    let len = rd::<u32>(self.dma, used + 4);
                    let minimum = if expected == 0x1101 { 408 } else { 24 };
                    if rd::<u32>(self.dma, used) != 0
                        || len < minimum
                        || len > 4096
                        || rd::<u32>(resp, 0) != expected
                        || rd::<u32>(resp, 4) & 1 == 0
                        || rd::<u64>(resp, 8) != self.index as u64 + 1
                    {
                        self.fail();
                        return Err("invalid GPU response");
                    }
                    self.index = next;
                    return Ok(());
                }
                core::hint::spin_loop();
            }
            self.fail();
            Err("control queue timeout")
        }
    }
    fn transfer(&mut self) -> Result<(), &'static str> {
        self.command(0x105, &[0, 0, self.width, self.height, 0, 0, 1, 0], 0x1100)
    }
    pub fn present(&mut self) -> Result<(), &'static str> {
        self.transfer()?;
        self.command(0x104, &[0, 0, self.width, self.height, 1, 0], 0x1100)
    }
}
