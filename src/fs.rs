#![allow(dead_code)]

use crate::drivers::nvme;

// ============================================================================
// Block-level constants
// ============================================================================

pub const BLOCK_SIZE: usize = 512; // bytes per sector

// ============================================================================
// FAT16 Layout Constants
// ============================================================================

/// Sectors per cluster (4 KB clusters).
pub const SECTORS_PER_CLUSTER: u32 = 8;

/// LBA of the Boot Sector / BPB.
pub const BOOT_SECTOR_LBA: u64 = 0;

/// Number of sectors occupied by the FAT table.
/// 64 sectors × 512 bytes = 32 768 bytes = 16 384 FAT entries (u16 each).
pub const FAT_SECTORS: u32 = 64;

/// LBA where the FAT table begins.
pub const FAT_START_LBA: u64 = 1;

/// Number of sectors reserved for the flat root directory.
/// 16 sectors × 16 entries/sector (32 bytes each) = 256 directory entries.
pub const ROOT_DIR_SECTORS: u32 = 16;
pub const ROOT_DIR_ENTRIES: usize = (ROOT_DIR_SECTORS as usize * BLOCK_SIZE) / 32; // 256

/// LBA where the root directory begins.
pub const ROOT_DIR_START_LBA: u64 = FAT_START_LBA + FAT_SECTORS as u64;

/// LBA where the data clusters begin.
pub const DATA_START_LBA: u64 = ROOT_DIR_START_LBA + ROOT_DIR_SECTORS as u64;

/// Total NVMe capacity: 1 GB = 2 097 152 × 512-byte sectors.
pub const TOTAL_SECTORS: u64 = 2_097_152;

/// Number of available data clusters.
pub const TOTAL_CLUSTERS: u32 =
    ((TOTAL_SECTORS - DATA_START_LBA) / SECTORS_PER_CLUSTER as u64) as u32;

/// FAT entry values.
pub const FAT_ENTRY_FREE: u16 = 0x0000;
pub const FAT_ENTRY_EOC: u16 = 0xFFFF; // End-of-cluster-chain
pub const FAT_ENTRY_RESERVED: u16 = 0xFFF0; // Minimum reserved value

/// Magic number stored in the Boot Sector ("KAGFAT16").
pub const FAT_MAGIC: u64 = 0x4B41_4746_4154_3136;

// ============================================================================
// Error types
// ============================================================================

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

// ============================================================================
// Device readiness
// ============================================================================

pub fn is_ready() -> bool {
    unsafe { nvme::default_nsid().is_some() }
}

pub fn block_size() -> usize {
    BLOCK_SIZE
}

// ============================================================================
// Global Filesystem Lock
// ============================================================================

static FS_LOCK: crate::sync::Spinlock<()> = crate::sync::Spinlock::new(());

// ============================================================================
// On-disk structures
// ============================================================================

/// Boot Sector / BIOS Parameter Block stored at LBA 0.
///
/// Field sizes:
///   magic(8) + bytes_per_sector(2) + sectors_per_cluster(4) +
///   fat_start_lba(4) + fat_sectors(4) + root_dir_start_lba(4) +
///   root_dir_sectors(4) + data_start_lba(4) + total_clusters(4) = 38 bytes
///   padding = 512 - 38 = 474 bytes
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct BootSector {
    /// Magic number to identify a formatted FAT volume.
    pub magic: u64,
    /// Bytes per sector (always 512).
    pub bytes_per_sector: u16,
    /// Sectors per cluster.
    pub sectors_per_cluster: u32,
    /// LBA of the FAT region.
    pub fat_start_lba: u32,
    /// Number of sectors in the FAT region.
    pub fat_sectors: u32,
    /// LBA of the root directory region.
    pub root_dir_start_lba: u32,
    /// Number of sectors in the root directory region.
    pub root_dir_sectors: u32,
    /// LBA where data clusters begin.
    pub data_start_lba: u32,
    /// Total number of data clusters.
    pub total_clusters: u32,
    /// Padding to fill out BLOCK_SIZE (512) bytes.
    pub padding: [u8; 474],
}

const _: () = assert!(
    core::mem::size_of::<BootSector>() == BLOCK_SIZE,
    "BootSector must be exactly BLOCK_SIZE bytes"
);

