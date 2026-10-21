#![no_std]
#![no_main]

// ── Raw syscall stubs ────────────────────────────────────────────────────────

#[inline(always)]
unsafe fn syscall0(id: usize) -> usize {
    let ret: usize;
    core::arch::asm!(
        "syscall",
        in("rax") id,
        lateout("rax") ret,
        out("rcx") _,
        out("r11") _,
        out("rdi") _,
        out("rsi") _,
        out("rdx") _,
        out("r10") _,
        out("r8") _,
        out("r9") _,
        options(nostack, preserves_flags)
    );
    ret
}

#[inline(always)]
unsafe fn syscall1(id: usize, arg1: usize) -> usize {
    let ret: usize;
    core::arch::asm!(
        "syscall",
        in("rax") id,
        inout("rdi") arg1 => _,
        lateout("rax") ret,
        out("rcx") _,
        out("r11") _,
        out("rsi") _,
        out("rdx") _,
        out("r10") _,
        out("r8") _,
        out("r9") _,
        options(nostack, preserves_flags)
    );
    ret
}

#[inline(always)]
unsafe fn syscall2(id: usize, arg1: usize, arg2: usize) -> usize {
    let ret: usize;
    core::arch::asm!(
        "syscall",
        in("rax") id,
        inout("rdi") arg1 => _,
        inout("rsi") arg2 => _,
        lateout("rax") ret,
        out("rcx") _,
        out("r11") _,
        out("rdx") _,
        out("r10") _,
        out("r8") _,
        out("r9") _,
        options(nostack, preserves_flags)
    );
    ret
}

#[inline(always)]
unsafe fn syscall3(id: usize, arg1: usize, arg2: usize, arg3: usize) -> usize {
    let ret: usize;
    core::arch::asm!(
        "syscall",
        in("rax") id,
        inout("rdi") arg1 => _,
        inout("rsi") arg2 => _,
        inout("rdx") arg3 => _,
        lateout("rax") ret,
        out("rcx") _,
        out("r11") _,
        out("r10") _,
        out("r8") _,
        out("r9") _,
        options(nostack, preserves_flags)
    );
    ret
}

#[inline(always)]
unsafe fn syscall4(id: usize, arg1: usize, arg2: usize, arg3: usize, arg4: usize) -> usize {
    let ret: usize;
    core::arch::asm!(
        "syscall",
        in("rax") id,
        inout("rdi") arg1 => _,
        inout("rsi") arg2 => _,
        inout("rdx") arg3 => _,
        inout("r10") arg4 => _,
        lateout("rax") ret,
        out("rcx") _,
        out("r11") _,
        out("r8") _,
        out("r9") _,
        options(nostack, preserves_flags)
    );
    ret
}

#[inline(always)]
unsafe fn syscall5(id: usize, arg1: usize, arg2: usize, arg3: usize, arg4: usize, arg5: usize) -> usize {
    let ret: usize;
    core::arch::asm!(
        "syscall",
        in("rax") id,
        inout("rdi") arg1 => _,
        inout("rsi") arg2 => _,
        inout("rdx") arg3 => _,
        inout("r10") arg4 => _,
        inout("r8") arg5 => _,
        lateout("rax") ret,
        out("rcx") _,
        out("r11") _,
        out("r9") _,
        options(nostack, preserves_flags)
    );
    ret
}

// ── Basic I/O ────────────────────────────────────────────────────────────────

/// Write to STDOUT, which may be redirected to a file or pipe.
pub fn print(s: &str) {
    write(STDOUT, s.as_bytes());
}

/// Non-blocking; prefer `read(STDIN, ..)`, which sleeps until a key arrives.
pub fn read_key() -> usize {
    unsafe { syscall0(8) }
}

pub fn poll_xhci() {
    unsafe { syscall0(6); }
}

pub fn yield_task() {
    unsafe { syscall0(4); }
}

/// Sleep for at least `milliseconds`, rounded up to the kernel timer tick.
pub fn sleep(milliseconds: usize) {
    unsafe { syscall1(25, milliseconds); }
}

pub fn shutdown() -> ! {
    unsafe { syscall0(7); }
    loop {}
}

pub fn clear() {
    unsafe { syscall0(9); }
}

