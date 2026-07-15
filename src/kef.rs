use crate::memory::{FrameAllocator, PageTable, map_page, PAGE_SIZE, PAGE_PRESENT, PAGE_WRITABLE, PAGE_USER};

#[repr(C, packed)]
#[derive(Debug, Copy, Clone)]
pub struct KefHeader {
    pub magic: [u8; 4],         // Magic bytes: b"KEF\0"
    pub entry_offset: u32,      // Offset from the start of the loaded code segment to the entry instruction
    pub code_offset: u32,       // File offset where the code segment starts
    pub code_size: u32,         // Size of the code segment in bytes
}

/// Loads a KEF executable from raw file bytes, allocates and maps its code and stack pages,
/// and returns (entry_point virtual address, user_rsp virtual address).
pub fn load_kef(
    file_data: &[u8],
    allocator: &mut FrameAllocator,
    pml4: &mut PageTable,
) -> Result<(u64, u64), &'static str> {
    if file_data.len() < core::mem::size_of::<KefHeader>() {
        return Err("File too small to contain KEF header");
    }

    // SAFETY: We checked the bounds of file_data.
    let header = unsafe { &*(file_data.as_ptr() as *const KefHeader) };
    if header.magic != [b'K', b'E', b'F', 0] {
        return Err("Invalid KEF magic number");
    }

    let code_size = header.code_size as usize;
    let code_pages = (code_size + 4095) / 4096;
    if code_pages == 0 {
        return Err("KEF code size is 0");
    }

    // Allocate contiguous frames for code
    let code_start_phys = allocator.allocate_frame().ok_or("OOM allocating code frame")?;
    for i in 1..code_pages {
        let frame = allocator.allocate_frame().ok_or("OOM allocating code frame")?;
        assert_eq!(
            frame,
            code_start_phys + i as u64 * 4096,
            "Allocated code frames are not contiguous"
        );
    }

    // Map code pages as user-accessible
    let flags = PAGE_PRESENT | PAGE_WRITABLE | PAGE_USER;
    for i in 0..code_pages {
        let addr = code_start_phys + i as u64 * 4096;
        unsafe {
            map_page(pml4, addr, addr, flags, allocator);
        }
    }

    // Copy code into the allocated frames
    let file_code_start = header.code_offset as usize;
    let file_code_end = file_code_start + code_size;
    if file_code_end > file_data.len() {
        return Err("KEF code segment extends past end of file");
    }

    unsafe {
        core::ptr::copy_nonoverlapping(
            file_data.as_ptr().add(file_code_start),
            code_start_phys as *mut u8,
            code_size,
        );
    }

    // Allocate stack frames (16KB / 4 pages) + 1 guard page
    let stack_pages = 4;
    let total_stack_frames = stack_pages + 1; // extra guard page at the bottom
    let guard_frame = allocator.allocate_frame().ok_or("OOM allocating stack guard")?;
    let stack_start_phys = guard_frame + PAGE_SIZE; // actual stack starts one page above guard
    for i in 1..total_stack_frames {
        let frame = allocator.allocate_frame().ok_or("OOM allocating stack frame")?;
        assert_eq!(
            frame,
            guard_frame + i as u64 * 4096,
            "Allocated stack frames are not contiguous"
        );
    }

    // Map only the usable stack pages (skip the guard frame — it stays unmapped)
    for i in 0..stack_pages {
        let addr = stack_start_phys + i as u64 * 4096;
        unsafe {
            map_page(pml4, addr, addr, flags, allocator);
        }
    }
    // guard_frame at (stack_start_phys - 4096) is intentionally NOT mapped.

    let entry_point = code_start_phys + header.entry_offset as u64;
    let user_rsp = stack_start_phys + (stack_pages as u64 * 4096);

    Ok((entry_point, user_rsp))
}
