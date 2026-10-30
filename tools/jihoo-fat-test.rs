extern crate alloc;

use std::ffi::c_void;

mod sync {
    pub struct Spinlock<T>(std::sync::Mutex<T>);
    impl<T> Spinlock<T> {
        pub const fn new(value: T) -> Self {
            Self(std::sync::Mutex::new(value))
        }
        pub fn lock(&self) -> std::sync::MutexGuard<'_, T> {
            self.0.lock().unwrap()
        }
    }
}

#[path = "../src/fs.rs"]
mod fs;

type Read = extern "C" fn(*mut c_void, u64, *mut u8) -> i32;
type Write = extern "C" fn(*mut c_void, u64, *const u8) -> i32;
unsafe extern "C" {
    fn jihoo_fat_read(cluster: u16, output: *mut u16, context: *mut c_void, read: Read) -> i32;
    fn jihoo_fat_write(
        cluster: u16,
        value: u16,
        context: *mut c_void,
        read: Read,
        write: Write,
    ) -> i32;
    fn jihoo_fat_alloc(
        clusters: u32,
        output: *mut u16,
        context: *mut c_void,
        read: Read,
        write: Write,
    ) -> i32;
    fn jihoo_fat_free(first: u16, context: *mut c_void, read: Read, write: Write) -> i32;
    fn jihoo_fat_walk(
        first: u16,
        steps: u64,
        previous: *mut u16,
        output: *mut u16,
        context: *mut c_void,
        read: Read,
    ) -> i32;
    fn jihoo_dir_read(index: usize, output: *mut u8, context: *mut c_void, read: Read) -> i32;
    fn jihoo_dir_write(
        index: usize,
        input: *const u8,
        context: *mut c_void,
        read: Read,
        write: Write,
    ) -> i32;
    fn jihoo_dir_find(
        name: *const u8,
        length: usize,
        output: *mut u8,
        context: *mut c_void,
        read: Read,
    ) -> i32;
    fn jihoo_dir_free(context: *mut c_void, read: Read) -> i32;
}

struct Disk {
    data: Vec<u8>,
    reads: Vec<u64>,
    writes: Vec<u64>,
    fail_read: u64,
    fail_write: u64,
}

impl Disk {
    fn new() -> Self {
        Self {
            data: vec![0xa5; 81 * 512],
            reads: Vec::new(),
            writes: Vec::new(),
            fail_read: u64::MAX,
            fail_write: u64::MAX,
        }
    }
    fn context(&mut self) -> *mut c_void {
        (self as *mut Self).cast()
    }
    fn put(&mut self, cluster: usize, value: u16) {
        self.data[512 + cluster * 2..514 + cluster * 2].copy_from_slice(&value.to_le_bytes());
    }
    fn get(&self, cluster: usize) -> u16 {
        u16::from_le_bytes(
            self.data[512 + cluster * 2..514 + cluster * 2]
                .try_into()
                .unwrap(),
        )
    }
}

extern "C" fn read(context: *mut c_void, lba: u64, output: *mut u8) -> i32 {
    let disk = unsafe { &mut *context.cast::<Disk>() };
    disk.reads.push(lba);
    if lba == disk.fail_read {
        return -42;
    }
    assert!(lba < 81);
    unsafe {
        std::ptr::copy_nonoverlapping(disk.data.as_ptr().add(lba as usize * 512), output, 512)
    };
    0
}

extern "C" fn write(context: *mut c_void, lba: u64, input: *const u8) -> i32 {
    let disk = unsafe { &mut *context.cast::<Disk>() };
    disk.writes.push(lba);
    if lba == disk.fail_write {
        return -43;
    }
    assert!(lba < 81);
    unsafe {
        std::ptr::copy_nonoverlapping(input, disk.data.as_mut_ptr().add(lba as usize * 512), 512)
    };
    0
}

