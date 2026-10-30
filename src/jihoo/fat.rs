use super::{FatDirEntry, FsError, FsResult};
use core::mem::MaybeUninit;

type Context = *mut core::ffi::c_void;

type Read = extern "C" fn(Context, u64, *mut u8) -> i32;
type Write = extern "C" fn(Context, u64, *const u8) -> i32;

struct Io {
    error: Option<FsError>,
}

unsafe extern "C" {
    fn jihoo_fat_read(cluster: u16, output: *mut u16, context: Context, read: Read) -> i32;
    fn jihoo_fat_write(cluster: u16, value: u16, context: Context, read: Read, write: Write)
    -> i32;
    fn jihoo_fat_alloc(
        clusters: u32,
        output: *mut u16,
        context: Context,
        read: Read,
        write: Write,
    ) -> i32;
    fn jihoo_fat_free(first: u16, context: Context, read: Read, write: Write) -> i32;
    fn jihoo_fat_walk(
        first: u16,
        steps: u64,
        previous: *mut u16,
        output: *mut u16,
        context: Context,
        read: Read,
    ) -> i32;
    fn jihoo_dir_read(index: usize, output: *mut u8, context: Context, read: Read) -> i32;
    fn jihoo_dir_write(
        index: usize,
        input: *const u8,
        context: Context,
        read: Read,
        write: Write,
    ) -> i32;
    fn jihoo_dir_find(
        name: *const u8,
        length: usize,
        output: *mut u8,
        context: Context,
        read: Read,
    ) -> i32;
    fn jihoo_dir_free(context: Context, read: Read) -> i32;
}

extern "C" fn read(context: Context, lba: u64, output: *mut u8) -> i32 {
    result(context, super::read_blocks_unlocked(lba, 1, output))
}

extern "C" fn write(context: Context, lba: u64, input: *const u8) -> i32 {
    result(context, super::write_blocks_unlocked(lba, 1, input))
}

fn result(context: Context, status: FsResult<()>) -> i32 {
    match status {
        Ok(()) => 0,
        Err(error) => {
            unsafe { (*context.cast::<Io>()).error = Some(error) };
            error.code()
        }
    }
}

impl Io {
    fn new() -> Self {
        Self { error: None }
    }
    fn context(&mut self) -> Context {
        (self as *mut Self).cast()
    }
    fn finish(&self, status: i32) -> FsResult<()> {
        if status == 0 {
            return Ok(());
        }
        Err(self.error.unwrap_or(match status {
            -2 => FsError::InvalidArgument,
            -5 => FsError::NoSpace,
            _ => FsError::DeviceError,
        }))
    }
    fn slot(&self, status: i32) -> FsResult<Option<usize>> {
        match status {
            0..=255 => Ok(Some(status as usize)),
            256 => Ok(None),
            _ => {
                self.finish(status)?;
                unreachable!()
            }
        }
    }
}

pub(super) fn read_fat_entry_unlocked(cluster: u16) -> FsResult<u16> {
    let mut io = Io::new();
    let mut output = MaybeUninit::uninit();
    let status = unsafe { jihoo_fat_read(cluster, output.as_mut_ptr(), io.context(), read) };
    io.finish(status)?;
    Ok(unsafe { output.assume_init() })
}

pub(super) fn write_fat_entry_unlocked(cluster: u16, value: u16) -> FsResult<()> {
    let mut io = Io::new();
    let status = unsafe { jihoo_fat_write(cluster, value, io.context(), read, write) };
    io.finish(status)
}

pub(super) fn alloc_cluster_unlocked() -> FsResult<u16> {
    let mut io = Io::new();
    let mut output = MaybeUninit::uninit();
    let status = unsafe {
        jihoo_fat_alloc(
            super::TOTAL_CLUSTERS,
            output.as_mut_ptr(),
            io.context(),
            read,
            write,
        )
    };
    io.finish(status)?;
    Ok(unsafe { output.assume_init() })
}

pub(super) fn free_cluster_chain_unlocked(first: u16) -> FsResult<()> {
    let mut io = Io::new();
    let status = unsafe { jihoo_fat_free(first, io.context(), read, write) };
    io.finish(status)
}

pub(super) fn walk_chain_unlocked(first: u16, steps: u64) -> FsResult<(u16, u16)> {
    let mut io = Io::new();
    let (mut previous, mut output) = (MaybeUninit::uninit(), MaybeUninit::uninit());
    let status = unsafe {
        jihoo_fat_walk(
            first,
            steps,
            previous.as_mut_ptr(),
            output.as_mut_ptr(),
            io.context(),
            read,
        )
    };
    io.finish(status)?;
    Ok(unsafe { (previous.assume_init(), output.assume_init()) })
}

pub(super) fn read_dir_entry_unlocked(index: usize) -> FsResult<FatDirEntry> {
    let mut io = Io::new();
    let mut output = MaybeUninit::<FatDirEntry>::uninit();
    let status = unsafe { jihoo_dir_read(index, output.as_mut_ptr().cast(), io.context(), read) };
    io.finish(status)?;
    Ok(unsafe { output.assume_init() })
}

pub(super) fn write_dir_entry_unlocked(index: usize, entry: &FatDirEntry) -> FsResult<()> {
    if index >= super::ROOT_DIR_ENTRIES {
        return Err(FsError::InvalidArgument);
    }
    if entry.in_use == 0 {
        super::retire_slot_unlocked(index);
    }
    let mut io = Io::new();
    let status = unsafe {
        jihoo_dir_write(
            index,
            (entry as *const FatDirEntry).cast(),
            io.context(),
            read,
            write,
        )
    };
    io.finish(status)
}

pub(super) fn find_file_unlocked(name: &str) -> FsResult<Option<(usize, FatDirEntry)>> {
    let mut io = Io::new();
    let mut output = MaybeUninit::<FatDirEntry>::uninit();
    let status = unsafe {
        jihoo_dir_find(
            name.as_ptr(),
            name.len(),
            output.as_mut_ptr().cast(),
            io.context(),
            read,
        )
    };
    Ok(io
        .slot(status)?
        .map(|slot| (slot, unsafe { output.assume_init() })))
}

pub(super) fn find_free_dir_slot_unlocked() -> FsResult<Option<usize>> {
    let mut io = Io::new();
    let status = unsafe { jihoo_dir_free(io.context(), read) };
    io.slot(status)
}
