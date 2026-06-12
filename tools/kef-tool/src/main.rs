use std::fs::OpenOptions;
use std::io::{Read, Write, Seek, SeekFrom};

// ============================================================================
// FAT16 Layout Constants (from src/fs.rs)
// ============================================================================

const SECTORS_PER_CLUSTER: u32 = 8;
const BLOCK_SIZE: usize = 512;
const FAT_SECTORS: u32 = 64;
const FAT_START_LBA: u64 = 1;
const ROOT_DIR_SECTORS: u32 = 16;
const ROOT_DIR_START_LBA: u64 = 65;
const DATA_START_LBA: u64 = 81;
const ROOT_DIR_ENTRIES: usize = 256;

const FAT_ENTRY_FREE: u16 = 0x0000;
const FAT_ENTRY_EOC: u16 = 0xFFFF;
const FAT_ENTRY_RESERVED: u16 = 0xFFF0;

const FAT_MAGIC: u64 = 0x4B41_4746_4154_3136; // "KAGFAT16"

// ============================================================================
// On-disk structures
// ============================================================================

struct BootSector {
    magic: u64,
    bytes_per_sector: u16,
    sectors_per_cluster: u32,
    fat_start_lba: u32,
    fat_sectors: u32,
    root_dir_start_lba: u32,
    root_dir_sectors: u32,
    data_start_lba: u32,
    total_clusters: u32,
}

impl BootSector {
    fn from_bytes(bytes: &[u8; 512]) -> Self {
        Self {
            magic: u64::from_le_bytes(bytes[0..8].try_into().unwrap()),
            bytes_per_sector: u16::from_le_bytes(bytes[8..10].try_into().unwrap()),
            sectors_per_cluster: u32::from_le_bytes(bytes[10..14].try_into().unwrap()),
            fat_start_lba: u32::from_le_bytes(bytes[14..18].try_into().unwrap()),
            fat_sectors: u32::from_le_bytes(bytes[18..22].try_into().unwrap()),
            root_dir_start_lba: u32::from_le_bytes(bytes[22..26].try_into().unwrap()),
            root_dir_sectors: u32::from_le_bytes(bytes[26..30].try_into().unwrap()),
            data_start_lba: u32::from_le_bytes(bytes[30..34].try_into().unwrap()),
            total_clusters: u32::from_le_bytes(bytes[34..38].try_into().unwrap()),
        }
    }

    fn to_bytes(&self) -> [u8; 512] {
        let mut bytes = [0u8; 512];
        bytes[0..8].copy_from_slice(&self.magic.to_le_bytes());
        bytes[8..10].copy_from_slice(&self.bytes_per_sector.to_le_bytes());
        bytes[10..14].copy_from_slice(&self.sectors_per_cluster.to_le_bytes());
        bytes[14..18].copy_from_slice(&self.fat_start_lba.to_le_bytes());
        bytes[18..22].copy_from_slice(&self.fat_sectors.to_le_bytes());
        bytes[22..26].copy_from_slice(&self.root_dir_start_lba.to_le_bytes());
        bytes[26..30].copy_from_slice(&self.root_dir_sectors.to_le_bytes());
        bytes[30..34].copy_from_slice(&self.data_start_lba.to_le_bytes());
        bytes[34..38].copy_from_slice(&self.total_clusters.to_le_bytes());
        bytes
    }
}

struct FatDirEntry {
    name: [u8; 22],
    first_cluster: u16,
    size: u32,
    in_use: u8,
}

impl FatDirEntry {
    fn from_bytes(bytes: &[u8; 32]) -> Self {
        Self {
            name: bytes[0..22].try_into().unwrap(),
            first_cluster: u16::from_le_bytes(bytes[22..24].try_into().unwrap()),
            size: u32::from_le_bytes(bytes[24..28].try_into().unwrap()),
            in_use: bytes[28],
        }
    }

    fn to_bytes(&self) -> [u8; 32] {
        let mut bytes = [0u8; 32];
        bytes[0..22].copy_from_slice(&self.name);
        bytes[22..24].copy_from_slice(&self.first_cluster.to_le_bytes());
        bytes[24..28].copy_from_slice(&self.size.to_le_bytes());
        bytes[28] = self.in_use;
        bytes
    }
}

