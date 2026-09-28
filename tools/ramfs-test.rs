extern crate alloc;

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

#[test]
fn temporary_files_and_disk_bounds() {
    let mut disk = vec![0u8; fs::TOTAL_SECTORS as usize * fs::BLOCK_SIZE];
    unsafe { fs::init_ram_disk(disk.as_mut_ptr()).unwrap() };
    let data = vec![0x5a; 9000];
    fs::create_file("test", &data).unwrap();
    assert_eq!(fs::read_file("test").unwrap(), data);
    fs::create_file("test", b"short").unwrap();
    assert_eq!(fs::read_file("test").unwrap(), b"short");
    fs::delete_file("test").unwrap();
    assert_eq!(fs::read_file("test"), Err(fs::FsError::FileNotFound));

    let block = [0x36; fs::BLOCK_SIZE];
    let last = fs::TOTAL_SECTORS - 1;
    fs::write_blocks(last, 1, block.as_ptr()).unwrap();
    let mut read = [0; fs::BLOCK_SIZE];
    fs::read_blocks(last, 1, read.as_mut_ptr()).unwrap();
    assert_eq!(read, block);
    for (lba, count) in [(last, 2), (fs::TOTAL_SECTORS, 1), (u64::MAX, 2), (0, 0)] {
        assert_eq!(fs::write_blocks(lba, count, block.as_ptr()), Err(fs::FsError::InvalidArgument));
        assert_eq!(fs::read_blocks(lba, count, read.as_mut_ptr()), Err(fs::FsError::InvalidArgument));
    }
    fs::create_file("session", b"temporary").unwrap();
    disk.fill(0);
    unsafe { fs::init_ram_disk(disk.as_mut_ptr()).unwrap() };
    assert!(fs::list_files().unwrap().is_empty());
}