// ── Memory ───────────────────────────────────────────────────────────────────
//
// First-fit allocator over pages obtained from heap_grow; the kernel only maps
// memory and never reads it. Every block starts with its size (header
// included, a multiple of 16). Free blocks also hold the next free block and
// stay sorted by address so neighbours coalesce. The word before each returned
// pointer holds its block start, which also covers over-aligned requests.

use core::sync::atomic::{AtomicBool, Ordering};

const BLOCK_ALIGN: usize = 16;
const HEADER: usize = 16;
const MIN_SPLIT: usize = 32;
const MIN_GROW: usize = 64 * 1024;

#[repr(C)]
struct FreeBlock {
    size: usize,
    next: *mut FreeBlock,
}

static HEAP_LOCK: AtomicBool = AtomicBool::new(false);
static mut FREE_LIST: *mut FreeBlock = core::ptr::null_mut();

fn heap_lock() {
    while HEAP_LOCK
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        yield_task();
    }
}

fn heap_unlock() {
    HEAP_LOCK.store(false, Ordering::Release);
}

/// Extend the process heap by at least `bytes`. Returns the start of the new
/// memory, or null if the kernel refused.
pub fn heap_grow(bytes: usize) -> *mut u8 {
    unsafe { syscall1(28, bytes) as *mut u8 }
}

unsafe fn insert_free(start: usize, size: usize) {
    let mut prev: *mut FreeBlock = core::ptr::null_mut();
    let mut next = FREE_LIST;
    while !next.is_null() && (next as usize) < start {
        prev = next;
        next = (*next).next;
    }
    let node = if !prev.is_null() && prev as usize + (*prev).size == start {
        (*prev).size += size;
        prev
    } else {
        let node = start as *mut FreeBlock;
        (*node).size = size;
        if prev.is_null() {
            FREE_LIST = node;
        } else {
            (*prev).next = node;
        }
        node
    };
    if !next.is_null() && node as usize + (*node).size == next as usize {
        (*node).size += (*next).size;
        (*node).next = (*next).next;
    } else {
        (*node).next = next;
    }
}

unsafe fn take_free(need: usize) -> usize {
    let mut prev: *mut FreeBlock = core::ptr::null_mut();
    let mut block = FREE_LIST;
    while !block.is_null() {
        let size = (*block).size;
        if size >= need {
            let replacement = if size - need >= MIN_SPLIT {
                let rest = (block as usize + need) as *mut FreeBlock;
                (*rest).size = size - need;
                (*rest).next = (*block).next;
                (*block).size = need;
                rest
            } else {
                (*block).next
            };
            if prev.is_null() {
                FREE_LIST = replacement;
            } else {
                (*prev).next = replacement;
            }
            return block as usize;
        }
        prev = block;
        block = (*block).next;
    }
    0
}

/// Allocate `size` bytes with `align` alignment. Returns null on failure.
pub fn alloc(size: usize, align: usize) -> *mut u8 {
    let align = if align == 0 { 8 } else { align };
    if !align.is_power_of_two() {
        return core::ptr::null_mut();
    }
    let extra = if align > BLOCK_ALIGN { align } else { 0 };
    let need = match size.checked_add(HEADER + extra + BLOCK_ALIGN - 1) {
        Some(n) => n & !(BLOCK_ALIGN - 1),
        None => return core::ptr::null_mut(),
    };
    heap_lock();
    let mut start = unsafe { take_free(need) };
    if start == 0 {
        let grow = if need > MIN_GROW { need } else { MIN_GROW };
        let base = heap_grow(grow) as usize;
        if base != 0 {
            let grown = (grow + 4095) & !4095;
            unsafe {
                insert_free(base, grown);
                start = take_free(need);
            }
        }
    }
    heap_unlock();
    if start == 0 {
        return core::ptr::null_mut();
    }
    let ptr = (start + HEADER + align - 1) & !(align - 1);
    unsafe { *((ptr - 8) as *mut usize) = start };
    ptr as *mut u8
}

/// Free a pointer previously returned by `alloc`.
pub fn free(ptr: *mut u8) {
    if ptr.is_null() {
        return;
    }
    unsafe {
        let start = *((ptr as usize - 8) as *const usize);
        let size = *(start as *const usize);
        heap_lock();
        insert_free(start, size);
        heap_unlock();
    }
}

