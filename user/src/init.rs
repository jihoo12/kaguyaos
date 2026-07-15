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
const MAX_PATH_LEN: usize = 47;

unsafe fn slice_from_raw<'a>(ptr: *const u8, len: usize) -> &'a [u8] {
    core::slice::from_raw_parts(ptr, len)
}

unsafe fn str_from_raw<'a>(ptr: *const u8, len: usize) -> &'a str {
    let bytes = slice_from_raw(ptr, len);
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] >= 0x80 {
            return "";
        }
        i += 1;
    }
    core::str::from_utf8_unchecked(bytes)
}

fn str_len(s: &str) -> usize {
    s.as_bytes().len()
}

fn str_eq(a: &str, b: &str) -> bool {
    let ab = a.as_bytes();
    let bb = b.as_bytes();
    if ab.len() != bb.len() {
        return false;
    }
    let mut i = 0;
    while i < ab.len() {
        if ab[i] != bb[i] {
            return false;
        }
        i += 1;
    }
    true
}

unsafe fn trim_start(s: &str) -> &str {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
        i += 1;
    }
    str_from_raw(bytes.as_ptr().add(i), bytes.len() - i)
}

unsafe fn substr(s: &str, start: usize, end: usize) -> &str {
    let bytes = s.as_bytes();
    let s = if start <= bytes.len() { start } else { bytes.len() };
    let e = if end <= bytes.len() { end } else { bytes.len() };
    str_from_raw(bytes.as_ptr().add(s), e - s)
}

fn str_empty(s: &str) -> bool {
    s.as_bytes().len() == 0
}

fn find_space(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b' ' || bytes[i] == b'\t' {
            return Some(i);
        }
        i += 1;
    }
    None
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
        std::print(str_from_raw(buf.as_ptr().add(i), 20 - i));
    }
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

// ── Built-in commands ──────────────────────────────────────────────────────

fn cmd_help() {
    println("Commands:");
    println("  help              Show this help");
    println("  ls                List files");
    println("  cat <file>        Show file contents");
    println("  write <file> <msg> Write msg to a file");
    println("  rm <file>         Delete a file");
    println("  clear             Clear screen");
    println("  shutdown          Shut down");
}

fn cmd_ls() {
    let mut entries = [std::FileEntry {
        name: [0u8; 47],
        name_len: 0,
        size: 0,
        first_cluster: 0,
    }; 16];

    let count = std::fs_ls(&mut entries);
    if count < 0 {
        println("Error: failed to list files");
        return;
    }
    if count == 0 {
        println("No files.");
        return;
    }

    println("Name                      Size");
    println("--------------------------------");
    let n = if count as usize > 16 { 16 } else { count as usize };
    let mut i = 0;
    while i < n {
        let name_len = entries[i].name_len as usize;
        let name = if name_len <= entries[i].name.len() {
            unsafe { str_from_raw(entries[i].name.as_ptr(), name_len) }
        } else {
            "???"
        };
        std::print(name);
        let name_l = str_len(name);
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
}

fn cmd_cat(filename: &str) {
    if str_empty(filename) {
        println("Usage: cat <filename>");
        return;
    }

    let size = std::fs_read(filename, &mut []);
    if size < 0 {
        std::print("Error: ");
        std::print(filename);
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
    let read = std::fs_read(filename, slice);
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

fn cmd_write(args: &str) {
    unsafe {
        let trimmed = trim_start(args);
        if str_empty(trimmed) {
            println("Usage: write <filename> <content>");
            return;
        }

        let (filename, content_len, content_ptr) = match find_space(trimmed) {
            Some(pos) => {
                let fname = substr(trimmed, 0, pos);
                let rest = trim_start(substr(trimmed, pos, str_len(trimmed)));
                (fname, str_len(rest), rest.as_ptr())
            }
            None => (trimmed, 0, trimmed.as_ptr()),
        };

        if str_len(filename) > MAX_PATH_LEN {
            println("Error: filename too long");
            return;
        }

        let content = if content_len > 0 {
            slice_from_raw(content_ptr, content_len)
        } else {
            &[]
        };
        let ret = std::fs_write(filename, content);
        if ret != 0 {
            println("Error: write failed");
        } else {
            std::print("Wrote ");
            print_u64(content_len as u64);
            println(" bytes");
        }
    }
}

fn cmd_rm(filename: &str) {
    if str_empty(filename) {
        println("Usage: rm <filename>");
        return;
    }
    let ret = std::fs_rm(filename);
    if ret != 0 {
        std::print("Error: cannot delete ");
        println(filename);
    } else {
        std::print("Deleted ");
        println(filename);
    }
}

// ── Command dispatch ───────────────────────────────────────────────────────

fn process_command(cmd: &str) {
    unsafe {
        let trimmed = trim_start(cmd);
        if str_empty(trimmed) {
            return;
        }

        let (cmd_name, args_start) = match find_space(trimmed) {
            Some(pos) => (substr(trimmed, 0, pos), pos),
            None => (trimmed, str_len(trimmed)),
        };

        let args = substr(trimmed, args_start, str_len(trimmed));

        if str_eq(cmd_name, "help") {
            cmd_help();
        } else if str_eq(cmd_name, "ls") {
            cmd_ls();
        } else if str_eq(cmd_name, "cat") {
            let a = trim_start(args);
            cmd_cat(a);
        } else if str_eq(cmd_name, "write") {
            cmd_write(args);
        } else if str_eq(cmd_name, "rm") {
            let a = trim_start(args);
            cmd_rm(a);
        } else if str_eq(cmd_name, "clear") {
            std::clear();
        } else if str_eq(cmd_name, "shutdown") {
            println("Goodbye!");
            std::shutdown();
        } else {
            std::print("Unknown: ");
            println(cmd_name);
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
            unsafe {
                let cmd_str = str_from_raw(cmd_buf.as_ptr(), cmd_len);
                process_command(cmd_str);
            }
        }
    }
}
