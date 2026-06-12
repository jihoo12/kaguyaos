#![allow(dead_code)]

use crate::nvme;

pub const BLOCK_SIZE: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsError {
    NotReady,
    InvalidArgument,
    DeviceError,
    NotFormatted,
    NoSpace,
    FileNotFound,
}

impl FsError {
    pub fn code(self) -> i32 {
        match self {
            FsError::NotReady => -1,
            FsError::InvalidArgument => -2,
            FsError::DeviceError => -3,
            FsError::NotFormatted => -4,
            FsError::NoSpace => -5,
            FsError::FileNotFound => -6,
        }
    }
}

pub type FsResult<T> = Result<T, FsError>;

pub fn is_ready() -> bool {
    unsafe { nvme::default_nsid().is_some() }
}

pub fn block_size() -> usize {
    BLOCK_SIZE
}

// Global Filesystem Lock
static FS_LOCK: crate::allocator::Spinlock<()> = crate::allocator::Spinlock::new(());

// ============================================================================
// Unlocked internal helper functions
// ============================================================================

fn read_block_unlocked(lba: u64, buffer: &mut [u8; BLOCK_SIZE]) -> FsResult<()> {
    read_blocks_unlocked(lba, 1, buffer.as_mut_ptr())
}

fn write_block_unlocked(lba: u64, buffer: &[u8; BLOCK_SIZE]) -> FsResult<()> {
    write_blocks_unlocked(lba, 1, buffer.as_ptr())
}

fn read_blocks_unlocked(lba: u64, count: u32, buffer: *mut u8) -> FsResult<()> {
    if count == 0 || buffer.is_null() {
        return Err(FsError::InvalidArgument);
    }

    // Direct calls in user mode are always blocked; callers must use syscalls.
    let nsid = unsafe { nvme::default_nsid().ok_or(FsError::NotReady)? };
    let status = unsafe { nvme::nvme_read(nsid, lba, buffer, count) };
    if status == 0 {
        Ok(())
    } else {
        Err(FsError::DeviceError)
    }
}

fn write_blocks_unlocked(lba: u64, count: u32, buffer: *const u8) -> FsResult<()> {
    if count == 0 || buffer.is_null() {
        return Err(FsError::InvalidArgument);
    }

    // Direct calls in user mode are always blocked; callers must use syscalls.
    let nsid = unsafe { nvme::default_nsid().ok_or(FsError::NotReady)? };
    let status = unsafe { nvme::nvme_write(nsid, lba, buffer as *mut u8, count) };
    if status == 0 {
        Ok(())
    } else {
        Err(FsError::DeviceError)
    }
}

fn read_superblock_unlocked() -> FsResult<Superblock> {
    if !is_ready() {
        return Err(FsError::NotReady);
    }
    let mut buf = [0u8; BLOCK_SIZE];
    read_block_unlocked(SUPERBLOCK_LBA, &mut buf)?;
    let sb = unsafe { core::ptr::read_unaligned(buf.as_ptr() as *const Superblock) };
    if sb.magic == SFS_MAGIC {
        Ok(sb)
    } else {
        Err(FsError::NotFormatted)
    }
}

fn write_superblock_unlocked(sb: &Superblock) -> FsResult<()> {
    if !is_ready() {
        return Err(FsError::NotReady);
    }
    let mut buf = [0u8; BLOCK_SIZE];
    let sb_bytes = unsafe {
        core::slice::from_raw_parts(sb as *const Superblock as *const u8, BLOCK_SIZE)
    };
    buf.copy_from_slice(sb_bytes);
    write_block_unlocked(SUPERBLOCK_LBA, &buf)
}

fn write_directory_entry_unlocked(index: usize, entry: &FileEntry) -> FsResult<()> {
    if index >= MAX_FILES {
        return Err(FsError::InvalidArgument);
    }
    let block_offset = index / 8;
    let entry_offset = (index % 8) * 64;
    let lba = DIR_START_LBA + (block_offset as u64);

    let mut buf = [0u8; BLOCK_SIZE];
    read_block_unlocked(lba, &mut buf)?;

    let entry_bytes = unsafe {
        core::slice::from_raw_parts(entry as *const FileEntry as *const u8, 64)
    };
    buf[entry_offset..entry_offset + 64].copy_from_slice(entry_bytes);
    write_block_unlocked(lba, &buf)
}

