#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

mod std;

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

#[unsafe(no_mangle)]
pub extern "C" fn _start(_args_ptr: *const u8, _args_len: usize) -> ! {
    unsafe {
        let buf_ptr = std::alloc(16 * core::mem::size_of::<std::FileEntry>(), 8);
        if buf_ptr.is_null() {
            println("Error: out of memory");
            std::terminate_task(1);
        }
        let entries = core::slice::from_raw_parts_mut(buf_ptr as *mut std::FileEntry, 16);

        let count = std::fs_ls(entries);
        if count < 0 {
            println("Error: failed to list files");
            std::free(buf_ptr);
            std::terminate_task(1);
        }
        if count == 0 {
            println("No files.");
            std::free(buf_ptr);
            std::terminate_task(0);
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
    std::terminate_task(0);
    loop {}
}
