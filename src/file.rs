//! Per-process file descriptor tables.
//!
//! Threads created with add_task share their creator's table; exec gives the
//! child a fresh one with the console on fds 0, 1 and 2. Disk writes go
//! straight to the filesystem, so closing a descriptor never fails or flushes.

use crate::fs::{self, FsError, FsResult};
use crate::sync::Spinlock;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;

pub const O_READ: usize = 1;
pub const O_WRITE: usize = 2;
pub const O_CREATE: usize = 4;
pub const O_TRUNC: usize = 8;
pub const O_APPEND: usize = 16;
const O_ALL: usize = O_READ | O_WRITE | O_CREATE | O_TRUNC | O_APPEND;

pub const SEEK_SET: usize = 0;
pub const SEEK_CUR: usize = 1;
pub const SEEK_END: usize = 2;

const MAX_FDS: usize = 32;

enum OpenFile {
    Console,
    Disk(DiskFile),
}

struct DiskFile {
    name: String,
    slot: usize,
    offset: u64,
    flags: usize,
}

pub struct FileTable {
    fds: Spinlock<Vec<Option<OpenFile>>>,
}

impl FileTable {
    pub fn with_console() -> Arc<Self> {
        let mut fds = Vec::new();
        fds.resize_with(3, || Some(OpenFile::Console));
        Arc::new(Self { fds: Spinlock::new(fds) })
    }

    pub fn open(&self, name: &str, flags: usize) -> FsResult<usize> {
        if flags & !O_ALL != 0 || flags & (O_READ | O_WRITE) == 0 {
            return Err(FsError::InvalidArgument);
        }
        let writable = flags & O_WRITE != 0;
        if !writable && flags & (O_CREATE | O_TRUNC | O_APPEND) != 0 {
            return Err(FsError::InvalidArgument);
        }

        let mut fds = self.fds.lock();
        let fd = match fds.iter().position(Option::is_none) {
            Some(fd) => fd,
            None if fds.len() < MAX_FDS => {
                fds.push(None);
                fds.len() - 1
            }
            None => return Err(FsError::TooManyOpenFiles),
        };
        let slot = fs::open_file(name, flags & O_CREATE != 0, flags & O_TRUNC != 0)?;
        fds[fd] = Some(OpenFile::Disk(DiskFile {
            name: String::from(name),
            slot,
            offset: 0,
            flags,
        }));
        Ok(fd)
    }

    pub fn read(&self, fd: usize, buf: &mut [u8]) -> FsResult<usize> {
        let mut fds = self.fds.lock();
        match fds.get_mut(fd).and_then(Option::as_mut) {
            Some(OpenFile::Console) => Ok(read_console(buf)),
            Some(OpenFile::Disk(file)) if file.flags & O_READ != 0 => {
                let n = fs::read_at(file.slot, &file.name, file.offset, buf)?;
                file.offset += n as u64;
                Ok(n)
            }
            _ => Err(FsError::BadDescriptor),
        }
    }

    pub fn write(&self, fd: usize, data: &[u8]) -> FsResult<usize> {
        let mut fds = self.fds.lock();
        match fds.get_mut(fd).and_then(Option::as_mut) {
            Some(OpenFile::Console) => {
                crate::print!("{}", String::from_utf8_lossy(data));
                Ok(data.len())
            }
            Some(OpenFile::Disk(file)) if file.flags & O_WRITE != 0 => {
                let offset = if file.flags & O_APPEND != 0 { None } else { Some(file.offset) };
                let (start, n) = fs::write_at(file.slot, &file.name, offset, data)?;
                file.offset = start + n as u64;
                Ok(n)
            }
            _ => Err(FsError::BadDescriptor),
        }
    }

    pub fn close(&self, fd: usize) -> FsResult<()> {
        let mut fds = self.fds.lock();
        match fds.get_mut(fd) {
            Some(entry @ Some(_)) => {
                *entry = None;
                Ok(())
            }
            _ => Err(FsError::BadDescriptor),
        }
    }

    pub fn seek(&self, fd: usize, offset: i64, whence: usize) -> FsResult<u64> {
        let mut fds = self.fds.lock();
        let Some(Some(OpenFile::Disk(file))) = fds.get_mut(fd) else {
            return Err(FsError::BadDescriptor);
        };
        let base = match whence {
            SEEK_SET => 0,
            SEEK_CUR => file.offset,
            SEEK_END => fs::file_size(file.slot, &file.name)?,
            _ => return Err(FsError::InvalidArgument),
        };
        let target = base
            .checked_add_signed(offset)
            .filter(|&target| target <= u32::MAX as u64)
            .ok_or(FsError::InvalidArgument)?;
        file.offset = target;
        Ok(target)
    }
}

/// Non-blocking: returns 0 when no key is buffered.
fn read_console(buf: &mut [u8]) -> usize {
    let _guard = crate::syscall::XHCI_LOCK.lock();
    unsafe {
        crate::drivers::xhci::process_events();
    }
    let mut n = 0;
    while n < buf.len() {
        match crate::drivers::xhci::get_key() {
            Some(key) => {
                buf[n] = key;
                n += 1;
            }
            None => break,
        }
    }
    n
}