fn find_file_unlocked(name: &str) -> FsResult<Option<(usize, FileEntry)>> {
    let name_bytes = name.as_bytes();
    if name_bytes.is_empty() || name_bytes.len() > 46 {
        return Err(FsError::InvalidArgument);
    }

    let mut buf = [0u8; BLOCK_SIZE];
    for b in 0..DIR_BLOCKS {
        read_block_unlocked(DIR_START_LBA + b, &mut buf)?;
        for i in 0..8 {
            let offset = i * 64;
            let entry = unsafe {
                core::ptr::read_unaligned(buf[offset..].as_ptr() as *const FileEntry)
            };
            if entry.in_use == 1 {
                let mut len = 0;
                while len < 47 && entry.name[len] != 0 {
                    len += 1;
                }
                if &entry.name[..len] == name_bytes {
                    let index = (b as usize) * 8 + i;
                    return Ok(Some((index, entry)));
                }
            }
        }
    }
    Ok(None)
}

fn find_free_entry_unlocked() -> FsResult<Option<usize>> {
    let mut buf = [0u8; BLOCK_SIZE];
    for b in 0..DIR_BLOCKS {
        read_block_unlocked(DIR_START_LBA + b, &mut buf)?;
        for i in 0..8 {
            let offset = i * 64;
            let entry = unsafe {
                core::ptr::read_unaligned(buf[offset..].as_ptr() as *const FileEntry)
            };
            if entry.in_use == 0 {
                let index = (b as usize) * 8 + i;
                return Ok(Some(index));
            }
        }
    }
    Ok(None)
}

fn format_unlocked() -> FsResult<()> {
    if !is_ready() {
        return Err(FsError::NotReady);
    }

    // Initialize superblock
    let sb = Superblock {
        magic: SFS_MAGIC,
        next_free_block: DATA_START_LBA,
        file_count: 0,
        padding: [0; 492],
    };
    write_superblock_unlocked(&sb)?;

    // Zero out directory blocks
    let zero_buf = [0u8; BLOCK_SIZE];
    for b in 0..DIR_BLOCKS {
        write_block_unlocked(DIR_START_LBA + b, &zero_buf)?;
    }

    Ok(())
}

fn create_file_unlocked(name: &str, data: &[u8]) -> FsResult<()> {
    let name_bytes = name.as_bytes();
    if name_bytes.is_empty() || name_bytes.len() > 46 {
        return Err(FsError::InvalidArgument);
    }

    let mut sb = read_superblock_unlocked()?;

    // Check if the file already exists. If so, delete it first to overwrite.
    if let Some((idx, _old_entry)) = find_file_unlocked(name)? {
        delete_file_at_unlocked(idx)?;
        // Reload superblock after deletion
        sb = read_superblock_unlocked()?;
    }

    // Find a free directory entry
    let free_idx = find_free_entry_unlocked()?.ok_or(FsError::NoSpace)?;

    // Calculate blocks needed
    let blocks_needed = (data.len() + BLOCK_SIZE - 1) / BLOCK_SIZE;

    // Check capacity bounds
    let start_block = sb.next_free_block;
    if start_block + blocks_needed as u64 > MAX_BLOCKS {
        return Err(FsError::NoSpace);
    }

    if blocks_needed > 0 {
        // Write data in a single contiguous operation
        let mut write_buf = alloc::vec![0u8; blocks_needed * BLOCK_SIZE];
        write_buf[..data.len()].copy_from_slice(data);
        write_blocks_unlocked(start_block, blocks_needed as u32, write_buf.as_ptr())?;
    }

    // Construct the new directory entry
    let mut new_entry = FileEntry {
        name: [0; 47],
        start_block,
        size: data.len() as u64,
        in_use: 1,
    };
    new_entry.name[..name_bytes.len()].copy_from_slice(name_bytes);

    // Save the new directory entry
    write_directory_entry_unlocked(free_idx, &new_entry)?;

    // Update the superblock
    sb.next_free_block += blocks_needed as u64;
    sb.file_count += 1;
    write_superblock_unlocked(&sb)?;

    Ok(())
}

fn read_file_unlocked(name: &str) -> FsResult<alloc::vec::Vec<u8>> {
    if let Some((_idx, entry)) = find_file_unlocked(name)? {
        let size = entry.size as usize;
        if size == 0 {
            return Ok(alloc::vec::Vec::new());
        }

        let blocks_to_read = (size + BLOCK_SIZE - 1) / BLOCK_SIZE;
        let mut buf = alloc::vec![0u8; blocks_to_read * BLOCK_SIZE];
        read_blocks_unlocked(entry.start_block, blocks_to_read as u32, buf.as_mut_ptr())?;
        buf.truncate(size);
        Ok(buf)
    } else {
        Err(FsError::FileNotFound)
    }
}

