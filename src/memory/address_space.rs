//! Per-process page tables.
//!
//! PML4 slots [USER_PML4_FIRST, USER_PML4_END) are private to a process and
//! freed with it. Every other slot is copied from the kernel PML4 when the
//! space is created, so kernel mappings below these slots (the identity map)
//! and above them (heaps, RAM disk) are shared. Kernel mappings must not add
//! new PML4 slots after the first process is created.
use super::{
    PAGE_NO_EXECUTE, PAGE_PRESENT, PAGE_USER, PAGE_WRITABLE, PageTable, alloc_frame,
    free_frame, get_table_mut, kernel_pml4_phys,
};
use crate::sync::Spinlock;
#[cfg(not(feature = "jihoo-kernel"))]
use super::PAGE_SIZE;

const USER_PML4_FIRST: usize = 128;
const USER_PML4_END: usize = 256;

pub const USER_SPACE_START: u64 = (USER_PML4_FIRST as u64) << 39;
pub const USER_SPACE_END: u64 = (USER_PML4_END as u64) << 39;

pub const USER_CODE_BASE: u64 = USER_SPACE_START;
pub const USER_HEAP_BASE: u64 = 0x0000_7000_0000_0000;
pub const USER_HEAP_LIMIT: u64 = USER_HEAP_BASE + 256 * 1024 * 1024;
pub const USER_STACK_TOP: u64 = 0x0000_7fff_ffff_0000;

#[cfg(feature = "jihoo-kernel")]
unsafe extern "C" {
    fn jihoo_space_create(kernel: u64) -> u64;
    fn jihoo_space_map(root: u64, address: u64, flags: u64) -> u64;
    fn jihoo_space_grow(root: u64, old: u64, bytes: u64) -> u64;
    fn jihoo_space_destroy(root: u64);
}

pub struct AddressSpace {
    pml4: u64,
    /// End of the mapped heap; pages in [USER_HEAP_BASE, heap_end) are mapped.
    heap_end: Spinlock<u64>,
}

impl AddressSpace {
    #[cfg(feature = "jihoo-kernel")]
    pub fn new() -> Option<Self> {
        let pml4 = unsafe { jihoo_space_create(kernel_pml4_phys()) };
        (pml4 != 0).then(|| Self { pml4, heap_end: Spinlock::new(USER_HEAP_BASE) })
    }

    #[cfg(not(feature = "jihoo-kernel"))]
    pub fn new() -> Option<Self> {
        let pml4 = alloc_frame()?;
        let kernel = unsafe { get_table_mut(kernel_pml4_phys()) };
        let table = unsafe { get_table_mut(pml4) };
        for i in (0..USER_PML4_FIRST).chain(USER_PML4_END..512) {
            table.entries[i] = kernel.entries[i];
        }
        Some(Self {
            pml4,
            heap_end: Spinlock::new(USER_HEAP_BASE),
        })
    }

    pub fn pml4_phys(&self) -> u64 {
        self.pml4
    }

    /// Map a fresh zeroed user page at `virt` and return its physical frame,
    /// which stays reachable through the kernel identity map.
    pub fn map_new_page(&self, virt: u64, writable: bool, executable: bool) -> Option<u64> {
        let mut flags = PAGE_PRESENT | PAGE_USER;
        if writable {
            flags |= PAGE_WRITABLE;
        }
        if !executable {
            flags |= PAGE_NO_EXECUTE;
        }
        #[cfg(feature = "jihoo-kernel")]
        {
            let frame = unsafe { jihoo_space_map(self.pml4, virt, flags) };
            return (frame != 0).then_some(frame);
        }
        #[cfg(not(feature = "jihoo-kernel"))]
        {
            let frame = alloc_frame()?;
            if self.map(virt, frame | flags) {
                Some(frame)
            } else {
                free_frame(frame);
                None
            }
        }
    }

    fn map(&self, virt: u64, entry: u64) -> bool {
        let Some(table) = self.leaf_table(virt) else {
            return false;
        };
        let idx = ((virt >> 12) & 0x1FF) as usize;
        if table.entries[idx] & PAGE_PRESENT != 0 {
            return false;
        }
        table.entries[idx] = entry;
        true
    }

