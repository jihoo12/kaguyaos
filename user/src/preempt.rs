#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

mod std;

#[inline(always)]
fn rdtsc() -> u64 {
    unsafe {
        let lo: u32;
        let hi: u32;
        core::arch::asm!("rdtsc", out("eax") lo, out("edx") hi, options(nomem, nostack));
        ((hi as u64) << 32) | lo as u64
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn _start(_args_ptr: *const u8, _args_len: usize) -> ! {
    std::print("preempt: cpu-bound start\n");

    // Spin for roughly one second at the same 2.4 GHz assumption already used
    // by ping's diagnostic timing. There are deliberately no syscalls, yields,
    // or sleeps in this loop, so only a hardware timer can preempt it.
    let deadline = rdtsc().wrapping_add(2_400_000_000);
    let mut value = 1u64;
    while (rdtsc().wrapping_sub(deadline) as i64) < 0 {
        value = value.wrapping_mul(6364136223846793005).wrapping_add(1);
        core::hint::black_box(value);
    }

    std::print("preempt: cpu-bound finish\n");
    std::terminate_task(0);
}