fn delete_file_unlocked(name: &str) -> FsResult<()> {
    if let Some((idx, _entry)) = find_file_unlocked(name)? {
        delete_file_at_unlocked(idx)
    } else {
        Err(FsError::FileNotFound)
    }
}

fn delete_file_at_unlocked(index: usize) -> FsResult<()> {
    // 1. Read the target file entry to find its size and start block.
    let block_offset = index / 8;
    let entry_offset = (index % 8) * 64;
    let lba = DIR_START_LBA + (block_offset as u64);
    let mut buf = [0u8; BLOCK_SIZE];
    read_block_unlocked(lba, &mut buf)?;
    let entry = unsafe {
        core::ptr::read_unaligned(buf[entry_offset..].as_ptr() as *const FileEntry)
    };

    if entry.in_use == 0 {
        return Ok(());
    }

    let deleted_start = entry.start_block;
    let reclaimed_blocks = (entry.size + BLOCK_SIZE as u64 - 1) / BLOCK_SIZE as u64;

    // 2. Clear the directory entry slot
    let zero_entry = FileEntry {
        name: [0; 47],
        start_block: 0,
        size: 0,
        in_use: 0,
    };
    let entry_bytes = unsafe {
        core::slice::from_raw_parts(&zero_entry as *const FileEntry as *const u8, 64)
    };
    buf[entry_offset..entry_offset + 64].copy_from_slice(entry_bytes);
    write_block_unlocked(lba, &buf)?;

    // Update superblock file count
    let mut sb = read_superblock_unlocked()?;
    if sb.file_count > 0 {
        sb.file_count -= 1;
        write_superblock_unlocked(&sb)?;
    }

    // 3. Compact files that are after deleted_start
    if reclaimed_blocks > 0 {
        // Collect all active entries with start_block > deleted_start
        let mut active_entries = alloc::vec::Vec::new();
        for idx in 0..MAX_FILES {
            let b_offset = idx / 8;
            let e_offset = (idx % 8) * 64;
            let dir_lba = DIR_START_LBA + (b_offset as u64);
            let mut dir_buf = [0u8; BLOCK_SIZE];
            read_block_unlocked(dir_lba, &mut dir_buf)?;
            let ent = unsafe {
                core::ptr::read_unaligned(dir_buf[e_offset..].as_ptr() as *const FileEntry)
            };
            if ent.in_use == 1 && ent.start_block > deleted_start {
                active_entries.push((idx, ent.start_block, ent.size));
            }
        }

        // Sort them by start_block ascending to copy them safely from left to right (low to high addresses)
        active_entries.sort_by_key(|&(_, start_block, _)| start_block);

        // Shift blocks for each file and update directory entries
        for (idx, start_block, size) in active_entries {
            let file_blocks = (size + BLOCK_SIZE as u64 - 1) / BLOCK_SIZE as u64;
            let new_start = start_block - reclaimed_blocks;

            // Copy blocks one-by-one in ascending order
            for i in 0..file_blocks {
                let mut temp_buf = [0u8; BLOCK_SIZE];
                read_block_unlocked(start_block + i, &mut temp_buf)?;
                write_block_unlocked(new_start + i, &temp_buf)?;
            }

            // Update entry start_block
            let b_offset = idx / 8;
            let e_offset = (idx % 8) * 64;
            let dir_lba = DIR_START_LBA + (b_offset as u64);
            let mut dir_buf = [0u8; BLOCK_SIZE];
            read_block_unlocked(dir_lba, &mut dir_buf)?;
            let mut ent = unsafe {
                core::ptr::read_unaligned(dir_buf[e_offset..].as_ptr() as *const FileEntry)
            };
            ent.start_block = new_start;
            let ent_bytes = unsafe {
                core::slice::from_raw_parts(&ent as *const FileEntry as *const u8, 64)
            };
            dir_buf[e_offset..e_offset + 64].copy_from_slice(ent_bytes);
            write_block_unlocked(dir_lba, &dir_buf)?;
        }

        // 4. Update next_free_block in Superblock
        let mut sb = read_superblock_unlocked()?;
        sb.next_free_block -= reclaimed_blocks;
        write_superblock_unlocked(&sb)?;
    }

    Ok(())
}

fn list_files_unlocked() -> FsResult<alloc::vec::Vec<PublicFileEntry>> {
    let _sb = read_superblock_unlocked()?; // Ensure SFS is formatted
    let mut list = alloc::vec::Vec::new();
    let mut buf = [0u8; BLOCK_SIZE];
    for b in 0..DIR_BLOCKS {
        read_block_unlocked(DIR_START_LBA + b, &mut buf)?;
        for i in 0..8 {
            let offset = i * 64;
            let entry = unsafe {
                core::ptr::read_unaligned(buf[offset..].as_ptr() as *const FileEntry)
            };
            if entry.in_use == 1 {
                let mut len = 0;
                while len < 47 && entry.name[len] != 0 {
                    len += 1;
                }
                let name = alloc::string::String::from_utf8_lossy(&entry.name[..len]).into_owned();
                list.push(PublicFileEntry {
                    name,
                    size: entry.size,
                    start_block: entry.start_block,
                });
            }
        }
    }
    Ok(list)
}

