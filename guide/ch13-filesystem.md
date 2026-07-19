# Chapter 13: FAT16 Filesystem

With the NVMe driver working, we have a block device. Now we need a filesystem to store and retrieve files — KEF programs, data, anything the user wants to persist.

## Why FAT16?

FAT16 is simple, well-documented, and understood by virtually every OS and tool. The trade-off is a 2 GB volume size limit, which is fine for a hobby OS.

## Disk Layout

```
Sector 0:     Boot Sector (BPB — BIOS Parameter Block)
Sectors 1-64: FAT (File Allocation Table) — 16K entries
Sectors 65-80: Root Directory — 256 directory entries (32 bytes each)
Sectors 81+:  Data Clusters — file content
```

Each cluster is 8 sectors = 4096 bytes (one page). The FAT maps each cluster to its next cluster (or `0xFFFF` for end-of-chain).

## The Boot Sector

The BPB describes the filesystem geometry:

```rust
// src/fs.rs (simplified constants)
const BLOCK_SIZE: usize = 512;
const SECTORS_PER_CLUSTER: u16 = 8;     // 4 KiB clusters
const FAT_SECTORS: u16 = 64;            // 16K FAT entries
const ROOT_DIR_SECTORS: u16 = 16;       // 256 directory entries
const FAT_START_LBA: u64 = 1;
const ROOT_DIR_START_LBA: u64 = FAT_START_LBA + FAT_SECTORS as u64;
const DATA_START_LBA: u64 = ROOT_DIR_START_LBA + ROOT_DIR_SECTORS as u64;
```

## Reading Files

To read a file:

1. **Scan the root directory** for the filename (32-byte entries, 8.3 format)
2. **Follow the FAT chain** starting from the entry's first cluster
3. **Read each cluster** from the data region via NVMe

```rust
// src/fs.rs (simplified)
pub fn read_file(name: &str) -> Result<Vec<u8>, FsError> {
    // 1. Find directory entry
    let entry = find_file(name)?.ok_or(FsError::NotFound)?;

    // 2. Follow FAT chain
    let mut data = Vec::new();
    let mut cluster = entry.first_cluster;

    while cluster < 0xFFF0 {
        // 3. Read cluster from NVMe
        let lba = cluster_to_lba(cluster);
        let sector_data = nvme_read_sectors(lba, SECTORS_PER_CLUSTER as u64);
        data.extend_from_slice(&sector_data);

        // 4. Follow chain
        cluster = fat_read(cluster)?;
    }

    Ok(data)
}
```

## Writing Files

Writing is more complex — you must:

1. Find or create a directory entry
2. Allocate free clusters
3. Write the FAT chain
4. Write the data

```rust
// src/fs.rs (simplified)
pub fn create_file(name: &str, content: &[u8]) -> Result<(), FsError> {
    // 1. Find free directory entry
    let dir_entry = find_free_entry()?;

    // 2. Allocate clusters
    let mut prev_cluster = 0xFFFFu16;
    let mut remaining = content;

    while !remaining.is_empty() {
        let cluster = alloc_free_cluster()?;

        // Link previous cluster to this one
        if prev_cluster != 0xFFFF {
            fat_write(prev_cluster, cluster)?;
        }

        // Write data to cluster
        let lba = cluster_to_lba(cluster);
        let chunk = &remaining[..min(remaining.len(), 4096)];
        nvme_write_sectors(lba, chunk, SECTORS_PER_CLUSTER as u64);

        remaining = &remaining[chunk.len()..];
        prev_cluster = cluster;
    }

    // 3. Update directory entry
    dir_entry.name = *name.as_bytes();
    dir_entry.first_cluster = /* first allocated cluster */;
    dir_entry.size = content.len() as u32;

    Ok(())
}
```

## Formatting

kaguyaOS can format the NVMe volume as FAT16:

```rust
pub fn format() -> Result<(), FsError> {
    // 1. Write boot sector (BPB)
    let bpb = create_bpb();
    nvme_write_sector(0, &bpb)?;

    // 2. Clear FAT (all entries = 0)
    let empty_fat = [0u8; 512 * FAT_SECTORS as usize];
    nvme_write_sectors(FAT_START_LBA, &empty_fat)?;

    // 3. Clear root directory
    let empty_dir = [0u8; 512 * ROOT_DIR_SECTORS as usize];
    nvme_write_sectors(ROOT_DIR_START_LBA, &empty_dir)?;

    // 4. Mark reserved clusters (0 and 1) as used
    fat_write(0, 0xFFF8)?;
    fat_write(1, 0xFFFF)?;

    Ok(())
}
```

## File System Lock

All filesystem operations go through a lock to prevent concurrent access:

```rust
// src/fs.rs:99
static FS_LOCK: crate::sync::Spinlock<()> = crate::sync::Spinlock::new(());
```

Every public function acquires this lock first.

## kaguyaOS Reference

| File | Lines | What it does |
|------|-------|-------------|
| `src/fs.rs` | 1-713 | Full FAT16 implementation |

---

**Next:** [Chapter 14 — Networking](ch14-networking.md) — Ethernet, ARP, and ICMP ping.