/// A 32-byte FAT directory entry stored in the root directory region.
/// 16 entries fit in one 512-byte sector.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct FatDirEntry {
    /// Filename, null-terminated, up to 21 bytes.
    pub name: [u8; 22],
    /// First cluster in the FAT chain (0 = no data).
    pub first_cluster: u16,
    /// File size in bytes.
    pub size: u32,
    /// 1 if this slot is in use, 0 if free.
    pub in_use: u8,
    /// Reserved / padding bytes.
    pub reserved: [u8; 3],
}

const _: () = assert!(
    core::mem::size_of::<FatDirEntry>() == 32,
    "FatDirEntry must be exactly 32 bytes"
);

// ============================================================================
// Public file listing type
// ============================================================================

pub struct PublicFileEntry {
    pub name: alloc::string::String,
    pub size: u64,
    pub first_cluster: u16,
}

// ============================================================================
// Raw NVMe block helpers (unlocked)
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
    let nsid = unsafe { nvme::default_nsid().ok_or(FsError::NotReady)? };
    let status = unsafe { nvme::nvme_write(nsid, lba, buffer as *mut u8, count) };
    if status == 0 {
        Ok(())
    } else {
        Err(FsError::DeviceError)
    }
}

// ============================================================================
// Boot Sector helpers (unlocked)
// ============================================================================

fn read_boot_sector_unlocked() -> FsResult<BootSector> {
    if !is_ready() {
        return Err(FsError::NotReady);
    }
    let mut buf = [0u8; BLOCK_SIZE];
    read_block_unlocked(BOOT_SECTOR_LBA, &mut buf)?;
    let bs = unsafe { core::ptr::read_unaligned(buf.as_ptr() as *const BootSector) };
    if bs.magic == FAT_MAGIC {
        Ok(bs)
    } else {
        Err(FsError::NotFormatted)
    }
}

fn write_boot_sector_unlocked(bs: &BootSector) -> FsResult<()> {
    if !is_ready() {
        return Err(FsError::NotReady);
    }
    let mut buf = [0u8; BLOCK_SIZE];
    let bs_bytes =
        unsafe { core::slice::from_raw_parts(bs as *const BootSector as *const u8, BLOCK_SIZE) };
    buf.copy_from_slice(bs_bytes);
    write_block_unlocked(BOOT_SECTOR_LBA, &buf)
}

// ============================================================================
// FAT table helpers (unlocked)
//
// The FAT is stored as a flat array of u16 values beginning at FAT_START_LBA.
// Each u16 entry corresponds to one data cluster:
//   - 0x0000 = free
//   - 0xFFFF = end-of-chain (EOC)
//   - other  = index of next cluster in chain
//
// Cluster indices start at 2 (clusters 0 and 1 are reserved by FAT convention).
// ============================================================================

/// Convert a cluster number to its LBA (first sector of that cluster).
#[inline]
fn cluster_to_lba(cluster: u16) -> u64 {
    DATA_START_LBA + (cluster as u64 - 2) * SECTORS_PER_CLUSTER as u64
}

/// Read the FAT entry for the given cluster.
fn read_fat_entry_unlocked(cluster: u16) -> FsResult<u16> {
    // Each FAT sector holds BLOCK_SIZE/2 = 256 u16 entries.
    let entries_per_sector = (BLOCK_SIZE / 2) as u32;
    let sector_index = (cluster as u32) / entries_per_sector;
    let entry_index = (cluster as u32) % entries_per_sector;

    if sector_index >= FAT_SECTORS {
        return Err(FsError::InvalidArgument);
    }

    let lba = FAT_START_LBA + sector_index as u64;
    let mut buf = [0u8; BLOCK_SIZE];
    read_block_unlocked(lba, &mut buf)?;

    let offset = (entry_index as usize) * 2;
    let value = u16::from_le_bytes([buf[offset], buf[offset + 1]]);
    Ok(value)
}

/// Write the FAT entry for the given cluster.
fn write_fat_entry_unlocked(cluster: u16, value: u16) -> FsResult<()> {
    let entries_per_sector = (BLOCK_SIZE / 2) as u32;
    let sector_index = (cluster as u32) / entries_per_sector;
    let entry_index = (cluster as u32) % entries_per_sector;

    if sector_index >= FAT_SECTORS {
        return Err(FsError::InvalidArgument);
    }

    let lba = FAT_START_LBA + sector_index as u64;
    let mut buf = [0u8; BLOCK_SIZE];
    read_block_unlocked(lba, &mut buf)?;

    let offset = (entry_index as usize) * 2;
    let bytes = value.to_le_bytes();
    buf[offset] = bytes[0];
    buf[offset + 1] = bytes[1];
    write_block_unlocked(lba, &buf)
}