fn raw_fat() {
    let mut disk = Disk::new();
    let mut output = 0xdead;
    unsafe {
        for cluster in [0u16, 1, 255, 256, 16383] {
            let value = 0x1234 ^ cluster;
            let mut expected = disk.data.clone();
            expected[512 + cluster as usize * 2..514 + cluster as usize * 2]
                .copy_from_slice(&value.to_le_bytes());
            assert_eq!(
                jihoo_fat_write(cluster, value, disk.context(), read, write),
                0
            );
            assert_eq!(disk.data, expected, "FAT write changed neighbouring bytes");
            assert_eq!(
                jihoo_fat_read(cluster, &mut output, disk.context(), read),
                0
            );
            assert_eq!(output, value);
        }
        let before = (disk.reads.len(), disk.writes.len());
        assert_eq!(jihoo_fat_read(16384, &mut output, disk.context(), read), -2);
        assert_eq!(jihoo_fat_write(16384, 0, disk.context(), read, write), -2);
        assert_eq!((disk.reads.len(), disk.writes.len()), before);
        disk.fail_read = 1;
        output = 0xdead;
        assert_eq!(jihoo_fat_read(2, &mut output, disk.context(), read), -42);
        assert_eq!(output, 0xdead);
        let saved = disk.data.clone();
        assert_eq!(jihoo_fat_write(2, 0, disk.context(), read, write), -42);
        assert_eq!(disk.data, saved);
        disk.fail_read = u64::MAX;
        disk.fail_write = 1;
        assert_eq!(jihoo_fat_write(2, 0, disk.context(), read, write), -43);
        assert_eq!(disk.data, saved);
        disk.fail_write = u64::MAX;

        disk.data[512..65 * 512].fill(0xff);
        disk.put(16383, 0);
        assert_eq!(
            jihoo_fat_alloc(u32::MAX, &mut output, disk.context(), read, write),
            0
        );
        assert_eq!(output, 16383);
        assert_eq!(disk.get(16383), 0xffff);
        assert_eq!(
            jihoo_fat_alloc(u32::MAX, &mut output, disk.context(), read, write),
            -5
        );
        disk.put(2, 0);
        assert_eq!(
            jihoo_fat_alloc(0, &mut output, disk.context(), read, write),
            -5
        );
        assert_eq!(
            jihoo_fat_alloc(1, &mut output, disk.context(), read, write),
            0
        );
        assert_eq!(output, 2);

        disk.put(2, 255);
        disk.put(255, 256);
        disk.put(256, 0xffff);
        let mut previous = 0;
        assert_eq!(
            jihoo_fat_walk(2, 2, &mut previous, &mut output, disk.context(), read),
            0
        );
        assert_eq!((previous, output), (255, 256));
        assert_eq!(
            jihoo_fat_walk(2, 3, &mut previous, &mut output, disk.context(), read),
            0
        );
        assert_eq!((previous, output), (256, 0xffff));
        assert_eq!(
            jihoo_fat_walk(2, 4, &mut previous, &mut output, disk.context(), read),
            -3
        );
        assert_eq!(jihoo_fat_free(2, disk.context(), read, write), 0);
        assert_eq!((disk.get(2), disk.get(255), disk.get(256)), (0, 0, 0));
        disk.put(2, 3);
        disk.put(3, 2);
        assert_eq!(jihoo_fat_free(2, disk.context(), read, write), 0);
        assert_eq!((disk.get(2), disk.get(3)), (0, 0));
        disk.put(2, 256);
        disk.put(256, 0xffff);
        disk.fail_read = 2;
        assert_eq!(jihoo_fat_free(2, disk.context(), read, write), -42);
        assert_eq!((disk.get(2), disk.get(256)), (0, 0xffff));
    }
    assert!(
        disk.data[..512].iter().all(|&b| b == 0xa5),
        "FAT operations touched the boot sector"
    );
}

fn raw_directory() {
    let mut disk = Disk::new();
    disk.data[65 * 512..].fill(0);
    let mut entry = [0u8; 32];
    let name = b"abcdefghijklmnopqrstu";
    entry[..21].copy_from_slice(name);
    entry[22..24].copy_from_slice(&0x1234u16.to_le_bytes());
    entry[24..28].copy_from_slice(&0xdeadbeefu32.to_le_bytes());
    entry[28] = 1;
    let mut output = [0xa5; 34];
    unsafe {
        let mut expected = disk.data.clone();
        expected[80 * 512 + 480..81 * 512].copy_from_slice(&entry);
        assert_eq!(
            jihoo_dir_write(255, entry.as_ptr(), disk.context(), read, write),
            0
        );
        assert_eq!(disk.data, expected);
        assert_eq!(
            jihoo_dir_read(255, output.as_mut_ptr().add(1), disk.context(), read),
            0
        );
        assert_eq!(&output[1..33], &entry);
        assert_eq!((output[0], output[33]), (0xa5, 0xa5));
        assert_eq!(
            jihoo_dir_find(
                name.as_ptr(),
                name.len(),
                output.as_mut_ptr().add(1),
                disk.context(),
                read
            ),
            255
        );
        assert_eq!(
            jihoo_dir_find(
                b"absent".as_ptr(),
                6,
                output.as_mut_ptr().add(1),
                disk.context(),
                read
            ),
            256
        );
        assert_eq!(
            jihoo_dir_find(
                name.as_ptr(),
                0,
                output.as_mut_ptr().add(1),
                disk.context(),
                read
            ),
            -2
        );
        assert_eq!(
            jihoo_dir_find(
                name.as_ptr(),
                22,
                output.as_mut_ptr().add(1),
                disk.context(),
                read
            ),
            -2
        );
        assert_eq!(jihoo_dir_free(disk.context(), read), 0);
        for slot in 0..256 {
            disk.data[65 * 512 + slot * 32 + 28] = 1;
        }
        assert_eq!(jihoo_dir_free(disk.context(), read), 256);
        let before = disk.reads.len();
        assert_eq!(
            jihoo_dir_read(256, output.as_mut_ptr(), disk.context(), read),
            -2
        );
        assert_eq!(
            jihoo_dir_write(usize::MAX, entry.as_ptr(), disk.context(), read, write),
            -2
        );
        assert_eq!(disk.reads.len(), before);
        disk.fail_read = 65;
        assert_eq!(
            jihoo_dir_find(
                name.as_ptr(),
                name.len(),
                output.as_mut_ptr(),
                disk.context(),
                read
            ),
            -42
        );
        assert_eq!(jihoo_dir_free(disk.context(), read), -42);
        disk.fail_read = u64::MAX;
        disk.fail_write = 80;
        let saved = disk.data.clone();
        assert_eq!(
            jihoo_dir_write(255, entry.as_ptr(), disk.context(), read, write),
            -43
        );
        assert_eq!(disk.data, saved);
    }
}

