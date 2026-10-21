//! Per-process file descriptor tables.
//!
//! Threads created with add_task share their creator's table; exec gives the
//! child a fresh one with the console on fds 0, 1 and 2, and spawn copies
//! three chosen descriptors into those slots. Disk writes go straight to the
//! filesystem, so closing a descriptor never fails or flushes. Pipe reads and
//! writes may block, so they run after the table lock is released.

use crate::fs::{self, FsError, FsResult};
use crate::pipe::{self, PipeReader, PipeWriter};
use crate::sync::Spinlock;
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

pub const KIND_CONSOLE: usize = 0;
pub const KIND_FILE: usize = 1;
pub const KIND_PIPE: usize = 2;

const MAX_FDS: usize = 32;

#[derive(Clone)]
enum OpenFile {
    Console,
    Disk(DiskFile),
    PipeRead(PipeReader),
    PipeWrite(PipeWriter),
}

#[derive(Clone)]
struct DiskFile {
    id: fs::FileId,
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

    /// A table for a child process whose fds 0, 1 and 2 copy this table's
    /// `stdio` descriptors. A copied disk file keeps its own offset.
    pub fn inherit(&self, stdio: [usize; 3]) -> FsResult<Arc<Self>> {
        let fds = self.fds.lock();
        let mut child = Vec::with_capacity(stdio.len());
        for fd in stdio {
            let file = fds.get(fd).and_then(Option::as_ref).ok_or(FsError::BadDescriptor)?;
            child.push(Some(file.clone()));
        }
        Ok(Arc::new(Self { fds: Spinlock::new(child) }))
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
        let fd = free_slot(&mut fds)?;
        let id = fs::open_file(name, flags & O_CREATE != 0, flags & O_TRUNC != 0)?;
        fds[fd] = Some(OpenFile::Disk(DiskFile {
            id,
            offset: 0,
            flags,
        }));
        Ok(fd)
    }

    /// Returns the read end's descriptor, then the write end's.
    pub fn pipe(&self) -> FsResult<(usize, usize)> {
        let (reader, writer) = pipe::new();
        let mut fds = self.fds.lock();
        let read_fd = free_slot(&mut fds)?;
        fds[read_fd] = Some(OpenFile::PipeRead(reader));
        match free_slot(&mut fds) {
            Ok(write_fd) => {
                fds[write_fd] = Some(OpenFile::PipeWrite(writer));
                Ok((read_fd, write_fd))
            }
            Err(e) => {
                let reader = fds[read_fd].take();
                drop(fds);
                drop((reader, writer));
                Err(e)
            }
        }
    }

    pub fn kind(&self, fd: usize) -> FsResult<usize> {
        match self.fds.lock().get(fd).and_then(Option::as_ref) {
            Some(OpenFile::Console) => Ok(KIND_CONSOLE),
            Some(OpenFile::Disk(_)) => Ok(KIND_FILE),
            Some(OpenFile::PipeRead(_) | OpenFile::PipeWrite(_)) => Ok(KIND_PIPE),
            None => Err(FsError::BadDescriptor),
        }
    }

    pub fn read(&self, fd: usize, buf: &mut [u8]) -> FsResult<usize> {
        let mut fds = self.fds.lock();
        match fds.get_mut(fd).and_then(Option::as_mut) {
            Some(OpenFile::Console) => {
                drop(fds);
                Ok(crate::drivers::keyboard::read_blocking(buf))
            }
            Some(OpenFile::Disk(file)) if file.flags & O_READ != 0 => {
                let n = fs::read_at(file.id, file.offset, buf)?;
                file.offset += n as u64;
                Ok(n)
            }
            Some(OpenFile::PipeRead(end)) => {
                // Hold a counted end: if another thread closes `fd` while
                // this read blocks, writers must still see a reader.
                let end = end.clone();
                drop(fds);
                end.read(buf)
            }
            _ => Err(FsError::BadDescriptor),
        }
    }

    pub fn write(&self, fd: usize, data: &[u8]) -> FsResult<usize> {
        let mut fds = self.fds.lock();
        match fds.get_mut(fd).and_then(Option::as_mut) {
            Some(OpenFile::Console) => {
                crate::print!("{}", alloc::string::String::from_utf8_lossy(data));
                Ok(data.len())
            }
            Some(OpenFile::Disk(file)) if file.flags & O_WRITE != 0 => {
                let offset = if file.flags & O_APPEND != 0 { None } else { Some(file.offset) };
                let (start, n) = fs::write_at(file.id, offset, data)?;
                file.offset = start + n as u64;
                Ok(n)
            }
            Some(OpenFile::PipeWrite(end)) => {
                let end = end.clone();
                drop(fds);
                end.write(data)
            }
            _ => Err(FsError::BadDescriptor),
        }
    }

    pub fn close(&self, fd: usize) -> FsResult<()> {
        let mut fds = self.fds.lock();
        let file = fds.get_mut(fd).and_then(Option::take).ok_or(FsError::BadDescriptor)?;
        drop(fds);
        drop(file);
        Ok(())
    }

    pub fn seek(&self, fd: usize, offset: i64, whence: usize) -> FsResult<u64> {
        let mut fds = self.fds.lock();
        let Some(Some(OpenFile::Disk(file))) = fds.get_mut(fd) else {
            return Err(FsError::BadDescriptor);
        };
        let base = match whence {
            SEEK_SET => 0,
            SEEK_CUR => file.offset,
            SEEK_END => fs::file_size(file.id)?,
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

fn free_slot(fds: &mut Vec<Option<OpenFile>>) -> FsResult<usize> {
    match fds.iter().position(Option::is_none) {
        Some(fd) => Ok(fd),
        None if fds.len() < MAX_FDS => {
            fds.push(None);
            Ok(fds.len() - 1)
        }
        None => Err(FsError::TooManyOpenFiles),
    }
}
