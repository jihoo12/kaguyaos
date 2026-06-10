/// Shared filesystem utilities for the shell.
///
/// This eliminates the duplicated "read size, allocate, read data" pattern
/// that was repeated across multiple command handlers.

use alloc::string::String;
use alloc::vec;

/// Read an entire file from the filesystem into a newly-allocated Vec.
///
/// Uses a two-pass approach: first call with an empty buffer to learn the
/// file size, then allocate and read the full contents.
pub fn fs_read_file(filename: &str) -> Result<alloc::vec::Vec<u8>, String> {
    let mut size_buf = [];
    let size = crate::std::fs_read(filename, &mut size_buf)
        .map_err(|e| alloc::format!("Error reading file '{}': {}", filename, e))?;

    let mut data = vec![0u8; size];
    crate::std::fs_read(filename, &mut data)
        .map_err(|e| alloc::format!("Error reading file '{}': {}", filename, e))?;

    Ok(data)
}
