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
fn cmd_ls() {
    unsafe {
        let buf_ptr = std::alloc(16 * core::mem::size_of::<std::FileEntry>(), 8);
        if buf_ptr.is_null() {
            println("Error: out of memory");
            return;
        }
        let entries = core::slice::from_raw_parts_mut(
            buf_ptr as *mut std::FileEntry,
            16,
        );

        let count = std::fs_ls(entries);
        if count < 0 {
            println("Error: failed to list files");
            std::free(buf_ptr);
            return;
        }
        if count == 0 {
            println("No files.");
            std::free(buf_ptr);
            return;
        }

        println("Name                      Size");
        println("--------------------------------");
        let n = if count as usize > 16 { 16 } else { count as usize };
        let mut i = 0;
        while i < n {
            let name_len = entries[i].name_len as usize;
            if name_len <= entries[i].name.len() {
                print_raw(entries[i].name.as_ptr(), name_len);
            } else {
                print("???");
            }
            let name_l = name_len;
            let pad = if 25 > name_l { 25 - name_l } else { 0 };
            let mut p = 0;
            while p < pad {
                put_char(b' ');
                p += 1;
            }
            print_u64(entries[i].size);
            std::print("\n");
            i += 1;
        }
        std::free(buf_ptr);
    }
}

#[inline(never)]
fn cmd_cat(args_ptr: *const u8, args_len: usize) {
    if args_len == 0 {
        println("Usage: cat <filename>");
        return;
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
        return;
    }

    let fname_str = unsafe { core::str::from_utf8_unchecked(core::slice::from_raw_parts(filename, fname_len)) };

    let size = std::fs_read(fname_str, &mut []);
    if size < 0 {
        print("Error: ");
        print(fname_str);
        println(" not found");
        return;
    }
    if size == 0 {
        println("(empty)");
        return;
    }

    let buf = std::alloc(size as usize, 1);
    if buf.is_null() {
        println("Error: out of memory");
        return;
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
}

#[inline(never)]
fn cmd_write(args_ptr: *const u8, args_len: usize) {
    unsafe {
        let mut start = 0;
        while start < args_len && (*args_ptr.add(start) == b' ' || *args_ptr.add(start) == b'\t') {
            start += 1;
        }
        let trimmed_ptr = args_ptr.add(start);
        let trimmed_len = args_len - start;

        if trimmed_len == 0 {
            println("Usage: write <filename> <content>");
            return;
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
                    return;
                }

                let fname = core::str::from_utf8_unchecked(
                    core::slice::from_raw_parts(fname_ptr, fname_len),
                );

                if content_len > 0 {
                    let content = core::slice::from_raw_parts(content_ptr, content_len);
                    let ret = std::fs_write(fname, content);
                    if ret != 0 {
                        println("Error: write failed");
                    } else {
                        std::print("Wrote ");
                        print_u64(content_len as u64);
                        println(" bytes");
                    }
                } else {
                    let ret = std::fs_write(fname, &[]);
                    if ret != 0 {
                        println("Error: write failed");
                    } else {
                        std::print("Wrote 0 bytes");
                        println("");
                    }
                }
            }
            None => {
                if trimmed_len > 47 {
                    println("Error: filename too long");
                    return;
                }
                let fname = core::str::from_utf8_unchecked(
                    core::slice::from_raw_parts(trimmed_ptr, trimmed_len),
                );
                let ret = std::fs_write(fname, &[]);
                if ret != 0 {
                    println("Error: write failed");
                } else {
                    println("Wrote 0 bytes");
                }
            }
        }
    }
}

#[inline(never)]
fn cmd_rm(args_ptr: *const u8, args_len: usize) {
    if args_len == 0 {
        println("Usage: rm <filename>");
        return;
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
        return;
    }

    let fname_str = unsafe { core::str::from_utf8_unchecked(core::slice::from_raw_parts(filename, fname_len)) };
    let ret = std::fs_rm(fname_str);
    if ret != 0 {
        print("Error: cannot delete ");
        println(fname_str);
    } else {
        print("Deleted ");
        println(fname_str);
    }
}

#[inline(never)]
fn cmd_exec(args_ptr: *const u8, args_len: usize) {
    if args_len == 0 {
        println("Usage: exec <filename.kef>");
        return;
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
        println("Usage: exec <filename.kef>");
        return;
    }

    let fname_str = unsafe { core::str::from_utf8_unchecked(core::slice::from_raw_parts(filename, fname_len)) };
    let task_id = std::exec(fname_str);
    if task_id == usize::MAX {
        print("Error: failed to execute ");
        println(fname_str);
    } else {
        print("Started task ");
        print_u64(task_id as u64);
        println("");
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
            cmd_ls();
        } else if bytes_eq(cmd_ptr, cmd_len, b"cat") {
            cmd_cat(args_ptr, args_len);
        } else if bytes_eq(cmd_ptr, cmd_len, b"write") {
            cmd_write(args_ptr, args_len);
        } else if bytes_eq(cmd_ptr, cmd_len, b"rm") {
            cmd_rm(args_ptr, args_len);
        } else if bytes_eq(cmd_ptr, cmd_len, b"clear") {
            std::clear();
        } else if bytes_eq(cmd_ptr, cmd_len, b"exec") {
            cmd_exec(args_ptr, args_len);
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
pub extern "C" fn _start() -> ! {
    println("");
    println("  ============================");
    println("    kaguyaOS v0.1.0");
    println("    Type 'help' for commands");
    println("  ============================");
    println("");

    let mut cmd_buf = [0u8; MAX_CMD_LEN];
    let mut cmd_len: usize;

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