// ============================================================================
// Disk abstraction
// ============================================================================

struct Disk {
    file: std::fs::File,
}

impl Disk {
    fn open(path: &str) -> std::io::Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)?;
        Ok(Self { file })
    }

    fn read_block(&mut self, lba: u64, buf: &mut [u8; 512]) -> std::io::Result<()> {
        self.file.seek(SeekFrom::Start(lba * 512))?;
        self.file.read_exact(buf)?;
        Ok(())
    }

    fn write_block(&mut self, lba: u64, buf: &[u8; 512]) -> std::io::Result<()> {
        self.file.seek(SeekFrom::Start(lba * 512))?;
        self.file.write_all(buf)?;
        Ok(())
    }

    fn write_blocks(&mut self, lba: u64, _count: u32, buf: &[u8]) -> std::io::Result<()> {
        self.file.seek(SeekFrom::Start(lba * 512))?;
        self.file.write_all(buf)?;
        Ok(())
    }
}

// ============================================================================
// Core filesystem helper logic
// ============================================================================

fn cluster_to_lba(cluster: u16) -> u64 {
    DATA_START_LBA + (cluster as u64 - 2) * SECTORS_PER_CLUSTER as u64
}

fn read_fat_entry(disk: &mut Disk, cluster: u16) -> std::io::Result<u16> {
    let entries_per_sector = (BLOCK_SIZE / 2) as u32;
    let sector_index = (cluster as u32) / entries_per_sector;
    let entry_index = (cluster as u32) % entries_per_sector;

    let lba = FAT_START_LBA + sector_index as u64;
    let mut buf = [0u8; 512];
    disk.read_block(lba, &mut buf)?;

    let offset = (entry_index as usize) * 2;
    let value = u16::from_le_bytes([buf[offset], buf[offset + 1]]);
    Ok(value)
}

fn write_fat_entry(disk: &mut Disk, cluster: u16, value: u16) -> std::io::Result<()> {
    let entries_per_sector = (BLOCK_SIZE / 2) as u32;
    let sector_index = (cluster as u32) / entries_per_sector;
    let entry_index = (cluster as u32) % entries_per_sector;

    let lba = FAT_START_LBA + sector_index as u64;
    let mut buf = [0u8; 512];
    disk.read_block(lba, &mut buf)?;

    let offset = (entry_index as usize) * 2;
    let bytes = value.to_le_bytes();
    buf[offset] = bytes[0];
    buf[offset + 1] = bytes[1];
    disk.write_block(lba, &mut buf)?;
    Ok(())
}

fn alloc_cluster(disk: &mut Disk, total_clusters: u32) -> std::io::Result<u16> {
    let max_cluster = (2 + total_clusters - 1) as u16;
    for cluster in 2..=max_cluster {
        let entry = read_fat_entry(disk, cluster)?;
        if entry == FAT_ENTRY_FREE {
            write_fat_entry(disk, cluster, FAT_ENTRY_EOC)?;
            return Ok(cluster);
        }
    }
    Err(std::io::Error::new(std::io::ErrorKind::WriteZero, "No free clusters available (Disk Full)"))
}

fn free_cluster_chain(disk: &mut Disk, first_cluster: u16) -> std::io::Result<()> {
    let mut current = first_cluster;
    loop {
        if current < 2 || current >= FAT_ENTRY_RESERVED {
            break;
        }
        let next = read_fat_entry(disk, current)?;
        write_fat_entry(disk, current, FAT_ENTRY_FREE)?;
        if next >= FAT_ENTRY_RESERVED {
            break;
        }
        current = next;
    }
    Ok(())
}

fn read_dir_entry(disk: &mut Disk, index: usize) -> std::io::Result<FatDirEntry> {
    let sector = index / 16;
    let slot = index % 16;
    let lba = ROOT_DIR_START_LBA + sector as u64;

    let mut buf = [0u8; 512];
    disk.read_block(lba, &mut buf)?;

    let offset = slot * 32;
    let mut entry_buf = [0u8; 32];
    entry_buf.copy_from_slice(&buf[offset..offset + 32]);
    Ok(FatDirEntry::from_bytes(&entry_buf))
}