/// Realloc a pointer to a new size/align. Returns null on failure.
pub fn realloc(ptr: *mut u8, size: usize, align: usize) -> *mut u8 {
    if ptr.is_null() {
        return alloc(size, align);
    }
    let new_ptr = alloc(size, align);
    if !new_ptr.is_null() {
        unsafe {
            let start = *((ptr as usize - 8) as *const usize);
            let capacity = start + *(start as *const usize) - ptr as usize;
            core::ptr::copy_nonoverlapping(ptr, new_ptr, if size < capacity { size } else { capacity });
        }
        free(ptr);
    }
    new_ptr
}

// ── Task management ──────────────────────────────────────────────────────────

/// Spawn a new user task at `entry` with stack pointer `user_rsp`.
/// Returns the task ID.
pub fn add_task(entry: usize, user_rsp: usize) -> usize {
    unsafe { syscall2(3, entry, user_rsp) }
}

/// Terminate the current task with `exit_code`.
pub fn terminate_task(exit_code: usize) -> ! {
    unsafe { syscall1(5, exit_code); }
    loop {}
}

/// Returns status of task with given ID.
/// 0 = running, 1 = finished, usize::MAX = not found.
pub fn get_task_status(task_id: usize) -> usize {
    unsafe { syscall1(16, task_id) }
}

/// Block until task_id exits and return its exit code.
pub fn wait_task(task_id: usize) -> usize {
    unsafe { syscall1(26, task_id) }
}

/// Returns exit code of a finished task.
pub fn get_task_exit_code(task_id: usize) -> usize {
    unsafe { syscall1(17, task_id) }
}

// ── File descriptors ─────────────────────────────────────────────────────────
//
// Every process starts with the console on STDIN, STDOUT and STDERR. Reading
// the console blocks until a key is pressed; it has no end of input. Errors
// are the negative filesystem codes.

pub const STDIN: usize = 0;
pub const STDOUT: usize = 1;
pub const STDERR: usize = 2;

pub const O_READ: usize = 1;
pub const O_WRITE: usize = 2;
pub const O_CREATE: usize = 4;
pub const O_TRUNC: usize = 8;
pub const O_APPEND: usize = 16;

pub const SEEK_SET: usize = 0;
pub const SEEK_CUR: usize = 1;
pub const SEEK_END: usize = 2;

/// Open a file. Returns a descriptor, or negative on error.
pub fn open(filename: &str, flags: usize) -> isize {
    unsafe { syscall3(29, filename.as_ptr() as usize, filename.len(), flags) as isize }
}

/// Read up to `buf.len()` bytes. Returns bytes read (0 at end of file).
pub fn read(fd: usize, buf: &mut [u8]) -> isize {
    unsafe { syscall3(30, fd, buf.as_mut_ptr() as usize, buf.len()) as isize }
}

/// Write `data`. Returns bytes written, which is short only if the disk filled up.
pub fn write(fd: usize, data: &[u8]) -> isize {
    unsafe { syscall3(31, fd, data.as_ptr() as usize, data.len()) as isize }
}

pub fn close(fd: usize) -> isize {
    unsafe { syscall1(32, fd) as isize }
}

/// Move the file offset. Returns the new offset.
pub fn seek(fd: usize, offset: isize, whence: usize) -> isize {
    unsafe { syscall3(33, fd, offset as usize, whence) as isize }
}

/// Create a pipe and return `(read_fd, write_fd)`. Reads block until data
/// arrives and return 0 once every write end is closed; writes block while
/// the pipe is full and fail with -9 once every read end is closed.
pub fn pipe() -> Option<(usize, usize)> {
    let mut fds = [0usize; 2];
    let ret = unsafe { syscall1(34, fds.as_mut_ptr() as usize) as isize };
    if ret == 0 { Some((fds[0], fds[1])) } else { None }
}

pub const FD_CONSOLE: isize = 0;
pub const FD_FILE: isize = 1;
pub const FD_PIPE: isize = 2;

