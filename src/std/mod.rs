pub mod global_alloc;
pub mod stdio;
use core::arch::asm;

// Syscalls
pub unsafe fn syscall(
    id: usize,
    arg1: usize,
    arg2: usize,
    arg3: usize,
    arg4: usize,
    arg5: usize,
    arg6: usize,
) -> usize {
    let ret: usize;
    asm!(
        "syscall",
        in("rax") id,
        in("rdi") arg1,
        in("rsi") arg2,
        in("rdx") arg3,
        in("r10") arg4,
        in("r8") arg5,
        in("r9") arg6,
        lateout("rax") ret,
        lateout("rcx") _,
        lateout("r11") _,
        options(nostack, preserves_flags)
    );
    ret
}

pub unsafe fn sys_alloc(size: usize, align: usize) -> *mut u8 {
    unsafe { syscall(2, size, align, 0, 0, 0, 0) as *mut u8 }
}

pub unsafe fn sys_free(ptr: *mut u8) {
    unsafe { syscall(3, ptr as usize, 0, 0, 0, 0, 0) };
}

pub unsafe fn sys_realloc(ptr: *mut u8, size: usize, align: usize) -> *mut u8 {
    unsafe { syscall(13, ptr as usize, size, align, 0, 0, 0) as *mut u8 }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct SyscallFileEntry {
    pub name: [u8; 47],
    pub name_len: u8,
    pub size: u64,
    pub first_cluster: u16,
}

pub fn fs_format() -> Result<(), i32> {
    let ret = unsafe { syscall(14, 0, 0, 0, 0, 0, 0) } as i32;
    if ret == 0 {
        Ok(())
    } else {
        Err(ret)
    }
}

pub fn fs_list_files(buf: &mut [SyscallFileEntry]) -> Result<usize, i32> {
    let ret = unsafe { syscall(15, buf.as_mut_ptr() as usize, buf.len(), 0, 0, 0, 0) } as isize;
    if ret >= 0 {
        Ok(ret as usize)
    } else {
        Err(ret as i32)
    }
}

pub fn fs_write(filename: &str, content: &[u8]) -> Result<(), i32> {
    let ret = unsafe {
        syscall(
            16,
            filename.as_ptr() as usize,
            filename.len(),
            content.as_ptr() as usize,
            content.len(),
            0,
            0,
        )
    } as i32;
    if ret == 0 {
        Ok(())
    } else {
        Err(ret)
    }
}

pub fn fs_read(filename: &str, buf: &mut [u8]) -> Result<usize, i32> {
    let ret = unsafe {
        syscall(
            17,
            filename.as_ptr() as usize,
            filename.len(),
            buf.as_mut_ptr() as usize,
            buf.len(),
            0,
            0,
        )
    } as isize;
    if ret >= 0 {
        Ok(ret as usize)
    } else {
        Err(ret as i32)
    }
}

pub fn fs_rm(filename: &str) -> Result<(), i32> {
    let ret = unsafe {
        syscall(
            18,
            filename.as_ptr() as usize,
            filename.len(),
            0,
            0,
            0,
            0,
        )
    } as i32;
    if ret == 0 {
        Ok(())
    } else {
        Err(ret)
    }
}

pub fn spawn_process(entry: usize, user_rsp: usize) -> usize {
    unsafe { syscall(4, entry, user_rsp, 0, 0, 0, 0) }
}

pub fn yield_process() {
    unsafe { syscall(5, 0, 0, 0, 0, 0, 0) };
}

pub extern "sysv64" fn exit_process(exit_code: usize) -> ! {
    unsafe { syscall(6, exit_code, 0, 0, 0, 0, 0) };
    loop {}
}

pub fn get_process_status(pid: usize) -> usize {
    unsafe { syscall(19, pid, 0, 0, 0, 0, 0) }
}

pub fn get_process_exit_code(pid: usize) -> usize {
    unsafe { syscall(20, pid, 0, 0, 0, 0, 0) }
}
