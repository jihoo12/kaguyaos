use crate::gdt;
use core::arch::asm;

// MSR Constants
const MSR_EFER: u32 = 0xC0000080;
const MSR_STAR: u32 = 0xC0000081;
const MSR_LSTAR: u32 = 0xC0000082;
const MSR_SFMASK: u32 = 0xC0000084;
const MSR_KERNEL_GS_BASE: u32 = 0xC0000102;

// EFER bits
const EFER_SCE: u64 = 1; // System Call Extensions

#[repr(C)]
pub struct KernelGsBase {
    pub kernel_stack: u64,
    pub user_stack: u64,
    pub scratch: u64, // Scratch space if needed
}

pub(crate) static mut KERNEL_GS_BASE: KernelGsBase = KernelGsBase {
    kernel_stack: 0,
    user_stack: 0,
    scratch: 0,
};

static XHCI_LOCK: crate::interrupts::InterruptSpinlock<()> = crate::interrupts::InterruptSpinlock::new(());

pub unsafe fn get_global_gs_base() -> u64 {
    core::ptr::addr_of_mut!(KERNEL_GS_BASE) as u64
}

// We need a kernel stack for syscalls.
// allocating 16KB stack
// aligning 16KB stack to 16 bytes
#[repr(align(16))]
struct AlignedStack([u8; 16384]);

static mut SYSCALL_STACK: AlignedStack = AlignedStack([0; 16384]);

pub unsafe fn init_cpu() {
    unsafe {
        // 1. Enable SCE in EFER
        let efer = crate::processor::rdmsr(MSR_EFER);
        crate::processor::wrmsr(MSR_EFER, efer | EFER_SCE);

        // 2. Setup STAR
        // Kernel Code is 0x08.
        // User Code is 0x20.

        let star_val: u64 = ((0x0010 as u64) << 48) | ((gdt::KERNEL_CODE_SEL as u64) << 32);
        crate::processor::wrmsr(MSR_STAR, star_val);

        // 3. Setup LSTAR (Target RIP)
        let handler_addr = syscall_handler as *const () as u64;
        crate::processor::wrmsr(MSR_LSTAR, handler_addr);

        // 4. Setup SFMASK (RFLAGS mask)
        // Mask interrupts (bit 9, 0x200) so cli is automatic on entry
        crate::processor::wrmsr(MSR_SFMASK, 0x200);
    }
}

pub unsafe fn init() {
    unsafe {
        init_cpu();

        // 5. Setup BSP's PercpuData slot 0
        let stack_ptr = core::ptr::addr_of_mut!(SYSCALL_STACK.0) as *mut u8;
        // Actually SYSCALL_STACK.0.len() is 16384.
        let stack_end = stack_ptr.add(16384) as u64;

        let bsp_percpu = &raw mut crate::processor::PERCPU_DATA_SLOTS[0];
        (*bsp_percpu).kernel_stack = stack_end;
        (*bsp_percpu).apic_id = crate::processor::current_apic_id();
        (*bsp_percpu).cpu_index = 0;
        (*bsp_percpu).current_task_index = 0; // BSP runs task 0 (main kernel task)

        crate::processor::set_percpu_data(bsp_percpu);
        crate::processor::wrmsr(crate::processor::MSR_IA32_KERNEL_GS_BASE, bsp_percpu as u64);

        // Safety: Ensure TSS RSP0 is set so interrupts from user mode can switch stack
        crate::gdt::set_tss_stack_cpu(0, stack_end);
    }
}

#[unsafe(naked)]
unsafe extern "C" fn syscall_handler() {
    core::arch::naked_asm!(
        "swapgs",
        "mov gs:[8], rsp",
        "mov rsp, gs:[0]",
        "push r11",
        "push rcx",
        "push rbp",
        "push rbx",
        "push r12",
        "push r13",
        "push r14",
        "push r15",

        "push r9", // Save old R9 (Arg 6)

        "mov r9, r8",  // Arg 5
        "mov r8, r10", // Arg 4
        "mov rcx, rdx", // Arg 3
        "mov rdx, rsi", // Arg 2
        "mov rsi, rdi", // Arg 1
        "mov rdi, rax", // Syscall ID

        "pop rax", // Pop old R9 into RAX temporarily
        "push rax", // Push it as 7th argument (on stack)

        "call {dispatcher}",

        "add rsp, 8", // Pop argument

        "pop r15",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop rbx",
        "pop rbp",
        "pop rcx",
        "pop r11",

        "mov rsp, gs:[8]",
        "swapgs",
        "sysretq",
        dispatcher = sym syscall_dispatcher_impl,
    );
}