    /// The page table covering `virt`, creating intermediate tables as needed.
    fn leaf_table(&self, virt: u64) -> Option<&mut PageTable> {
        if !(USER_SPACE_START..USER_SPACE_END).contains(&virt) {
            return None;
        }
        let mut table = unsafe { get_table_mut(self.pml4) };
        for shift in [39, 30, 21] {
            let idx = ((virt >> shift) & 0x1FF) as usize;
            if table.entries[idx] & PAGE_PRESENT == 0 {
                table.entries[idx] = alloc_frame()? | PAGE_PRESENT | PAGE_WRITABLE | PAGE_USER;
            }
            table = unsafe { get_table_mut(table.entries[idx] & !0xFFF) };
        }
        Some(table)
    }

    /// Extend the heap by `bytes` (rounded up to pages) of zeroed memory and
    /// return the previous end. The kernel never reads heap contents; the
    /// allocator lives in userspace.
    ///
    /// All or nothing: page tables and frames are reserved before any page is
    /// mapped, so a failure never leaves pages behind or needs an unmap.
    #[cfg(feature = "jihoo-kernel")]
    pub fn grow_heap(&self, bytes: u64) -> Option<u64> {
        let mut end = self.heap_end.lock();
        let old = *end;
        let new = unsafe { jihoo_space_grow(self.pml4, old, bytes) };
        if new == 0 { return None; }
        *end = new;
        Some(old)
    }

    #[cfg(not(feature = "jihoo-kernel"))]
    pub fn grow_heap(&self, bytes: u64) -> Option<u64> {
        let mut end = self.heap_end.lock();
        let old = *end;
        let new = old
            .checked_add(bytes.checked_add(PAGE_SIZE - 1)? & !(PAGE_SIZE - 1))?;
        if new > USER_HEAP_LIMIT {
            return None;
        }

        let mut virt = old;
        while virt < new {
            self.leaf_table(virt)?;
            virt += PAGE_SIZE;
        }

        // Chain reserved frames through their first word.
        let mut frames = 0u64;
        let mut virt = old;
        while virt < new {
            let Some(frame) = alloc_frame() else {
                while frames != 0 {
                    let next = unsafe { *(frames as *const u64) };
                    free_frame(frames);
                    frames = next;
                }
                return None;
            };
            unsafe { *(frame as *mut u64) = frames };
            frames = frame;
            virt += PAGE_SIZE;
        }

        let mut virt = old;
        while virt < new {
            let frame = frames;
            frames = unsafe { *(frame as *const u64) };
            unsafe { *(frame as *mut u64) = 0 };
            let mapped = self.map(virt, frame | PAGE_PRESENT | PAGE_USER | PAGE_WRITABLE | PAGE_NO_EXECUTE);
            debug_assert!(mapped);
            virt += PAGE_SIZE;
        }
        *end = new;
        Some(old)
    }
}

impl Drop for AddressSpace {
    /// Callers guarantee no CPU has this PML4 loaded.
    fn drop(&mut self) {
        #[cfg(feature = "jihoo-kernel")]
        unsafe { jihoo_space_destroy(self.pml4); }
        #[cfg(not(feature = "jihoo-kernel"))]
        {
            let pml4 = unsafe { get_table_mut(self.pml4) };
            for i in USER_PML4_FIRST..USER_PML4_END {
                free_table(pml4.entries[i], 3);
            }
            free_frame(self.pml4);
        }
    }
}

/// Free a present entry and, for table levels, everything below it.
fn free_table(entry: u64, levels_below: u32) {
    if entry & PAGE_PRESENT == 0 {
        return;
    }
    let frame = entry & 0x000f_ffff_ffff_f000;
    if levels_below > 0 {
        let table: &PageTable = unsafe { get_table_mut(frame) };
        for &child in &table.entries {
            free_table(child, levels_below - 1);
        }
    }
    free_frame(frame);
}