fn filesystem_contract() {
    use fs::FsError;
    assert_eq!(fs::read_fat_entry(2), Err(FsError::NotReady));
    let mut disk = vec![0u8; fs::TOTAL_SECTORS as usize * 512];
    unsafe { fs::init_ram_disk(disk.as_mut_ptr()).unwrap() };
    assert_eq!(fs::read_fat_entry(0).unwrap(), 0xfff8);
    assert_eq!(fs::read_fat_entry(1).unwrap(), 0xffff);
    let id = fs::open_file("offset", true, false).unwrap();
    assert_eq!(fs::write_at(id, Some(0), b"abc"), Ok((0, 3)));
    assert_eq!(fs::write_at(id, None, b"def"), Ok((3, 3)));
    let tail: Vec<u8> = (0..5000).map(|i| (i * 7) as u8).collect();
    assert_eq!(fs::write_at(id, Some(10000), &tail), Ok((10000, 5000)));
    let mut expected = vec![0; 15000];
    expected[..6].copy_from_slice(b"abcdef");
    expected[10000..].copy_from_slice(&tail);
    assert_eq!(fs::read_file("offset").unwrap(), expected);
    let mut partial = vec![0xa5; 8000];
    assert_eq!(fs::read_at(id, 4090, &mut partial), Ok(8000));
    assert_eq!(partial, expected[4090..12090]);
    assert_eq!(fs::read_at(id, 15000, &mut partial), Ok(0));
    assert_eq!(
        fs::write_at(id, Some(u32::MAX as u64), b"x"),
        Err(FsError::NoSpace)
    );
    fs::open_file("offset", false, true).unwrap();
    assert_eq!(fs::file_size(id), Ok(0));
    fs::delete_file("offset").unwrap();
    let next = fs::open_file("offset", true, false).unwrap();
    assert_ne!(id, next);
    assert_eq!(fs::read_at(id, 0, &mut partial), Err(FsError::FileNotFound));
    fs::delete_file("offset").unwrap();
    assert_eq!(
        fs::open_file("", true, false),
        Err(FsError::InvalidArgument)
    );
    assert_eq!(
        fs::open_file("abcdefghijklmnopqrstuv", true, false),
        Err(FsError::InvalidArgument)
    );
    fs::create_file("가가가가가가가", b"utf8").unwrap();
    assert_eq!(fs::read_file("가가가가가가가").unwrap(), b"utf8");
    fs::delete_file("가가가가가가가").unwrap();
    for slot in 0..256 {
        fs::open_file(&format!("file{slot}"), true, false).unwrap();
    }
    assert_eq!(fs::open_file("full", true, false), Err(FsError::NoSpace));
    fs::delete_file("file255").unwrap();
    let full = fs::open_file("full", true, false).unwrap();
    for cluster in 2..=(fs::TOTAL_CLUSTERS + 1) as u16 {
        fs::write_fat_entry(cluster, 0xffff).unwrap();
    }
    assert_eq!(fs::write_at(full, Some(0), b"x"), Err(FsError::NoSpace));
    fs::write_fat_entry(2, 0).unwrap();
    assert_eq!(
        fs::write_at(full, Some(0), &vec![0x5a; 5000]),
        Ok((0, 4096))
    );
    assert_eq!(fs::file_size(full), Ok(4096));
    assert_eq!(fs::read_file("full").unwrap(), vec![0x5a; 4096]);
    disk[..512].fill(0x5a);
    let unchanged = disk.clone();
    assert_eq!(
        fs::open_file("unrecognized", true, false),
        Err(FsError::NotFormatted)
    );
    assert_eq!(
        fs::create_file("unrecognized", b"data"),
        Err(FsError::NotFormatted)
    );
    assert_eq!(
        disk, unchanged,
        "an unrecognized volume must not be automatically formatted"
    );
}

fn main() {
    raw_fat();
    raw_directory();
    filesystem_contract();
    println!(
        "{} FAT: entry/sector boundaries, bounded allocation, chain traversal/free, callback failures, directory limits, offsets/gaps, stale IDs, partial ENOSPC and no automatic formatting passed",
        if cfg!(feature = "jihoo-kernel") {
            "jihoo"
        } else {
            "Rust"
        }
    );
}
