#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

mod std;

fn print(s: &str) {
    std::print(s);
}

fn println(s: &str) {
    std::print(s);
    std::print("\n");
}

#[unsafe(no_mangle)]
pub extern "C" fn _start(args_ptr: *const u8, args_len: usize) -> ! {
    if args_len == 0 {
        println("Usage: rm <filename>");
        std::terminate_task(1);
    }

    let filename;
    let fname_len;
    unsafe {
        let mut start = 0;
        while start < args_len && (*args_ptr.add(start) == b' ' || *args_ptr.add(start) == b'\t') {
            start += 1;
        }
        filename = args_ptr.add(start);
        fname_len = args_len - start;
    }

    if fname_len == 0 {
        println("Usage: rm <filename>");
        std::terminate_task(1);
    }

    let fname_str = unsafe { core::str::from_utf8_unchecked(core::slice::from_raw_parts(filename, fname_len)) };
    let ret = std::fs_rm(fname_str);
    if ret != 0 {
        print("Error: cannot delete ");
        println(fname_str);
        std::terminate_task(1);
    } else {
        print("Deleted ");
        println(fname_str);
    }
    std::terminate_task(0);
}
