extern crate alloc;

use std::sync::{Mutex, MutexGuard, OnceLock};

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

struct Disk {
    bytes: Vec<u8>,
    ready: bool,
    fail_at: usize,
    trace: Vec<(bool, u64, u32)>,
}

fn disk() -> MutexGuard<'static, Disk> {
    static DISK: OnceLock<Mutex<Disk>> = OnceLock::new();
    DISK.get_or_init(|| {
        Mutex::new(Disk {
            bytes: vec![0xa5; 256 * 512],
            ready: false,
            fail_at: 0,
            trace: Vec::new(),
        })
    })
    .lock()
    .unwrap()
}

mod drivers {
    pub mod nvme {
        pub unsafe fn default_nsid() -> Option<u32> {
            super::super::disk().ready.then_some(1)
        }
        pub unsafe fn nvme_read(nsid: u32, lba: u64, buffer: *mut u8, count: u32) -> i32 {
            transfer(nsid, lba, buffer, count, false)
        }
        pub unsafe fn nvme_write(nsid: u32, lba: u64, buffer: *mut u8, count: u32) -> i32 {
            transfer(nsid, lba, buffer, count, true)
        }
        fn transfer(nsid: u32, lba: u64, buffer: *mut u8, count: u32, write: bool) -> i32 {
            assert_eq!(nsid, 1);
            let mut state = super::super::disk();
            state.trace.push((write, lba, count));
            if state.fail_at != 0 && state.trace.len() == state.fail_at {
                return -1;
            }
            let Some(start) = lba.checked_mul(512) else {
                return -1;
            };
            let length = count as usize * 512;
            if start > state.bytes.len() as u64 || length > state.bytes.len() - start as usize {
                return -1;
            }
            unsafe {
                let address = state.bytes.as_mut_ptr().add(start as usize);
                if write {
                    std::ptr::copy_nonoverlapping(buffer, address, length);
                } else {
                    std::ptr::copy_nonoverlapping(address, buffer, length);
                }
            }
            0
        }
    }
}

#[path = "../src/fs.rs"]
mod fs;

fn reset(image: &[u8]) {
    let mut state = disk();
    state.bytes.copy_from_slice(image);
    state.ready = true;
    state.fail_at = 0;
    state.trace.clear();
}

fn hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |h, &b| {
        (h ^ b as u64).wrapping_mul(0x100000001b3)
    })
}

#[derive(Clone, Copy, Debug)]
enum Operation {
    Read(u64, usize),
    Whole,
    Write(Option<u64>, usize),
}

fn attempt(image: &[u8], operation: Operation, fault: usize) -> usize {
    reset(image);
    let id = fs::open_file("data", false, false).unwrap();
    {
        let mut state = disk();
        state.trace.clear();
        state.fail_at = fault;
    }
    let (status, output) = match operation {
        Operation::Read(offset, count) => {
            let mut output = vec![0xa5; count];
            (
                format!("{:?}", fs::read_at(id, offset, &mut output)),
                output,
            )
        }
        Operation::Whole => match fs::read_file("data") {
            Ok(data) => (format!("Ok({})", data.len()), data),
            Err(e) => (format!("Err({e:?})"), Vec::new()),
        },
        Operation::Write(offset, count) => {
            let data: Vec<u8> = (0..count).map(|i| (i * 19 + 7) as u8).collect();
            (format!("{:?}", fs::write_at(id, offset, &data)), Vec::new())
        }
    };
    let (calls, trace, image_hash) = {
        let mut state = disk();
        state.fail_at = 0;
        (state.trace.len(), state.trace.clone(), hash(&state.bytes))
    };
    let size = fs::file_size(id);
    println!(
        "{operation:?} fault={fault} result={status} size={size:?} output={:016x} disk={image_hash:016x} trace={trace:?}",
        hash(&output)
    );
    calls
}

fn fixture(empty: bool) -> Vec<u8> {
    reset(&vec![0xa5; 256 * 512]);
    fs::format().unwrap();
    let id = fs::open_file("data", true, false).unwrap();
    if !empty {
        let bytes: Vec<u8> = (0..9000).map(|i| (i * 7 + i / 17) as u8).collect();
        assert_eq!(fs::write_at(id, Some(0), &bytes), Ok((0, bytes.len())));
        assert_eq!(fs::read_file("data").unwrap(), bytes);
    }
    disk().bytes.clone()
}

fn failure_matrix(image: &[u8]) {
    for operation in [
        Operation::Read(0, 0),
        Operation::Read(4090, 7000),
        Operation::Read(0, 9000),
        Operation::Read(u64::MAX, 3),
        Operation::Whole,
        Operation::Write(Some(0), 4096),
        Operation::Write(Some(511), 4099),
        Operation::Write(Some(4090), 5000),
        Operation::Write(None, 2000),
        Operation::Write(Some(20000), 6000),
        Operation::Write(Some(u64::MAX), 0),
        Operation::Write(Some(u32::MAX as u64), 1),
    ] {
        let calls = attempt(image, operation, 0);
        for fault in 1..=calls + 1 {
            attempt(image, operation, fault);
        }
    }
}

fn model_test(image: &[u8]) {
    reset(image);
    let id = fs::open_file("data", false, false).unwrap();
    let mut model = fs::read_file("data").unwrap();
    let mut seed = 0x123456789abcdefu64;
    for _ in 0..500 {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let offset = seed as usize % 28000;
        let count = (seed >> 32) as usize % 4100;
        let data: Vec<u8> = (0..count).map(|i| (i + seed as usize) as u8).collect();
        assert_eq!(
            fs::write_at(id, Some(offset as u64), &data),
            Ok((offset as u64, count))
        );
        if count > 0 {
            model.resize(model.len().max(offset + count), 0);
            model[offset..offset + count].copy_from_slice(&data);
        }
        assert_eq!(fs::read_file("data").unwrap(), model);
        let mut output = vec![0xa5; count];
        let wanted = count.min(model.len().saturating_sub(offset));
        assert_eq!(fs::read_at(id, offset as u64, &mut output), Ok(wanted));
        if wanted > 0 {
            assert_eq!(&output[..wanted], &model[offset..offset + wanted]);
        }
        assert!(output[wanted..].iter().all(|&b| b == 0xa5));
    }
    println!(
        "model: 500 writes/reads passed, data={:016x}, disk={:016x}",
        hash(&model),
        hash(&disk().bytes)
    );
}

fn main() {
    assert_eq!(
        fs::open_file("data", false, false),
        Err(fs::FsError::NotReady)
    );
    let populated = fixture(false);
    failure_matrix(&populated);
    let empty = fixture(true);
    failure_matrix(&empty);
    // Premature end, an invalid next-cluster index, and a valid index whose
    // data blocks exceed the mock device. Preserve each API's existing policy.
    for next in [0xffffu16, 16384, 16383] {
        let mut corrupt = populated.clone();
        corrupt[512 + 4..512 + 6].copy_from_slice(&next.to_le_bytes());
        attempt(&corrupt, Operation::Read(0, 9000), 0);
        attempt(&corrupt, Operation::Whole, 0);
    }
    model_test(&populated);
}
