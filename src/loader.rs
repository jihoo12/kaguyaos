use crate::memory::address_space::{USER_CODE_BASE, USER_STACK_TOP};
use crate::memory::{AddressSpace, PAGE_SIZE};

#[repr(C, packed)]
#[derive(Debug, Copy, Clone)]
pub struct KefHeader {
    pub magic: [u8; 4],         // Magic bytes: b"KEF\0"
    pub entry_offset: u32,      // Offset from the start of the loaded code segment to the entry instruction
    pub code_offset: u32,       // File offset where the code segment starts
    pub code_size: u32,         // Size of the code segment in bytes
}

/// Loads a KEF executable into `space`: code at USER_CODE_BASE, a stack below
/// USER_STACK_TOP with an unmapped guard page, and `args` copied to the top of
/// the stack. Returns (entry point, initial RSP, args pointer).
pub fn load_kef(
    file_data: &[u8],
    space: &AddressSpace,
    args: &[u8],
) -> Result<(u64, u64, u64), &'static str> {
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

    // Trailing .bss is not stored in the flat binary; the loader zero-fills
    // whatever part of code_size lies past the end of the file.
    let file_code_start = header.code_offset as usize;
    let file_code_end = file_code_start
        .checked_add(code_size)
        .ok_or("KEF code size overflow")?
        .min(file_data.len());
    if file_code_start > file_code_end || header.entry_offset as usize >= code_size {
        return Err("Invalid KEF code range or entry point");
    }

    const STACK_PAGES: u64 = 4;
    if args.len() > PAGE_SIZE as usize / 2 {
        return Err("KEF arguments too long");
    }

    let code = &file_data[file_code_start..file_code_end];
    for i in 0..code_pages {
        let frame = space
            .map_new_page(USER_CODE_BASE + (i as u64) * PAGE_SIZE, true, true)
            .ok_or("OOM mapping KEF code")?;
        if let Some(chunk) = code.chunks(PAGE_SIZE as usize).nth(i) {
            unsafe {
                core::ptr::copy_nonoverlapping(chunk.as_ptr(), frame as *mut u8, chunk.len());
            }
        }
    }

    let mut top_frame = 0;
    for i in (1..=STACK_PAGES).rev() {
        top_frame = space
            .map_new_page(USER_STACK_TOP - i * PAGE_SIZE, true, false)
            .ok_or("OOM mapping KEF stack")?;
    }

    let args_ptr = (USER_STACK_TOP - args.len() as u64) & !0xF;
    let args_offset = args_ptr - (USER_STACK_TOP - PAGE_SIZE);
    unsafe {
        core::ptr::copy_nonoverlapping(
            args.as_ptr(),
            (top_frame + args_offset) as *mut u8,
            args.len(),
        );
    }

    let entry_point = USER_CODE_BASE + header.entry_offset as u64;
    crate::println!(
        "[kef] loaded {} bytes at {:#x}, {} frames in use",
        code_size,
        USER_CODE_BASE,
        crate::memory::frames_in_use()
    );

    Ok((entry_point, args_ptr, args_ptr))
}