/// Find and allocate one free cluster in the FAT, returning its index.
/// Sets the new cluster's FAT entry to EOC.
fn alloc_cluster_unlocked() -> FsResult<u16> {
    // Clusters are numbered starting at 2 by FAT convention.
    // Maximum usable cluster = 2 + TOTAL_CLUSTERS - 1.
    let max_cluster = (2u32 + TOTAL_CLUSTERS - 1) as u16;

    for cluster in 2u16..=max_cluster {
        let entry = read_fat_entry_unlocked(cluster)?;
        if entry == FAT_ENTRY_FREE {
            write_fat_entry_unlocked(cluster, FAT_ENTRY_EOC)?;
            return Ok(cluster);
        }
    }
    Err(FsError::NoSpace)
}

/// Follow the FAT chain starting at `first_cluster` and free every cluster
/// (set their FAT entries back to FAT_ENTRY_FREE).
fn free_cluster_chain_unlocked(first_cluster: u16) -> FsResult<()> {
    let mut current = first_cluster;
    loop {
        if current < 2 || current >= FAT_ENTRY_RESERVED {
            break;
        }
        let next = read_fat_entry_unlocked(current)?;
        write_fat_entry_unlocked(current, FAT_ENTRY_FREE)?;
        if next >= FAT_ENTRY_RESERVED {
            // EOC or reserved — chain ends here
            break;
        }
        current = next;
    }
    Ok(())
}

// ============================================================================
// Root directory helpers (unlocked)
//
// The root directory is a flat array of FatDirEntry (32 bytes each).
// 16 entries fit in each 512-byte sector; ROOT_DIR_SECTORS sectors → 256 entries.
// ============================================================================

const DIR_ENTRIES_PER_SECTOR: usize = BLOCK_SIZE / 32; // 16

/// Read the directory entry at the given index (0-based).
fn read_dir_entry_unlocked(index: usize) -> FsResult<FatDirEntry> {
    if index >= ROOT_DIR_ENTRIES {
        return Err(FsError::InvalidArgument);
    }
    let sector = index / DIR_ENTRIES_PER_SECTOR;
    let slot = index % DIR_ENTRIES_PER_SECTOR;
    let lba = ROOT_DIR_START_LBA + sector as u64;

    let mut buf = [0u8; BLOCK_SIZE];
    read_block_unlocked(lba, &mut buf)?;

    let offset = slot * 32;
    let entry =
        unsafe { core::ptr::read_unaligned(buf[offset..].as_ptr() as *const FatDirEntry) };
    Ok(entry)
}

/// Write a directory entry at the given index.
fn write_dir_entry_unlocked(index: usize, entry: &FatDirEntry) -> FsResult<()> {
    if index >= ROOT_DIR_ENTRIES {
        return Err(FsError::InvalidArgument);
    }
    let sector = index / DIR_ENTRIES_PER_SECTOR;
    let slot = index % DIR_ENTRIES_PER_SECTOR;
    let lba = ROOT_DIR_START_LBA + sector as u64;

    let mut buf = [0u8; BLOCK_SIZE];
    read_block_unlocked(lba, &mut buf)?;

    let offset = slot * 32;
    let entry_bytes =
        unsafe { core::slice::from_raw_parts(entry as *const FatDirEntry as *const u8, 32) };
    buf[offset..offset + 32].copy_from_slice(entry_bytes);
    write_block_unlocked(lba, &buf)
}

/// Search the root directory for a file by name.
/// Returns `(index, FatDirEntry)` if found.
fn find_file_unlocked(name: &str) -> FsResult<Option<(usize, FatDirEntry)>> {
    let name_bytes = name.as_bytes();
    if name_bytes.is_empty() || name_bytes.len() > 21 {
        return Err(FsError::InvalidArgument);
    }

    for i in 0..ROOT_DIR_ENTRIES {
        let entry = read_dir_entry_unlocked(i)?;
        if entry.in_use == 1 {
            let mut len = 0usize;
            while len < 22 && entry.name[len] != 0 {
                len += 1;
            }
            if &entry.name[..len] == name_bytes {
                return Ok(Some((i, entry)));
            }
        }
    }
    Ok(None)
}

/// Find the first free directory slot.
fn find_free_dir_slot_unlocked() -> FsResult<Option<usize>> {
    for i in 0..ROOT_DIR_ENTRIES {
        let entry = read_dir_entry_unlocked(i)?;
        if entry.in_use == 0 {
            return Ok(Some(i));
        }
    }
    Ok(None)
}

