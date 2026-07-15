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

fn print_u64(mut val: u64) {
    if val == 0 {
        let buf = [b'0'];
        unsafe { std::print(core::str::from_utf8_unchecked(&buf)); }
        return;
    }
    let mut buf = [0u8; 20];
    let mut i = 20;
    while val > 0 {
        i -= 1;
        buf[i] = b'0' + (val % 10) as u8;
        val /= 10;
    }
    unsafe {
        std::print(core::str::from_utf8_unchecked(
            core::slice::from_raw_parts(buf.as_ptr().add(i), 20 - i),
        ));
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn _start(args_ptr: *const u8, args_len: usize) -> ! {
    if args_len == 0 {
        println("Usage: write <filename> <content>");
        std::terminate_task(1);
    }

    unsafe {
        let mut start = 0;
        while start < args_len && (*args_ptr.add(start) == b' ' || *args_ptr.add(start) == b'\t') {
            start += 1;
        }
        let trimmed_ptr = args_ptr.add(start);
        let trimmed_len = args_len - start;

        if trimmed_len == 0 {
            println("Usage: write <filename> <content>");
            std::terminate_task(1);
        }

        let mut space_pos = None;
        let mut i = 0;
        while i < trimmed_len {
            if *trimmed_ptr.add(i) == b' ' || *trimmed_ptr.add(i) == b'\t' {
                space_pos = Some(i);
                break;
            }
            i += 1;
        }

        match space_pos {
            Some(pos) => {
                let fname_ptr = trimmed_ptr;
                let fname_len = pos;
                let mut content_start = pos;
                while content_start < trimmed_len
                    && (*trimmed_ptr.add(content_start) == b' '
                        || *trimmed_ptr.add(content_start) == b'\t')
                {
                    content_start += 1;
                }
                let content_ptr = trimmed_ptr.add(content_start);
                let content_len = trimmed_len - content_start;

                if fname_len > 47 {
                    println("Error: filename too long");
                    std::terminate_task(1);
                }

                let fname = core::str::from_utf8_unchecked(
                    core::slice::from_raw_parts(fname_ptr, fname_len),
                );

                if content_len > 0 {
                    let content = core::slice::from_raw_parts(content_ptr, content_len);
                    let ret = std::fs_write(fname, content);
                    if ret != 0 {
                        println("Error: write failed");
                        std::terminate_task(1);
                    } else {
                        std::print("Wrote ");
                        print_u64(content_len as u64);
                        println(" bytes");
                    }
                } else {
                    let ret = std::fs_write(fname, &[]);
                    if ret != 0 {
                        println("Error: write failed");
                        std::terminate_task(1);
                    } else {
                        println("Wrote 0 bytes");
                    }
                }
            }
            None => {
                if trimmed_len > 47 {
                    println("Error: filename too long");
                    std::terminate_task(1);
                }
                let fname = core::str::from_utf8_unchecked(
                    core::slice::from_raw_parts(trimmed_ptr, trimmed_len),
                );
                let ret = std::fs_write(fname, &[]);
                if ret != 0 {
                    println("Error: write failed");
                    std::terminate_task(1);
                } else {
                    println("Wrote 0 bytes");
                }
            }
        }
    }
    std::terminate_task(0);
}
