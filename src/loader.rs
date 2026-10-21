use crate::memory::address_space::{USER_CODE_BASE, USER_STACK_TOP};
use crate::memory::{AddressSpace, PAGE_SIZE};

/// KEF header, little-endian u32 fields after a 4-byte magic:
///
/// | offset | v0 (`KEF\0`) / v1 (`KEF\x01`) |
/// |--------|------------------------------------------------------------|
/// | 4      | entry_offset: entry point relative to the image start |
/// | 8      | code_offset: file offset of the image |
/// | 12     | code_size: in-memory image size |
/// | 16     | v1 only: init_size, bytes stored in the file; the loader zero-fills up to code_size (.bss) |
///
/// v0 images must store all of code_size in the file.
fn header_u32(file_data: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(file_data.get(offset..offset + 4)?.try_into().ok()?))
}

/// Loads a KEF executable into `space`: code at USER_CODE_BASE, a stack below
/// USER_STACK_TOP with an unmapped guard page, and `args` copied to the top of
/// the stack. Returns (entry point, initial RSP, args pointer).
pub fn load_kef(
    file_data: &[u8],
    space: &AddressSpace,
    args: &[u8],
) -> Result<(u64, u64, u64), &'static str> {
    let version = match file_data.get(0..4) {
        Some(b"KEF\0") => 0,
        Some(b"KEF\x01") => 1,
        Some(_) => return Err("Invalid KEF magic number"),
        None => return Err("File too small to contain KEF header"),
    };
    let field = |offset| header_u32(file_data, offset).ok_or("File too small to contain KEF header");
    let entry_offset = field(4)? as usize;
    let code_offset = field(8)? as usize;
    let code_size = field(12)? as usize;
    let init_size = if version == 0 { code_size } else { field(16)? as usize };

    let code_pages = (code_size + 4095) / 4096;
    if code_pages == 0 {
        return Err("KEF code size is 0");
    }
    if init_size > code_size || entry_offset >= code_size {
        return Err("Invalid KEF code range or entry point");
    }
    let file_code_end = code_offset
        .checked_add(init_size)
        .filter(|&end| end <= file_data.len())
        .ok_or("KEF file is truncated")?;

    const STACK_PAGES: u64 = 4;
    if args.len() > PAGE_SIZE as usize / 2 {
        return Err("KEF arguments too long");
    }

    let code = &file_data[code_offset..file_code_end];
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

    let entry_point = USER_CODE_BASE + entry_offset as u64;
    crate::println!(
        "[kef] loaded {} bytes at {:#x}, {} frames in use",
        code_size,
        USER_CODE_BASE,
        crate::memory::frames_in_use()
    );

    Ok((entry_point, args_ptr, args_ptr))
}