// ============================================================================
// High-level filesystem operations (unlocked)
// ============================================================================

fn format_unlocked() -> FsResult<()> {
    if !is_ready() {
        return Err(FsError::NotReady);
    }

    // 1. Write the Boot Sector
    let bs = BootSector {
        magic: FAT_MAGIC,
        bytes_per_sector: BLOCK_SIZE as u16,
        sectors_per_cluster: SECTORS_PER_CLUSTER,
        fat_start_lba: FAT_START_LBA as u32,
        fat_sectors: FAT_SECTORS,
        root_dir_start_lba: ROOT_DIR_START_LBA as u32,
        root_dir_sectors: ROOT_DIR_SECTORS,
        data_start_lba: DATA_START_LBA as u32,
        total_clusters: TOTAL_CLUSTERS,
        padding: [0u8; 474],
    };
    write_boot_sector_unlocked(&bs)?;

    // 2. Zero the FAT table
    let zero_buf = [0u8; BLOCK_SIZE];
    for i in 0..FAT_SECTORS as u64 {
        write_block_unlocked(FAT_START_LBA + i, &zero_buf)?;
    }

    // Mark clusters 0 and 1 as reserved (FAT convention)
    write_fat_entry_unlocked(0, 0xFFF8)?; // media descriptor in cluster 0
    write_fat_entry_unlocked(1, FAT_ENTRY_EOC)?; // reserved

    // 3. Zero the root directory
    for i in 0..ROOT_DIR_SECTORS as u64 {
        write_block_unlocked(ROOT_DIR_START_LBA + i, &zero_buf)?;
    }

    Ok(())
}

fn create_file_unlocked(name: &str, data: &[u8]) -> FsResult<()> {
    let name_bytes = name.as_bytes();
    if name_bytes.is_empty() || name_bytes.len() > 21 {
        return Err(FsError::InvalidArgument);
    }

    // Validate the volume is formatted
    read_boot_sector_unlocked()?;

    // If the file already exists, delete it first (overwrite semantics)
    if let Some((idx, old_entry)) = find_file_unlocked(name)? {
        // Free the old cluster chain
        if old_entry.first_cluster >= 2 {
            free_cluster_chain_unlocked(old_entry.first_cluster)?;
        }
        // Clear the directory entry
        let empty = FatDirEntry {
            name: [0; 22],
            first_cluster: 0,
            size: 0,
            in_use: 0,
            reserved: [0; 3],
        };
        write_dir_entry_unlocked(idx, &empty)?;
    }

    // Find a free directory slot
    let slot_idx = find_free_dir_slot_unlocked()?.ok_or(FsError::NoSpace)?;

    // Allocate a FAT cluster chain for the file data
    let first_cluster: u16 = if data.is_empty() {
        0 // No data → no clusters needed
    } else {
        let cluster_bytes = (SECTORS_PER_CLUSTER as usize) * BLOCK_SIZE; // bytes per cluster
        let clusters_needed = (data.len() + cluster_bytes - 1) / cluster_bytes;

        // Allocate all clusters first, chaining them together
        let mut prev_cluster: Option<u16> = None;
        let mut first: u16 = 0;

        for i in 0..clusters_needed {
            let c = alloc_cluster_unlocked()?;
            if i == 0 {
                first = c;
            }
            if let Some(prev) = prev_cluster {
                // Point previous cluster to this one
                write_fat_entry_unlocked(prev, c)?;
            }
            prev_cluster = Some(c);

            // Write cluster data
            let src_offset = i * cluster_bytes;
            let src_end = (src_offset + cluster_bytes).min(data.len());
            let chunk = &data[src_offset..src_end];

            // Build a full-cluster buffer (zero-padded)
            let mut cluster_buf = alloc::vec![0u8; cluster_bytes];
            cluster_buf[..chunk.len()].copy_from_slice(chunk);

            let lba = cluster_to_lba(c);
            write_blocks_unlocked(lba, SECTORS_PER_CLUSTER, cluster_buf.as_ptr())?;
        }
        // The last cluster already has EOC from alloc_cluster_unlocked

        first
    };

    // Write the directory entry
    let mut new_entry = FatDirEntry {
        name: [0; 22],
        first_cluster,
        size: data.len() as u32,
        in_use: 1,
        reserved: [0; 3],
    };
    new_entry.name[..name_bytes.len()].copy_from_slice(name_bytes);
    write_dir_entry_unlocked(slot_idx, &new_entry)?;

    Ok(())
}

