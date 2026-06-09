#![allow(dead_code)]

use crate::nvme;

pub const BLOCK_SIZE: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsError {
    NotReady,
    InvalidArgument,
    DeviceError,
}

impl FsError {
    pub fn code(self) -> i32 {
        match self {
            FsError::NotReady => -1,
            FsError::InvalidArgument => -2,
            FsError::DeviceError => -3,
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

pub fn read_block(lba: u64, buffer: &mut [u8; BLOCK_SIZE]) -> FsResult<()> {
    read_blocks(lba, 1, buffer.as_mut_ptr())
}

pub fn write_block(lba: u64, buffer: &[u8; BLOCK_SIZE]) -> FsResult<()> {
    write_blocks(lba, 1, buffer.as_ptr())
}

pub fn read_blocks(lba: u64, count: u32, buffer: *mut u8) -> FsResult<()> {
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

pub fn write_blocks(lba: u64, count: u32, buffer: *const u8) -> FsResult<()> {
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
