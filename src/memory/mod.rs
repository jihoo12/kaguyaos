pub mod address_space;
#[cfg(not(feature = "jihoo-kernel"))]
pub mod heap;
#[cfg(feature = "jihoo-kernel")]
#[path = "../jihoo/heap.rs"]
pub mod heap;

pub use address_space::AddressSpace;

use crate::BootInfo;
#[cfg(not(feature = "jihoo-kernel"))]
use crate::uefi::{EFI_CONVENTIONAL_MEMORY, EFI_MEMORY_DESCRIPTOR};
use core::arch::asm;

pub const PAGE_SIZE: u64 = 4096;

pub const PAGE_PRESENT: u64 = 1 << 0;
pub const PAGE_WRITABLE: u64 = 1 << 1;
pub const PAGE_USER: u64 = 1 << 2;
pub const PAGE_CACHE_DISABLE: u64 = 1 << 4;
pub const PAGE_NO_EXECUTE: u64 = 1 << 63;

#[cfg(feature = "jihoo-kernel")]
unsafe extern "C" {
    fn jihoo_boot_frame_alloc(
        memory_map: *const u8,
        map_size: usize,
        stride: usize,
        descriptor_index: *mut usize,
        page_offset: *mut u64,
    ) -> u64;
    fn jihoo_boot_frames_alloc(
        memory_map: *const u8,
        map_size: usize,
        stride: usize,
        descriptor_index: *mut usize,
        page_offset: *mut u64,
        count: usize,
    ) -> u64;
}

/// A simple physical frame allocator using UEFI memory map.
pub struct FrameAllocator {
    memory_map: *const u8,
    memory_map_size: usize,
    pub descriptor_size: usize,
    pub descriptor_version: u32,

    current_descriptor_index: usize,
    current_page_offset: u64,
}

impl FrameAllocator {
    /// # Safety
    /// BootInfo memory map must be valid.
    pub unsafe fn new(boot_info: &BootInfo) -> Self {
        Self {
            memory_map: boot_info.memory_map,
            memory_map_size: boot_info.memory_map_size,
            descriptor_size: boot_info.descriptor_size,
            descriptor_version: boot_info.descriptor_version,
            current_descriptor_index: 0,
            current_page_offset: 0,
        }
    }

    /// Boot-time allocation; skipped fragments remain reserved (no frame free list).
    #[cfg(feature = "jihoo-kernel")]
    pub fn allocate_contiguous(&mut self, count: usize) -> Option<u64> {
        let frame = unsafe {
            jihoo_boot_frames_alloc(
                self.memory_map,
                self.memory_map_size,
                self.descriptor_size,
                &mut self.current_descriptor_index,
                &mut self.current_page_offset,
                count,
            )
        };
        if frame == 0 { None } else { Some(frame) }
    }

    #[cfg(not(feature = "jihoo-kernel"))]
    pub fn allocate_contiguous(&mut self, count: usize) -> Option<u64> {
        if count == 0 { return None; }
        let mut start = self.allocate_frame()?;
        let mut found = 1;
        while found < count {
            let next = self.allocate_frame()?;
            if next == start + found as u64 * PAGE_SIZE { found += 1; }
            else { start = next; found = 1; }
        }
        Some(start)
    }

    #[cfg(feature = "jihoo-kernel")]
    pub fn allocate_frame(&mut self) -> Option<u64> {
        let frame = unsafe {
            jihoo_boot_frame_alloc(
                self.memory_map,
                self.memory_map_size,
                self.descriptor_size,
                &mut self.current_descriptor_index,
                &mut self.current_page_offset,
            )
        };
        if frame == 0 { None } else { Some(frame) }
    }

    #[cfg(not(feature = "jihoo-kernel"))]
    pub fn allocate_frame(&mut self) -> Option<u64> {
        let num_descriptors = self.memory_map_size / self.descriptor_size;

        while self.current_descriptor_index < num_descriptors {
            let offset = self.current_descriptor_index * self.descriptor_size;
            // SAFE: We are within bounds derived from map size
            let descriptor_ptr =
                unsafe { self.memory_map.add(offset) } as *const EFI_MEMORY_DESCRIPTOR;
            let descriptor = unsafe { &*descriptor_ptr };

            if descriptor.Type == EFI_CONVENTIONAL_MEMORY {
                if self.current_page_offset < descriptor.NumberOfPages {
                    let frame_address =
                        descriptor.PhysicalStart + (self.current_page_offset * PAGE_SIZE);
                    self.current_page_offset += 1;
                    if frame_address > 0 {
                        return Some(frame_address);
                    }
                }
            }

            self.current_descriptor_index += 1;
            self.current_page_offset = 0;
        }
        None
    }
}

// ---------------------------------------------------------------------------
// Global frame allocator support (used by exec syscall)
// ---------------------------------------------------------------------------

/// Boot info parameters needed to create a FrameAllocator on demand.
static mut BOOT_INFO_MEM_MAP: *const u8 = core::ptr::null();
static mut BOOT_INFO_MEM_MAP_SIZE: usize = 0;
static mut BOOT_INFO_DESC_SIZE: usize = 0;
static mut BOOT_INFO_DESC_VER: u32 = 0;

pub unsafe fn store_boot_info(
    memory_map: *const u8,
    memory_map_size: usize,
    descriptor_size: usize,
    descriptor_version: u32,
) {
    unsafe {
        BOOT_INFO_MEM_MAP = memory_map;
        BOOT_INFO_MEM_MAP_SIZE = memory_map_size;
        BOOT_INFO_DESC_SIZE = descriptor_size;
        BOOT_INFO_DESC_VER = descriptor_version;
    }
}