fn read_file_unlocked(name: &str) -> FsResult<alloc::vec::Vec<u8>> {
    let (_, entry) = find_file_unlocked(name)?.ok_or(FsError::FileNotFound)?;

    let size = entry.size as usize;
    if size == 0 {
        return Ok(alloc::vec::Vec::new());
    }

    let cluster_bytes = (SECTORS_PER_CLUSTER as usize) * BLOCK_SIZE;
    let mut result = alloc::vec![0u8; size];
    let mut written = 0usize;
    let mut current = entry.first_cluster;

    while current >= 2 && current < FAT_ENTRY_RESERVED && written < size {
        let lba = cluster_to_lba(current);
        let remaining = size - written;
        let to_read = remaining.min(cluster_bytes);

        // Read the full cluster into a temp buffer, then copy what we need
        let mut cluster_buf = alloc::vec![0u8; cluster_bytes];
        read_blocks_unlocked(lba, SECTORS_PER_CLUSTER, cluster_buf.as_mut_ptr())?;
        result[written..written + to_read].copy_from_slice(&cluster_buf[..to_read]);
        written += to_read;

        current = read_fat_entry_unlocked(current)?;
    }

    Ok(result)
}

fn delete_file_unlocked(name: &str) -> FsResult<()> {
    let (idx, entry) = find_file_unlocked(name)?.ok_or(FsError::FileNotFound)?;

    // Free the FAT cluster chain
    if entry.first_cluster >= 2 {
        free_cluster_chain_unlocked(entry.first_cluster)?;
    }

    // Clear the directory entry
    let empty = FatDirEntry {
        name: [0; 22],
        first_cluster: 0,
        size: 0,
        in_use: 0,
        reserved: [0; 3],
    };
    write_dir_entry_unlocked(idx, &empty)?;

    Ok(())
}

fn list_files_unlocked() -> FsResult<alloc::vec::Vec<PublicFileEntry>> {
    // Validate the volume is formatted
    read_boot_sector_unlocked()?;

    let mut list = alloc::vec::Vec::new();
    for i in 0..ROOT_DIR_ENTRIES {
        let entry = read_dir_entry_unlocked(i)?;
        if entry.in_use == 1 {
            let mut len = 0usize;
            while len < 22 && entry.name[len] != 0 {
                len += 1;
            }
            let name =
                alloc::string::String::from_utf8_lossy(&entry.name[..len]).into_owned();
            list.push(PublicFileEntry {
                name,
                size: entry.size as u64,
                first_cluster: entry.first_cluster,
            });
        }
    }
    Ok(list)
}

// ============================================================================
// Public locked APIs — raw block I/O (unchanged interface)
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
// Public locked APIs — FAT filesystem operations
// ============================================================================

pub fn format() -> FsResult<()> {
    let _guard = FS_LOCK.lock();
    format_unlocked()
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

pub fn list_files() -> FsResult<alloc::vec::Vec<PublicFileEntry>> {
    let _guard = FS_LOCK.lock();
    list_files_unlocked()
}

// ============================================================================
// Public locked APIs — Boot Sector access
// ============================================================================

pub fn read_boot_sector() -> FsResult<BootSector> {
    let _guard = FS_LOCK.lock();
    read_boot_sector_unlocked()
}

pub fn write_boot_sector(bs: &BootSector) -> FsResult<()> {
    let _guard = FS_LOCK.lock();
    write_boot_sector_unlocked(bs)
}

// ============================================================================
// Public locked APIs — FAT table entry access
// ============================================================================

pub fn read_fat_entry(cluster: u16) -> FsResult<u16> {
    let _guard = FS_LOCK.lock();
    read_fat_entry_unlocked(cluster)
}

pub fn write_fat_entry(cluster: u16, value: u16) -> FsResult<()> {
    let _guard = FS_LOCK.lock();
    write_fat_entry_unlocked(cluster, value)
}

// ============================================================================
// Public locked APIs — Directory entry access
// ============================================================================

pub fn read_dir_entry(index: usize) -> FsResult<FatDirEntry> {
    let _guard = FS_LOCK.lock();
    read_dir_entry_unlocked(index)
}

pub fn write_dir_entry(index: usize, entry: &FatDirEntry) -> FsResult<()> {
    let _guard = FS_LOCK.lock();
    write_dir_entry_unlocked(index, entry)
}

pub fn find_file(name: &str) -> FsResult<Option<(usize, FatDirEntry)>> {
    let _guard = FS_LOCK.lock();
    find_file_unlocked(name)
}