fn write_dir_entry(disk: &mut Disk, index: usize, entry: &FatDirEntry) -> std::io::Result<()> {
    let sector = index / 16;
    let slot = index % 16;
    let lba = ROOT_DIR_START_LBA + sector as u64;

    let mut buf = [0u8; 512];
    disk.read_block(lba, &mut buf)?;

    let offset = slot * 32;
    buf[offset..offset + 32].copy_from_slice(&entry.to_bytes());
    disk.write_block(lba, &buf)?;
    Ok(())
}

fn find_file(disk: &mut Disk, name: &str) -> std::io::Result<Option<(usize, FatDirEntry)>> {
    let name_bytes = name.as_bytes();
    if name_bytes.is_empty() || name_bytes.len() > 21 {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "Filename must be 1 to 21 bytes"));
    }

    for i in 0..ROOT_DIR_ENTRIES {
        let entry = read_dir_entry(disk, i)?;
        if entry.in_use == 1 {
            let mut len = 0;
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

fn delete_file(disk: &mut Disk, name: &str) -> std::io::Result<()> {
    if let Some((idx, entry)) = find_file(disk, name)? {
        if entry.first_cluster >= 2 {
            free_cluster_chain(disk, entry.first_cluster)?;
        }
        let empty = FatDirEntry {
            name: [0; 22],
            first_cluster: 0,
            size: 0,
            in_use: 0,
        };
        write_dir_entry(disk, idx, &empty)?;
    }
    Ok(())
}

fn create_file(disk: &mut Disk, name: &str, data: &[u8], total_clusters: u32) -> std::io::Result<()> {
    let name_bytes = name.as_bytes();
    if name_bytes.is_empty() || name_bytes.len() > 21 {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "Filename must be 1 to 21 bytes"));
    }

    // Delete existing file first to overwrite
    delete_file(disk, name)?;

    // Find a free root directory entry
    let mut slot_idx = None;
    for i in 0..ROOT_DIR_ENTRIES {
        let entry = read_dir_entry(disk, i)?;
        if entry.in_use == 0 {
            slot_idx = Some(i);
            break;
        }
    }
    let slot_idx = slot_idx.ok_or_else(|| std::io::Error::new(std::io::ErrorKind::WriteZero, "No free directory slots left"))?;

    // Allocate clusters and write data
    let first_cluster = if data.is_empty() {
        0
    } else {
        let cluster_bytes = (SECTORS_PER_CLUSTER as usize) * BLOCK_SIZE; // 4096 bytes
        let clusters_needed = (data.len() + cluster_bytes - 1) / cluster_bytes;

        let mut prev_cluster: Option<u16> = None;
        let mut first = 0;

        for i in 0..clusters_needed {
            let c = alloc_cluster(disk, total_clusters)?;
            if i == 0 {
                first = c;
            }
            if let Some(prev) = prev_cluster {
                write_fat_entry(disk, prev, c)?;
            }
            prev_cluster = Some(c);

            let src_offset = i * cluster_bytes;
            let src_end = (src_offset + cluster_bytes).min(data.len());
            let chunk = &data[src_offset..src_end];

            let mut cluster_buf = vec![0u8; cluster_bytes];
            cluster_buf[..chunk.len()].copy_from_slice(chunk);

            let lba = cluster_to_lba(c);
            disk.write_blocks(lba, SECTORS_PER_CLUSTER, &cluster_buf)?;
        }
        first
    };

    // Write directory entry
    let mut new_entry = FatDirEntry {
        name: [0; 22],
        first_cluster,
        size: data.len() as u32,
        in_use: 1,
    };
    new_entry.name[..name_bytes.len()].copy_from_slice(name_bytes);
    write_dir_entry(disk, slot_idx, &new_entry)?;
    Ok(())
}

