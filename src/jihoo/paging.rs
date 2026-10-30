use super::{FrameAllocator, FrameSource, PageTable};

unsafe extern "C" {
    pub(super) fn jihoo_page_zero(table: *mut u64);
    fn jihoo_page_map(
        root: *mut u64,
        address: u64,
        physical: u64,
        flags: u64,
        context: *mut u8,
        allocate: extern "C" fn(*mut u8) -> u64,
    ) -> bool;
    fn jihoo_range_mapped(root: *const u64, start: u64, length: u64, user: bool) -> bool;
    fn jihoo_paging_init(
        map: *const u8,
        size: usize,
        stride: usize,
        framebuffer: u64,
        framebuffer_size: usize,
        context: *mut u8,
        allocate: extern "C" fn(*mut u8) -> u64,
    ) -> u64;
}

extern "C" fn allocate<A: FrameSource>(context: *mut u8) -> u64 {
    unsafe { (&mut *context.cast::<A>()).allocate_frame().unwrap_or(0) }
}

pub(super) unsafe fn build_paging(info: &crate::BootInfo, allocator: &mut FrameAllocator) -> u64 {
    let root = unsafe {
        jihoo_paging_init(
            allocator.memory_map,
            allocator.memory_map_size,
            allocator.descriptor_size,
            info.framebuffer_base,
            info.framebuffer_size,
            (allocator as *mut FrameAllocator).cast(),
            allocate::<FrameAllocator>,
        )
    };
    assert_ne!(root, 0, "boot page-table allocation failed");
    root
}

pub unsafe fn map_page<A: FrameSource>(
    pml4: &mut PageTable,
    virt_addr: u64,
    phys_addr: u64,
    flags: u64,
    allocator: &mut A,
) {
    let mapped = unsafe {
        jihoo_page_map(
            pml4.entries.as_mut_ptr(),
            virt_addr,
            phys_addr,
            flags,
            (allocator as *mut A).cast(),
            allocate::<A>,
        )
    };
    assert!(mapped, "page-table allocation or mapping failed");
    unsafe {
        core::arch::asm!("invlpg [{}]", in(reg) virt_addr, options(nostack, preserves_flags));
    }
}

pub fn is_range_mapped(pml4: &PageTable, start: u64, len: u64) -> bool {
    unsafe { jihoo_range_mapped(pml4.entries.as_ptr(), start, len, false) }
}

pub fn is_user_range_mapped(pml4: &PageTable, start: u64, len: u64) -> bool {
    unsafe { jihoo_range_mapped(pml4.entries.as_ptr(), start, len, true) }
}