#[unsafe(no_mangle)]
extern "sysv64" fn syscall_dispatcher_impl(
    id: usize,
    arg1: usize,
    arg2: usize,
    arg3: usize,
    arg4: usize,
    _arg5: usize,
    _arg6: usize,
) -> usize {
    match id {
        1 => {
            // sys_print(ptr, len)
            sys_print(arg1, arg2);
            0
        }
        2 => {
            // sys_alloc(size, align)
            sys_alloc(arg1, arg2)
        }
        3 => {
            // sys_free(ptr)
            sys_free(arg1);
            0
        }
        4 => {
            // sys_add_task(entry, user_rsp)
            sys_add_task(arg1, arg2)
        }
        5 => {
            // sys_switch_task()
            sys_switch_task();
            0
        }
        6 => {
            // sys_terminate_task(exit_code)
            sys_terminate_task(arg1);
            0
        }
        9 => {
            // sys_xhci_poll()
            let _guard = XHCI_LOCK.lock();
            unsafe {
                crate::xhci::process_events();
            }
            0
        }
        10 => {
            // sys_shutdown()
            sys_shutdown();
            0
        }
        11 => {
            // sys_read_key() -> u8
            sys_read_key()
        }
        12 => {
            // sys_clear()
            sys_clear();
            0
        }
        13 => {
            // sys_realloc(ptr, size, align)
            sys_realloc(arg1, arg2, arg3)
        }
        14 => {
            // sys_fsformat() -> i32
            sys_fsformat() as usize
        }
        15 => {
            // sys_fsls(buf, max_entries) -> isize
            sys_fsls(arg1, arg2) as usize
        }
        16 => {
            // sys_fswrite(filename_ptr, filename_len, content_ptr, content_len) -> i32
            sys_fswrite(arg1, arg2, arg3, arg4) as usize
        }
        17 => {
            // sys_fsread(filename_ptr, filename_len, buffer_ptr, buffer_len) -> isize
            sys_fsread(arg1, arg2, arg3, arg4) as usize
        }
        18 => {
            // sys_fsrm(filename_ptr, filename_len) -> i32
            sys_fsrm(arg1, arg2) as usize
        }
        19 => {
            // sys_get_task_status(task_id) -> usize
            sys_get_task_status(arg1)
        }
        20 => {
            // sys_get_task_exit_code(task_id) -> usize
            sys_get_task_exit_code(arg1)
        }
        21 => {
            // sys_run_ap_scheduler()
            sys_run_ap_scheduler();
        }
        _ => {
            // Unknown syscall
            let _ = crate::println!("Unknown syscall: {}", id);
            usize::MAX
        }
    }
}

use core::alloc::Layout;
use core::slice;
use core::str;

fn sys_print(ptr: usize, len: usize) {
    let slice = unsafe { slice::from_raw_parts(ptr as *const u8, len) };
    match str::from_utf8(slice) {
        Ok(s) => {
            crate::print!("{}", s);
        }
        Err(e) => {
            crate::print!(
                "(sys_print: invalid utf8, ptr={:#x}, len={}, byte={:#x}, err={})",
                ptr,
                len,
                slice[0],
                e
            );
        }
    }
}

fn sys_alloc(size: usize, align: usize) -> usize {
    // We expect valid alignment from userspace (power of 2)
    // If align is 0, default to 8.
    let align = if align == 0 { 8 } else { align };
    match Layout::from_size_align(size, align) {
        Ok(layout) => unsafe { crate::allocator::alloc_aligned(layout) as usize },
        Err(_) => 0, // Allocation failed due to invalid layout
    }
}

fn sys_realloc(ptr: usize, size: usize, align: usize) -> usize {
    let align = if align == 0 { 8 } else { align };
    match Layout::from_size_align(size, align) {
        Ok(layout) => unsafe {
            crate::allocator::realloc_aligned(ptr as *mut u8, layout, size) as usize
        },
        Err(_) => 0,
    }
}

fn sys_free(ptr: usize) {
    unsafe {
        crate::allocator::free(ptr as *mut u8);
    }
}

fn sys_add_task(entry: usize, user_rsp: usize) -> usize {
    // We assume stack size 16KB for new user tasks
    let stack_size = 16384;
    crate::scheduler::add_new_user_task(entry as u64, user_rsp as u64, stack_size)
}

fn sys_switch_task() {
    crate::scheduler::switch_task();
}

fn sys_terminate_task(exit_code: usize) {
    crate::scheduler::terminate_task(exit_code);
}