fn format_disk(disk: &mut Disk) -> std::io::Result<()> {
    // We assume 1GB default size = 2,097,152 sectors
    let total_sectors: u64 = 2_097_152;
    let total_clusters = ((total_sectors - DATA_START_LBA) / SECTORS_PER_CLUSTER as u64) as u32;

    let bs = BootSector {
        magic: FAT_MAGIC,
        bytes_per_sector: 512,
        sectors_per_cluster: SECTORS_PER_CLUSTER,
        fat_start_lba: FAT_START_LBA as u32,
        fat_sectors: FAT_SECTORS,
        root_dir_start_lba: ROOT_DIR_START_LBA as u32,
        root_dir_sectors: ROOT_DIR_SECTORS,
        data_start_lba: DATA_START_LBA as u32,
        total_clusters,
    };

    disk.write_block(0, &bs.to_bytes())?;

    // Zero out the FAT table
    let zero_buf = [0u8; 512];
    for i in 0..FAT_SECTORS as u64 {
        disk.write_block(FAT_START_LBA + i, &zero_buf)?;
    }

    // Reserved entries 0 and 1
    write_fat_entry(disk, 0, 0xFFF8)?;
    write_fat_entry(disk, 1, FAT_ENTRY_EOC)?;

    // Zero out root directory entries
    for i in 0..ROOT_DIR_SECTORS as u64 {
        disk.write_block(ROOT_DIR_START_LBA + i, &zero_buf)?;
    }

    Ok(())
}

fn list_files(disk: &mut Disk) -> std::io::Result<()> {
    // First read boot sector to verify
    let mut boot_buf = [0u8; 512];
    disk.read_block(0, &mut boot_buf)?;
    let bs = BootSector::from_bytes(&boot_buf);
    if bs.magic != FAT_MAGIC {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Disk is not formatted with KAGFAT16"));
    }

    println!("{:<22} {:<12} {:<10}", "Filename", "Size (bytes)", "First Cluster");
    println!("{}", "-".repeat(48));

    let mut count = 0;
    for i in 0..ROOT_DIR_ENTRIES {
        let entry = read_dir_entry(disk, i)?;
        if entry.in_use == 1 {
            let mut len = 0;
            while len < 22 && entry.name[len] != 0 {
                len += 1;
            }
            let name = String::from_utf8_lossy(&entry.name[..len]);
            println!("{:<22} {:<12} {:<10}", name, entry.size, entry.first_cluster);
            count += 1;
        }
    }
    println!("\nTotal files: {}", count);
    Ok(())
}

// ============================================================================
// Main Execution
// ============================================================================

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        print_usage();
        std::process::exit(1);
    }

    let cmd = &args[1];
    let img_path = &args[2];

    let mut disk = match Disk::open(img_path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("Error opening disk image '{}': {}", img_path, e);
            std::process::exit(1);
        }
    };

    match cmd.as_str() {
        "format" => {
            if let Err(e) = format_disk(&mut disk) {
                eprintln!("Error formatting disk: {}", e);
                std::process::exit(1);
            }
            println!("Disk formatted successfully with KAGFAT16 layout.");
        }
        "list" => {
            if let Err(e) = list_files(&mut disk) {
                eprintln!("Error listing files: {}", e);
                std::process::exit(1);
            }
        }
        "insert" => {
            if args.len() < 5 {
                eprintln!("Usage: kef-tool insert <image_path> <kef_path> <dest_name>");
                std::process::exit(1);
            }
            let kef_path = &args[3];
            let dest_name = &args[4];

            let data = match std::fs::read(kef_path) {
                Ok(d) => d,
                Err(e) => {
                    eprintln!("Error reading KEF file '{}': {}", kef_path, e);
                    std::process::exit(1);
                }
            };

            // Read boot sector to get total_clusters
            let mut boot_buf = [0u8; 512];
            if let Err(e) = disk.read_block(0, &mut boot_buf) {
                eprintln!("Error reading boot sector: {}", e);
                std::process::exit(1);
            }
            let bs = BootSector::from_bytes(&boot_buf);
            if bs.magic != FAT_MAGIC {
                eprintln!("Disk is not formatted with KAGFAT16! Please format it first.");
                std::process::exit(1);
            }

            if let Err(e) = create_file(&mut disk, dest_name, &data, bs.total_clusters) {
                eprintln!("Error inserting file: {}", e);
                std::process::exit(1);
            }
            println!("Successfully inserted '{}' into disk image as '{}' ({} bytes)", kef_path, dest_name, data.len());
        }
        _ => {
            print_usage();
            std::process::exit(1);
        }
    }
}

fn print_usage() {
    println!("Usage:");
    println!("  kef-tool format <image_path>");
    println!("  kef-tool list <image_path>");
    println!("  kef-tool insert <image_path> <kef_path> <dest_name>");
}
