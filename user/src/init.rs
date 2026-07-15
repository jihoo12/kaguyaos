#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

mod std;

const KEY_ENTER: u8 = 0x0A;
const KEY_BACKSPACE: u8 = 0x08;
const MAX_CMD_LEN: usize = 128;

unsafe fn print_raw(ptr: *const u8, len: usize) {
    std::print(core::str::from_utf8_unchecked(
        core::slice::from_raw_parts(ptr, len),
    ));
}

fn put_char(ch: u8) {
    let buf = [ch];
    unsafe {
        std::print(core::str::from_utf8_unchecked(&buf));
    }
}

fn print(s: &str) {
    std::print(s);
}

fn println(s: &str) {
    std::print(s);
    std::print("\n");
}

fn print_u64(mut val: u64) {
    if val == 0 {
        put_char(b'0');
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

#[inline(never)]
fn bytes_eq(a: *const u8, a_len: usize, b: &[u8]) -> bool {
    if a_len != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a_len {
        unsafe {
            if *a.add(i) != b[i] {
                return false;
            }
        }
        i += 1;
    }
    true
}

#[inline(never)]
fn cmd_help() {
    println("Commands:");
    println("  help              Show this help");
    println("  ls                List files");
    println("  cat <file>        Show file contents");
    println("  write <file> <msg> Write msg to a file");
    println("  rm <file>         Delete a file");
    println("  exec <file>       Execute a KEF binary");
    println("  clear             Clear screen");
    println("  shutdown          Shut down");
}

#[inline(never)]
fn exec_program(name: &str, args: &str) {
    if args.len() > 0 {
        let task_id = std::exec2(name, args);
        if task_id == usize::MAX {
            print("Error: failed to execute ");
            println(name);
        } else {
            std::yield_task();
        }
    } else {
        let task_id = std::exec(name);
        if task_id == usize::MAX {
            print("Error: failed to execute ");
            println(name);
        } else {
            std::yield_task();
        }
    }
}

// ── Command dispatch ───────────────────────────────────────────────────────

#[inline(never)]
fn process_command(cmd_ptr: *const u8, cmd_len: usize) {
    unsafe {
        let mut start = 0;
        while start < cmd_len && (*cmd_ptr.add(start) == b' ' || *cmd_ptr.add(start) == b'\t') {
            start += 1;
        }
        let trimmed_ptr = cmd_ptr.add(start);
        let trimmed_len = cmd_len - start;

        if trimmed_len == 0 {
            return;
        }

        let mut space_pos = trimmed_len;
        let mut i = 0;
        while i < trimmed_len {
            if *trimmed_ptr.add(i) == b' ' || *trimmed_ptr.add(i) == b'\t' {
                space_pos = i;
                break;
            }
            i += 1;
        }

        let cmd_ptr = trimmed_ptr;
        let cmd_len = space_pos;
        let mut args_ptr = trimmed_ptr.add(space_pos);
        let mut args_len = trimmed_len - space_pos;
        while args_len > 0 && (*args_ptr == b' ' || *args_ptr == b'\t') {
            args_ptr = args_ptr.add(1);
            args_len -= 1;
        }

        if bytes_eq(cmd_ptr, cmd_len, b"help") {
            cmd_help();
        } else if bytes_eq(cmd_ptr, cmd_len, b"ls") {
            exec_program("ls.kef", "");
        } else if bytes_eq(cmd_ptr, cmd_len, b"cat") {
            exec_program("cat.kef", core::str::from_utf8_unchecked(core::slice::from_raw_parts(args_ptr, args_len)));
        } else if bytes_eq(cmd_ptr, cmd_len, b"write") {
            exec_program("write.kef", core::str::from_utf8_unchecked(core::slice::from_raw_parts(args_ptr, args_len)));
        } else if bytes_eq(cmd_ptr, cmd_len, b"rm") {
            exec_program("rm.kef", core::str::from_utf8_unchecked(core::slice::from_raw_parts(args_ptr, args_len)));
        } else if bytes_eq(cmd_ptr, cmd_len, b"clear") {
            std::clear();
        } else if bytes_eq(cmd_ptr, cmd_len, b"exec") {
            exec_program(core::str::from_utf8_unchecked(core::slice::from_raw_parts(args_ptr, args_len)), "");
        } else if bytes_eq(cmd_ptr, cmd_len, b"shutdown") {
            println("Goodbye!");
            std::shutdown();
        } else {
            print("Unknown: ");
            unsafe {
                print_raw(cmd_ptr, cmd_len);
            }
            println("");
            println("Type 'help' for commands.");
        }
    }
}

// ── Entry point ────────────────────────────────────────────────────────────

#[unsafe(no_mangle)]
pub extern "C" fn _start(_args_ptr: *const u8, _args_len: usize) -> ! {
    println("");
    println("  ============================");
    println("    kaguyaOS v0.1.0");
    println("    Type 'help' for commands");
    println("  ============================");
    println("");

    let mut cmd_buf = [0u8; MAX_CMD_LEN];
    let mut cmd_len: usize;

    // ── Auto-exec diagnostic: run ls once at startup to test exec/yield/terminate flow ──
    println("");
    println("[auto-exec] Running ls.kef to test scheduler...");
    exec_program("ls.kef", "");
    println("[auto-exec] Returned to shell OK!");

    loop {
        std::print("kaguya> ");
        cmd_len = 0;

        loop {
            std::poll_xhci();
            let key = std::read_key() as u8;

            if key == 0 {
                continue;
            }

            if key == KEY_ENTER {
                std::print("\n");
                break;
            }

            if key == KEY_BACKSPACE {
                if cmd_len > 0 {
                    cmd_len -= 1;
                    std::print("\x08 \x08");
                }
                continue;
            }

            if key >= 0x20 && key < 0x7F && cmd_len < MAX_CMD_LEN - 1 {
                cmd_buf[cmd_len] = key;
                cmd_len += 1;
                let ch = [key];
                unsafe {
                    std::print(core::str::from_utf8_unchecked(&ch));
                }
            }
        }

        if cmd_len > 0 {
            process_command(cmd_buf.as_ptr(), cmd_len);
        }
    }
}
