#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let msg = "\n=========================================\n\
               🦀 Hello from Rust User Mode (Ring 3)! 🦀\n\
               =========================================\n\
               init.kef loaded and executed successfully.\n";
    print(msg);

    // Let's poll for keypress to shut down
    print("Press any key to trigger shutdown...\n");

    loop {
        // Poll xhci first to ensure keys are processed
        poll_xhci();

        let key = read_key();
        if key != 0 {
            break;
        }
    }

    print("\nShutting down the system. Goodbye!\n");
    shutdown();
}

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

fn print(s: &str) {
    unsafe {
        syscall2(1, s.as_ptr() as usize, s.len());
    }
}

fn read_key() -> usize {
    unsafe {
        syscall0(11)
    }
}

fn poll_xhci() {
    unsafe {
        syscall0(9);
    }
}

fn yield_task() {
    unsafe {
        syscall0(5);
    }
}

fn shutdown() -> ! {
    unsafe {
        syscall0(10);
    }
    loop {}
}
