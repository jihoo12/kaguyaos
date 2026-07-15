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
        in("rdi") arg1,
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
        in("rdi") arg1,
        in("rsi") arg2,
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
unsafe fn syscall4(id: usize, arg1: usize, arg2: usize, arg3: usize, arg4: usize) -> usize {
    let ret: usize;
    core::arch::asm!(
        "syscall",
        in("rax") id,
        in("rdi") arg1,
        in("rsi") arg2,
        in("rdx") arg3,
        in("r10") arg4,
        lateout("rax") ret,
        out("rcx") _,
        out("r11") _,
        out("r8") _,
        out("r9") _,
        options(nostack, preserves_flags)
    );
    ret
}

// ── Basic I/O ────────────────────────────────────────────────────────────────

pub fn print(s: &str) {
    unsafe {
        syscall2(0, s.as_ptr() as usize, s.len());
    }
}

pub fn read_key() -> usize {
    unsafe { syscall0(8) }
}

pub fn poll_xhci() {
    unsafe { syscall0(6); }
}

pub fn yield_task() {
    unsafe { syscall0(4); }
}

pub fn shutdown() -> ! {
    unsafe { syscall0(7); }
    loop {}
}

pub fn clear() {
    unsafe { syscall0(9); }
}

// ── Memory ───────────────────────────────────────────────────────────────────

/// Allocate `size` bytes with `align` alignment. Returns null on failure.
pub fn alloc(size: usize, align: usize) -> *mut u8 {
    unsafe { syscall2(1, size, align) as *mut u8 }
}

/// Free a pointer previously returned by `alloc`.
pub fn free(ptr: *mut u8) {
    unsafe { syscall1(2, ptr as usize); }
}

/// Realloc a pointer to a new size/align. Returns null on failure.
pub fn realloc(ptr: *mut u8, size: usize, align: usize) -> *mut u8 {
    unsafe {
        let ret: usize;
        core::arch::asm!(
            "syscall",
            in("rax") 10usize,
            in("rdi") ptr as usize,
            in("rsi") size,
            in("rdx") align,
            lateout("rax") ret,
            out("rcx") _,
            out("r11") _,
            out("r10") _,
            out("r8") _,
            out("r9") _,
            options(nostack, preserves_flags)
        );
        ret as *mut u8
    }
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

/// Returns exit code of a finished task.
pub fn get_task_exit_code(task_id: usize) -> usize {
    unsafe { syscall1(17, task_id) }
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