#[cfg(not(feature = "jihoo-kernel"))]
mod frame;
#[cfg(feature = "jihoo-kernel")]
#[path = "../jihoo/frame.rs"]
mod frame;
pub use frame::{alloc_frame, commit_frame_allocator, frames_in_use, free_frame};

unsafe impl Send for FrameAllocator {}

/// Source of page-table frames for `map_page`.
pub trait FrameSource {
    fn allocate_frame(&mut self) -> Option<u64>;
}

impl FrameSource for FrameAllocator {
    fn allocate_frame(&mut self) -> Option<u64> {
        FrameAllocator::allocate_frame(self)
    }
}

/// The global frame pool, usable once boot has committed its allocator.
pub struct GlobalFrames;

impl FrameSource for GlobalFrames {
    fn allocate_frame(&mut self) -> Option<u64> {
        alloc_frame()
    }
}

pub struct PageTable {
    pub entries: [u64; 512],
}

impl PageTable {
    pub fn zero(&mut self) {
        #[cfg(feature = "jihoo-kernel")]
        unsafe { paging::jihoo_page_zero(self.entries.as_mut_ptr()) };
        #[cfg(not(feature = "jihoo-kernel"))]
        self.entries.fill(0);
    }
}

/// Helper to get a mutable reference to a PageTable from a physical address.
pub unsafe fn get_table_mut(phys_addr: u64) -> &'static mut PageTable {
    unsafe { &mut *(phys_addr as *mut PageTable) }
}

/// Read the physical address of the active PML4 from CR3.
pub fn current_pml4_phys() -> u64 {
    let cr3: u64;
    unsafe { core::arch::asm!("mov {}, cr3", out(reg) cr3, options(nomem, nostack, preserves_flags)) };
    cr3
}

#[cfg(not(feature = "jihoo-kernel"))]
mod paging;
#[cfg(feature = "jihoo-kernel")]
#[path = "../jihoo/paging.rs"]
mod paging;
pub use paging::{is_user_range_mapped, map_page};

/// Check that every page is present, including large-page mappings.
pub fn is_range_mapped(pml4: &PageTable, start: u64, len: u64) -> bool {
    paging::is_range_mapped(pml4, start, len)
}

static KERNEL_PML4: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

/// The boot page table, used by kernel threads and idle CPUs.
pub fn kernel_pml4_phys() -> u64 {
    KERNEL_PML4.load(core::sync::atomic::Ordering::Relaxed)
}

/// Load `pml4_phys` into CR3 unless it is already active.
pub unsafe fn switch_pml4(pml4_phys: u64) {
    if current_pml4_phys() != pml4_phys {
        unsafe { asm!("mov cr3, {}", in(reg) pml4_phys, options(nostack, preserves_flags)) };
    }
}

pub unsafe fn init_paging(boot_info: &BootInfo, allocator: &mut FrameAllocator) -> u64 {
    #[cfg(feature = "jihoo-kernel")]
    let pml4_phys = unsafe { paging::build_paging(boot_info, allocator) };
    #[cfg(not(feature = "jihoo-kernel"))]
    let pml4_phys = {
        // 1. Allocate PML4
        let pml4_phys = allocator.allocate_frame().expect("Failed to allocate PML4");
        let pml4 = unsafe { get_table_mut(pml4_phys) };
        pml4.zero();

        // 2. Identity Map Regions
        let num_descriptors = allocator.memory_map_size / allocator.descriptor_size;

        for i in 0..num_descriptors {
            let offset = i * allocator.descriptor_size;
            let descriptor_ptr =
                unsafe { allocator.memory_map.add(offset) } as *const EFI_MEMORY_DESCRIPTOR;
            let descriptor = unsafe { &*descriptor_ptr };

            match descriptor.Type {
                crate::uefi::EFI_CONVENTIONAL_MEMORY
                | crate::uefi::EFI_LOADER_CODE
                | crate::uefi::EFI_LOADER_DATA
                | crate::uefi::EFI_BOOT_SERVICES_CODE
                | crate::uefi::EFI_BOOT_SERVICES_DATA
                | crate::uefi::EFI_RUNTIME_SERVICES_CODE
                | crate::uefi::EFI_RUNTIME_SERVICES_DATA
                | crate::uefi::EFI_ACPI_RECLAIM_MEMORY
                | crate::uefi::EFI_ACPI_MEMORY_NVS
                | crate::uefi::EFI_MEMORY_MAPPED_IO
                | crate::uefi::EFI_MEMORY_MAPPED_IO_PORT_SPACE => {
                    let start = descriptor.PhysicalStart;
                    let end = start + (descriptor.NumberOfPages * PAGE_SIZE);
                    for addr in (start..end).step_by(PAGE_SIZE as usize) {
                        unsafe { map_page(pml4, addr, addr, PAGE_WRITABLE, allocator) };
                    }
                }
                _ => {}
            }
        }

        // 3. Map Framebuffer
        let fb_base = boot_info.framebuffer_base;
        let fb_size = boot_info.framebuffer_size as u64;
        for addr in (fb_base..(fb_base + fb_size)).step_by(PAGE_SIZE as usize) {
            unsafe { map_page(pml4, addr, addr, PAGE_WRITABLE, allocator) };
        }

        pml4_phys
    };

    // 4. Load CR3
    unsafe { asm!("mov cr3, {}", in(reg) pml4_phys) };
    KERNEL_PML4.store(pml4_phys, core::sync::atomic::Ordering::Relaxed);

    pml4_phys
}