/// What `fd` refers to: FD_CONSOLE, FD_FILE or FD_PIPE, or negative if unused.
pub fn fd_kind(fd: usize) -> isize {
    unsafe { syscall1(36, fd) as isize }
}

// ── Filesystem ───────────────────────────────────────────────────────────────

/// Matches the kernel's `SyscallFileEntry` repr.
#[derive(Copy, Clone)]
#[repr(C)]
pub struct FileEntry {
    pub name: [u8; 47],
    pub name_len: u8,
    pub size: u64,
    pub first_cluster: u16,
}

/// Format the filesystem. Returns 0 on success, negative error code otherwise.
pub fn fs_format() -> i32 {
    unsafe { syscall0(11) as i32 }
}

/// List files. Fills `buf` and returns the total number of files (may exceed buf.len()).
/// Returns negative on error.
pub fn fs_ls(buf: &mut [FileEntry]) -> isize {
    unsafe { syscall2(12, buf.as_mut_ptr() as usize, buf.len()) as isize }
}

/// Write a file. Returns 0 on success.
pub fn fs_write(filename: &str, content: &[u8]) -> i32 {
    unsafe {
        syscall4(
            13,
            filename.as_ptr() as usize,
            filename.len(),
            content.as_ptr() as usize,
            content.len(),
        ) as i32
    }
}

/// Read a file into `buf`. Returns bytes copied, or negative on error.
/// If `buf` is empty, returns the required size.
pub fn fs_read(filename: &str, buf: &mut [u8]) -> isize {
    unsafe {
        syscall4(
            14,
            filename.as_ptr() as usize,
            filename.len(),
            buf.as_mut_ptr() as usize,
            buf.len(),
        ) as isize
    }
}

/// Delete a file. Returns 0 on success.
pub fn fs_rm(filename: &str) -> i32 {
    unsafe {
        syscall2(
            15,
            filename.as_ptr() as usize,
            filename.len(),
        ) as i32
    }
}

/// Execute a KEF binary from the filesystem. Returns the new task ID,
/// or usize::MAX on error.
pub fn exec(filename: &str) -> usize {
    unsafe { syscall2(21, filename.as_ptr() as usize, filename.len()) }
}

/// Execute a KEF binary with arguments. Returns the new task ID,
/// or usize::MAX on error.
pub fn exec2(filename: &str, args: &str) -> usize {
    unsafe { syscall4(22, filename.as_ptr() as usize, filename.len(), args.as_ptr() as usize, args.len()) }
}

/// Execute a KEF binary whose STDIN, STDOUT and STDERR are copies of this
/// process's descriptors in `stdio`. Returns the new task ID, or usize::MAX.
pub fn spawn(filename: &str, args: &str, stdio: [usize; 3]) -> usize {
    unsafe {
        syscall5(
            35,
            filename.as_ptr() as usize,
            filename.len(),
            args.as_ptr() as usize,
            args.len(),
            stdio.as_ptr() as usize,
        )
    }
}

// ── Network ─────────────────────────────────────────────────────────────────

/// Send an ICMP Echo Request to the given IP address.
/// `dst_ip` is `[a,b,c,d]` in host byte order.
/// Returns the ICMP sequence number, or 0 on failure.
pub fn net_send_ping(dst_ip: [u8; 4]) -> u32 {
    let packed = (dst_ip[0] as u32)
        | ((dst_ip[1] as u32) << 8)
        | ((dst_ip[2] as u32) << 16)
        | ((dst_ip[3] as u32) << 24);
    unsafe { syscall1(23, packed as usize) as u32 }
}

/// Receive one ICMP Echo Reply from the kernel ring buffer.
/// Copies into `buf` and returns bytes read, or 0 if empty.
pub fn net_recv_ping(buf: &mut [u8]) -> usize {
    unsafe { syscall2(24, buf.as_mut_ptr() as usize, buf.len()) }
}


/// Resolve an IPv4 A record through the kernel DNS client.
pub fn dns_resolve(name: &str) -> Option<[u8; 4]> {
    let v = unsafe { syscall2(27, name.as_ptr() as usize, name.len()) } as u32;
    if v == 0 { None } else { Some([v as u8, (v>>8) as u8, (v>>16) as u8, (v>>24) as u8]) }
}
