use super::{FatDirEntry, FsError, FsResult};
use core::mem::MaybeUninit;

type Context = *mut core::ffi::c_void;

type Read = extern "C" fn(Context, u64, *mut u8) -> i32;
type Write = extern "C" fn(Context, u64, *const u8) -> i32;

struct Io {
    error: Option<FsError>,
}

#[repr(C)]
struct FileData {
    first: u16,
    size: u32,
}

#[repr(C)]
struct DataIo {
    context: Context,
    read: extern "C" fn(Context, u64, u32, *mut u8) -> i32,
    write: extern "C" fn(Context, u64, u32, *const u8) -> i32,
    clusters: u32,
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
    fn jihoo_file_read(
        file: *const FileData,
        offset: u64,
        output: *mut u8,
        length: usize,
        workspace: *mut u8,
        io: *const DataIo,
        whole: bool,
    ) -> i64;
    fn jihoo_file_write(
        file: *mut FileData,
        offset: u64,
        input: *const u8,
        length: usize,
        workspace: *mut u8,
        io: *const DataIo,
        written: *mut u64,
    ) -> i32;
}

extern "C" fn read_data(context: Context, lba: u64, count: u32, output: *mut u8) -> i32 {
    result(context, super::read_blocks_unlocked(lba, count, output))
}

extern "C" fn write_data(context: Context, lba: u64, count: u32, input: *const u8) -> i32 {
    result(context, super::write_blocks_unlocked(lba, count, input))
}

impl DataIo {
    fn new(io: &mut Io) -> Self {
        Self {
            context: io.context(),
            read: read_data,
            write: write_data,
            clusters: super::TOTAL_CLUSTERS,
        }
    }
}

fn read_file_data(
    entry: &FatDirEntry,
    offset: u64,
    output: &mut [u8],
    whole: bool,
) -> FsResult<usize> {
    if offset >= entry.size as u64 || output.is_empty() {
        return Ok(0);
    }
    let file = FileData {
        first: entry.first_cluster,
        size: entry.size,
    };
    let mut io = Io::new();
    let callbacks = DataIo::new(&mut io);
    let mut workspace = alloc::vec![0u8; super::CLUSTER_BYTES];
    let count = unsafe {
        jihoo_file_read(
            &file,
            offset,
            output.as_mut_ptr(),
            output.len(),
            workspace.as_mut_ptr(),
            &callbacks,
            whole,
        )
    };
    if count < 0 {
        io.finish(count as i32)?;
    }
    Ok(count as usize)
}

pub(super) fn read_file_unlocked(name: &str) -> FsResult<alloc::vec::Vec<u8>> {
    let (_, entry) = find_file_unlocked(name)?.ok_or(FsError::FileNotFound)?;
    let mut output = alloc::vec![0u8; entry.size as usize];
    read_file_data(&entry, 0, &mut output, true)?;
    Ok(output)
}

pub(super) fn read_at_unlocked(
    id: super::FileId,
    offset: u64,
    output: &mut [u8],
) -> FsResult<usize> {
    let entry = super::open_entry_unlocked(id)?;
    read_file_data(&entry, offset, output, false)
}

pub(super) fn write_at_unlocked(
    id: super::FileId,
    offset: Option<u64>,
    data: &[u8],
) -> FsResult<(u64, usize)> {
    let mut entry = super::open_entry_unlocked(id)?;
    let offset = offset.unwrap_or(entry.size as u64);
    if data.is_empty() {
        return Ok((offset, 0));
    }
    offset
        .checked_add(data.len() as u64)
        .filter(|&end| end <= u32::MAX as u64)
        .ok_or(FsError::NoSpace)?;
    let mut file = FileData {
        first: entry.first_cluster,
        size: entry.size,
    };
    let mut io = Io::new();
    let callbacks = DataIo::new(&mut io);
    let mut workspace = alloc::vec![0u8; super::CLUSTER_BYTES];
    let mut written = 0;
    let status = unsafe {
        jihoo_file_write(
            &mut file,
            offset,
            data.as_ptr(),
            data.len(),
            workspace.as_mut_ptr(),
            &callbacks,
            &mut written,
        )
    };
    entry.first_cluster = file.first;
    entry.size = file.size;
    super::write_dir_entry_unlocked(id.slot, &entry)?;
    if status == 0 {
        return Ok((offset, data.len()));
    }
    if written > 0 {
        return Ok((offset, written as usize));
    }
    io.finish(status)?;
    unreachable!()
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
