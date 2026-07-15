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
        println("Usage: cat <filename>");
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
        println("Usage: cat <filename>");
        std::terminate_task(1);
    }

    let fname_str = unsafe { core::str::from_utf8_unchecked(core::slice::from_raw_parts(filename, fname_len)) };

    let size = std::fs_read(fname_str, &mut []);
    if size < 0 {
        print("Error: ");
        print(fname_str);
        println(" not found");
        std::terminate_task(1);
    }
    if size == 0 {
        println("(empty)");
        std::terminate_task(0);
    }

    let buf = std::alloc(size as usize, 1);
    if buf.is_null() {
        println("Error: out of memory");
        std::terminate_task(1);
    }

    let slice = unsafe { core::slice::from_raw_parts_mut(buf, size as usize) };
    let read = std::fs_read(fname_str, slice);
    if read > 0 {
        unsafe {
            let data = core::slice::from_raw_parts(buf, read as usize);
            let mut is_text = true;
            let mut j = 0;
            while j < data.len() {
                if data[j] >= 0x80 {
                    is_text = false;
                    break;
                }
                j += 1;
            }
            if is_text {
                let s = core::str::from_utf8_unchecked(data);
                std::print(s);
            } else {
                println("(binary, cannot display)");
            }
        }
    }

    std::free(buf);
    std::terminate_task(0);
}
