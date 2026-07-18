pub mod heap;

use crate::BootInfo;
use crate::uefi::{EFI_CONVENTIONAL_MEMORY, EFI_MEMORY_DESCRIPTOR};
use core::arch::asm;

pub const PAGE_SIZE: u64 = 4096;

pub const PAGE_PRESENT: u64 = 1 << 0;
pub const PAGE_WRITABLE: u64 = 1 << 1;
pub const PAGE_USER: u64 = 1 << 2;
pub const PAGE_CACHE_DISABLE: u64 = 1 << 4;
pub const PAGE_NO_EXECUTE: u64 = 1 << 63;

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

/// Persistent frame allocator: the global tracks the high-water mark of all
/// frames ever given out.  `new_frame_allocator()` clones the current global
/// position.  After `load_kef` (or any heavy allocation), call
/// `commit_frame_allocator()` to push the new position back into the global.
static mut GLOBAL_ALLOCATOR: Option<FrameAllocator> = None;

pub fn new_frame_allocator() -> FrameAllocator {
    unsafe {
        if let Some(ref alloc) = GLOBAL_ALLOCATOR {
            FrameAllocator {
                memory_map: alloc.memory_map,
                memory_map_size: alloc.memory_map_size,
                descriptor_size: alloc.descriptor_size,
                descriptor_version: alloc.descriptor_version,
                current_descriptor_index: alloc.current_descriptor_index,
                current_page_offset: alloc.current_page_offset,
            }
        } else {
            FrameAllocator {
                memory_map: BOOT_INFO_MEM_MAP,
                memory_map_size: BOOT_INFO_MEM_MAP_SIZE,
                descriptor_size: BOOT_INFO_DESC_SIZE,
                descriptor_version: BOOT_INFO_DESC_VER,
                current_descriptor_index: 0,
                current_page_offset: 0,
            }
        }
    }
}

/// Save the allocator's current position back into the global so the next
/// `new_frame_allocator()` call resumes from here instead of from the start.
pub fn commit_frame_allocator(alloc: &FrameAllocator) {
    unsafe {
        GLOBAL_ALLOCATOR = Some(FrameAllocator {
            memory_map: alloc.memory_map,
            memory_map_size: alloc.memory_map_size,
            descriptor_size: alloc.descriptor_size,
            descriptor_version: alloc.descriptor_version,
            current_descriptor_index: alloc.current_descriptor_index,
            current_page_offset: alloc.current_page_offset,
        });
    }
}

pub fn init_global_frame_allocator() {
    unsafe {
        GLOBAL_ALLOCATOR = Some(FrameAllocator {
            memory_map: BOOT_INFO_MEM_MAP,
            memory_map_size: BOOT_INFO_MEM_MAP_SIZE,
            descriptor_size: BOOT_INFO_DESC_SIZE,
            descriptor_version: BOOT_INFO_DESC_VER,
            current_descriptor_index: 0,
            current_page_offset: 0,
        });
    }
}

pub struct PageTable {
    pub entries: [u64; 512],
}

impl PageTable {
    pub fn zero(&mut self) {
        for i in 0..512 {
            self.entries[i] = 0;
        }
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

/// Maps a virtual address to a physical address.
pub unsafe fn map_page(
    pml4: &mut PageTable,
    virt_addr: u64,
    phys_addr: u64,
    flags: u64,
    allocator: &mut FrameAllocator,
) {
    let pml4_idx = ((virt_addr >> 39) & 0x1FF) as usize;
    let pdp_idx = ((virt_addr >> 30) & 0x1FF) as usize;
    let pd_idx = ((virt_addr >> 21) & 0x1FF) as usize;
    let pt_idx = ((virt_addr >> 12) & 0x1FF) as usize;

    // 1. Get PDPT
    if (pml4.entries[pml4_idx] & PAGE_PRESENT) == 0 {
        let frame = allocator.allocate_frame().expect("OOM allocating PDPT");
        let table = unsafe { get_table_mut(frame) };
        table.zero();
        pml4.entries[pml4_idx] = frame | PAGE_PRESENT | PAGE_WRITABLE | PAGE_USER;
    }
    let pdpt_phys = pml4.entries[pml4_idx] & !0xFFF;
    let pdpt = unsafe { get_table_mut(pdpt_phys) };

    // 2. Get PD
    if (pdpt.entries[pdp_idx] & PAGE_PRESENT) == 0 {
        let frame = allocator.allocate_frame().expect("OOM allocating PD");
        let table = unsafe { get_table_mut(frame) };
        table.zero();
        pdpt.entries[pdp_idx] = frame | PAGE_PRESENT | PAGE_WRITABLE | PAGE_USER;
    }
    let pd_phys = pdpt.entries[pdp_idx] & !0xFFF;
    let pd = unsafe { get_table_mut(pd_phys) };

    // 3. Get PT
    if (pd.entries[pd_idx] & PAGE_PRESENT) == 0 {
        let frame = allocator.allocate_frame().expect("OOM allocating PT");
        let table = unsafe { get_table_mut(frame) };
        table.zero();
        pd.entries[pd_idx] = frame | PAGE_PRESENT | PAGE_WRITABLE | PAGE_USER;
    }
    let pt_phys = pd.entries[pd_idx] & !0xFFF;
    let pt = unsafe { get_table_mut(pt_phys) };

    // 4. Map Page
    pt.entries[pt_idx] = phys_addr | flags | PAGE_PRESENT;

    // 5. Flush TLB entry for this virtual address
    unsafe {
        core::arch::asm!("invlpg [{}]", in(reg) virt_addr, options(nostack, preserves_flags));
    }
}

/// Check whether every page in the virtual address range [start, start + len)
/// is present in the current page tables.  Returns true if all pages are mapped.
pub fn is_range_mapped(pml4: &PageTable, start: u64, len: u64) -> bool {
    if len == 0 {
        return true;
    }
    let end = start + len;
    let mut addr = start & !0xFFF; // page-align down

    while addr < end {
        if !is_page_mapped(pml4, addr) {
            return false;
        }
        addr += PAGE_SIZE;
    }
    true
}

fn is_page_mapped(pml4: &PageTable, virt_addr: u64) -> bool {
    let pml4_idx = ((virt_addr >> 39) & 0x1FF) as usize;
    let pdp_idx = ((virt_addr >> 30) & 0x1FF) as usize;
    let pd_idx = ((virt_addr >> 21) & 0x1FF) as usize;
    let pt_idx = ((virt_addr >> 12) & 0x1FF) as usize;

    if (pml4.entries[pml4_idx] & PAGE_PRESENT) == 0 {
        return false;
    }
    let pdpt = unsafe { get_table_mut(pml4.entries[pml4_idx] & !0xFFF) };

    if (pdpt.entries[pdp_idx] & PAGE_PRESENT) == 0 {
        return false;
    }
    let pd = unsafe { get_table_mut(pdpt.entries[pdp_idx] & !0xFFF) };

    // Check for huge page (1GB or 2MB)
    if (pd.entries[pd_idx] & PAGE_PRESENT) == 0 {
        return false;
    }
    if (pd.entries[pd_idx] & (1 << 7)) != 0 {
        return true; // 2MB page
    }

    let pt = unsafe { get_table_mut(pd.entries[pd_idx] & !0xFFF) };
    (pt.entries[pt_idx] & PAGE_PRESENT) != 0
}

pub unsafe fn init_paging(boot_info: &BootInfo, allocator: &mut FrameAllocator) -> u64 {
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

    // 4. Load CR3
    unsafe { asm!("mov cr3, {}", in(reg) pml4_phys) };

    pml4_phys
}