fn sys_shutdown() {
    unsafe {
        crate::xhci::shutdown();
        crate::nvme::shutdown();
        crate::uefi::system_reset(crate::uefi::EFI_RESET_TYPE::EfiResetShutdown, 0);
    }
}

fn sys_read_key() -> usize {
    let _guard = XHCI_LOCK.lock();
    if let Some(key) = crate::xhci::get_key() {
        key as usize
    } else {
        0
    }
}

fn sys_clear() {
    crate::writer::clear();
}

#[inline(always)]
unsafe fn syscall(
    id: usize,
    arg1: usize,
    arg2: usize,
    arg3: usize,
    arg4: usize,
    arg5: usize,
    arg6: usize,
) -> usize {
    let ret: usize;
    try_syscall(id, arg1, arg2, arg3, arg4, arg5, arg6)
}

#[inline(always)]
unsafe fn try_syscall(
    id: usize,
    arg1: usize,
    arg2: usize,
    arg3: usize,
    arg4: usize,
    arg5: usize,
    arg6: usize,
) -> usize {
    let ret: usize;
    unsafe {
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
    }
    ret
}

#[repr(C)]
pub struct SyscallFileEntry {
    pub name: [u8; 47],
    pub name_len: u8,
    pub size: u64,
    pub start_block: u64,
}

fn sys_fsformat() -> i32 {
    match crate::fs::format() {
        Ok(()) => 0,
        Err(e) => e.code(),
    }
}

fn sys_fsls(buffer_ptr: usize, max_entries: usize) -> isize {
    match crate::fs::list_files() {
        Ok(files) => {
            if buffer_ptr != 0 && max_entries > 0 {
                let dest = unsafe {
                    core::slice::from_raw_parts_mut(
                        buffer_ptr as *mut SyscallFileEntry,
                        max_entries,
                    )
                };
                let count = files.len().min(max_entries);
                for i in 0..count {
                    let mut name_buf = [0u8; 47];
                    let name_bytes = files[i].name.as_bytes();
                    let len = name_bytes.len().min(47);
                    name_buf[..len].copy_from_slice(&name_bytes[..len]);

                    dest[i] = SyscallFileEntry {
                        name: name_buf,
                        name_len: len as u8,
                        size: files[i].size,
                        start_block: files[i].start_block,
                    };
                }
            }
            files.len() as isize
        }
        Err(e) => e.code() as isize,
    }
}

fn sys_fswrite(
    filename_ptr: usize,
    filename_len: usize,
    content_ptr: usize,
    content_len: usize,
) -> i32 {
    let name_slice = unsafe { core::slice::from_raw_parts(filename_ptr as *const u8, filename_len) };
    let Ok(filename) = core::str::from_utf8(name_slice) else {
        return crate::fs::FsError::InvalidArgument.code();
    };
    let content = unsafe { core::slice::from_raw_parts(content_ptr as *const u8, content_len) };
    match crate::fs::create_file(filename, content) {
        Ok(()) => 0,
        Err(e) => e.code(),
    }
}

fn sys_fsread(
    filename_ptr: usize,
    filename_len: usize,
    buffer_ptr: usize,
    buffer_len: usize,
) -> isize {
    let name_slice = unsafe { core::slice::from_raw_parts(filename_ptr as *const u8, filename_len) };
    let Ok(filename) = core::str::from_utf8(name_slice) else {
        return crate::fs::FsError::InvalidArgument.code() as isize;
    };
    match crate::fs::read_file(filename) {
        Ok(data) => {
            if buffer_ptr == 0 || buffer_len == 0 {
                return data.len() as isize;
            }
            let copy_len = data.len().min(buffer_len);
            unsafe {
                core::ptr::copy_nonoverlapping(data.as_ptr(), buffer_ptr as *mut u8, copy_len);
            }
            copy_len as isize
        }
        Err(e) => e.code() as isize,
    }
}

fn sys_fsrm(filename_ptr: usize, filename_len: usize) -> i32 {
    let name_slice = unsafe { core::slice::from_raw_parts(filename_ptr as *const u8, filename_len) };
    let Ok(filename) = core::str::from_utf8(name_slice) else {
        return crate::fs::FsError::InvalidArgument.code();
    };
    match crate::fs::delete_file(filename) {
        Ok(()) => 0,
        Err(e) => e.code(),
    }
}

fn sys_get_task_status(task_id: usize) -> usize {
    crate::scheduler::get_task_status(task_id)
}

fn sys_get_task_exit_code(task_id: usize) -> usize {
    crate::scheduler::get_task_exit_code(task_id)
}

fn sys_run_ap_scheduler() -> ! {
    crate::scheduler::run_ap_scheduler();
}
