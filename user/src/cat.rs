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

/// Copy STDIN to STDOUT until end of input.
fn copy_stdin() -> ! {
    let mut buf = [0u8; 512];
    loop {
        let n = std::read(std::STDIN, &mut buf);
        if n <= 0 {
            std::terminate_task(if n == 0 { 0 } else { 1 });
        }
        if std::write(std::STDOUT, &buf[..n as usize]) < 0 {
            std::terminate_task(1);
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn _start(args_ptr: *const u8, args_len: usize) -> ! {
    if args_len == 0 {
        if std::fd_kind(std::STDIN) != std::FD_CONSOLE {
            copy_stdin();
        }
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

    let fd = std::open(fname_str, std::O_READ);
    if fd < 0 {
        print("Error: ");
        print(fname_str);
        println(" not found");
        std::terminate_task(1);
    }
    let fd = fd as usize;

    let mut buf = [0u8; 512];
    let mut total = 0;
    loop {
        let n = std::read(fd, &mut buf);
        if n < 0 {
            println("Error: read failed");
            std::close(fd);
            std::terminate_task(1);
        }
        if n == 0 {
            break;
        }
        let data = &buf[..n as usize];
        if data.contains(&0) {
            println("(binary, cannot display)");
            break;
        }
        if std::write(std::STDOUT, data) < 0 {
            std::close(fd);
            std::terminate_task(1);
        }
        total += n;
    }
    if total == 0 {
        println("(empty)");
    }

    std::close(fd);
    std::terminate_task(0);
}