// ============================================================================
// Public locked APIs
// ============================================================================

pub fn read_block(lba: u64, buffer: &mut [u8; BLOCK_SIZE]) -> FsResult<()> {
    let _guard = FS_LOCK.lock();
    read_block_unlocked(lba, buffer)
}

pub fn write_block(lba: u64, buffer: &[u8; BLOCK_SIZE]) -> FsResult<()> {
    let _guard = FS_LOCK.lock();
    write_block_unlocked(lba, buffer)
}

pub fn read_blocks(lba: u64, count: u32, buffer: *mut u8) -> FsResult<()> {
    let _guard = FS_LOCK.lock();
    read_blocks_unlocked(lba, count, buffer)
}

pub fn write_blocks(lba: u64, count: u32, buffer: *const u8) -> FsResult<()> {
    let _guard = FS_LOCK.lock();
    write_blocks_unlocked(lba, count, buffer)
}

// ============================================================================
// SimpleFS Structures & Constants
// ============================================================================

pub const SUPERBLOCK_LBA: u64 = 0;
pub const DIR_START_LBA: u64 = 1;
pub const DIR_BLOCKS: u64 = 16;
pub const DATA_START_LBA: u64 = 1 + DIR_BLOCKS; // 17
pub const MAX_FILES: usize = (DIR_BLOCKS as usize) * 8; // 128 (8 entries of 64 bytes per block)
pub const SFS_MAGIC: u64 = 0x5349_4d50_4c45_4653; // "SIMPLEFS" in ASCII
pub const MAX_BLOCKS: u64 = 2_097_152; // 1GB NVMe capacity (2097152 blocks of 512 bytes)

#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct Superblock {
    pub magic: u64,           // Should match SFS_MAGIC
    pub next_free_block: u64, // The next block where file content can be written
    pub file_count: u32,      // Number of active files
    pub padding: [u8; 492],   // Pad to BLOCK_SIZE (512 bytes)
}

#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct FileEntry {
    pub name: [u8; 47],       // Filename, null-terminated
    pub start_block: u64,     // The start block in NVMe
    pub size: u64,            // The file size in bytes
    pub in_use: u8,           // 1 if active, 0 if free
}

// ============================================================================
// SimpleFS Operations
// ============================================================================

pub fn read_superblock() -> FsResult<Superblock> {
    let _guard = FS_LOCK.lock();
    read_superblock_unlocked()
}

pub fn write_superblock(sb: &Superblock) -> FsResult<()> {
    let _guard = FS_LOCK.lock();
    write_superblock_unlocked(sb)
}

pub fn format() -> FsResult<()> {
    let _guard = FS_LOCK.lock();
    format_unlocked()
}

pub fn write_directory_entry(index: usize, entry: &FileEntry) -> FsResult<()> {
    let _guard = FS_LOCK.lock();
    write_directory_entry_unlocked(index, entry)
}

pub fn find_file(name: &str) -> FsResult<Option<(usize, FileEntry)>> {
    let _guard = FS_LOCK.lock();
    find_file_unlocked(name)
}

pub fn find_free_entry() -> FsResult<Option<usize>> {
    let _guard = FS_LOCK.lock();
    find_free_entry_unlocked()
}

pub fn create_file(name: &str, data: &[u8]) -> FsResult<()> {
    let _guard = FS_LOCK.lock();
    create_file_unlocked(name, data)
}

pub fn read_file(name: &str) -> FsResult<alloc::vec::Vec<u8>> {
    let _guard = FS_LOCK.lock();
    read_file_unlocked(name)
}

pub fn delete_file(name: &str) -> FsResult<()> {
    let _guard = FS_LOCK.lock();
    delete_file_unlocked(name)
}

fn delete_file_at(index: usize) -> FsResult<()> {
    let _guard = FS_LOCK.lock();
    delete_file_at_unlocked(index)
}

pub struct PublicFileEntry {
    pub name: alloc::string::String,
    pub size: u64,
    pub start_block: u64,
}

pub fn list_files() -> FsResult<alloc::vec::Vec<PublicFileEntry>> {
    let _guard = FS_LOCK.lock();
    list_files_unlocked()
}
