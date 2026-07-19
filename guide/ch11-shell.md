# Chapter 11: Building a Shell

Every OS needs a shell. This chapter builds `init.kef` — an interactive command-line that reads keyboard input, parses commands, and dispatches to built-in utilities.

## Keyboard Input

kaguyaOS uses USB keyboard via xHCI (Chapter 12). The user-space approach is polling:

```rust
// user/src/init.rs (simplified loop)
fn main() {
    loop {
        let key = std::read_key();  // syscall 8
        if key != 0 {
            handle_key(key);
        }
        std::yield_task();  // Don't busy-loop — yield to other tasks
    }
}
```

The `read_key` syscall reads from a shared scancode buffer that the xHCI IRQ handler fills.

## Command Parsing

The shell maintains a line buffer and processes keys:

```rust
// user/src/init.rs
static mut CMD_BUF: [u8; 128] = [0u8; 128];
static mut CMD_LEN: usize = 0;

fn handle_key(key: u8) {
    match key {
        0x0A => {  // Enter
            process_command();
            CMD_LEN = 0;
        }
        0x08 => {  // Backspace
            if CMD_LEN > 0 {
                CMD_LEN -= 1;
                print("\x08");  // Erase character on screen
            }
        }
        c if c >= 0x20 && c < 0x7F => {  // Printable ASCII
            if CMD_LEN < 127 {
                CMD_BUF[CMD_LEN] = c;
                CMD_LEN += 1;
                print_core(&[c]);  // Echo character
            }
        }
        _ => {}
    }
}
```

## Command Dispatch

```rust
fn process_command() {
    print("\n");
    let cmd = core::str::from_utf8(&CMD_BUF[..CMD_LEN]).unwrap_or("");

    if cmd.starts_with("ls") {
        sys_list_files();
    } else if cmd.starts_with("cat ") {
        let filename = &cmd[4..];
        sys_read_file(filename);
    } else if cmd.starts_with("write ") {
        let rest = &cmd[6..];
        if let Some(pos) = rest.find(' ') {
            let filename = &rest[..pos];
            let content = &rest[pos+1..];
            sys_create_file(filename, content.as_bytes());
        }
    } else if cmd.starts_with("rm ") {
        sys_delete_file(&cmd[3..]);
    } else if cmd.starts_with("exec ") {
        sys_exec2(&cmd[5..]);
    } else if cmd == "clear" {
        sys_clear();
    } else if cmd == "help" {
        print("Available commands:\n");
        print("  ls              - List files\n");
        print("  cat <file>      - Read file\n");
        print("  write <f> <msg> - Write to file\n");
        print("  rm <file>       - Delete file\n");
        print("  exec <file>     - Run program\n");
        print("  clear           - Clear screen\n");
        print("  shutdown        - Power off\n");
    } else if cmd == "shutdown" {
        sys_shutdown();
    } else {
        print("Unknown command. Type 'help'.\n");
    }
}
```

## Cooperative Yielding

The shell runs in a loop. Without yielding, it would starve other tasks:

```rust
// user/src/init.rs:215
loop {
    let key = std::read_key();
    if key != 0 {
        handle_key(key);
    }
    std::yield_task();  // syscall 4 — lets scheduler run other tasks
}
```

The `yield_task()` syscall is critical. Without it, the shell's busy loop would never give the scheduler a chance to run child processes (like `ping.kef`).

## Terminal Rendering

For a more polished shell, kaguyaOS has a cell-based terminal renderer (`src/console/term.rs`). It renders characters as 8×16 pixel cells directly to the framebuffer:

```rust
// src/console/term.rs
pub struct CellRenderer {
    framebuffer: *mut u32,
    stride: usize,   // pixels per scanline
    cols: usize,     // horizontal_resolution / 8
    rows: usize,     // vertical_resolution / 16
}

impl CellRenderer {
    pub fn write_cell(&mut self, row: usize, col: usize,
                      ch: char, fg: u32, bg: u32) {
        if row >= self.rows || col >= self.cols { return; }

        let bitmap = BASIC_FONTS.get(ch).unwrap();
        let x_base = col * 8;
        let y_base = row * 16;

        for (dy, byte) in bitmap.iter().enumerate() {
            for dx in 0..8 {
                let color = if (byte >> dx) & 1 == 1 { fg } else { bg };
                let offset = (y_base + dy) * self.stride + (x_base + dx);
                unsafe { *self.framebuffer.add(offset) = color; }
            }
        }
    }
}
```

## Running External Programs

The `exec` command loads a KEF binary from the filesystem and creates a new user task:

```rust
// Syscall 21 (sys_exec2):
// 1. Read the KEF file from FAT16
// 2. Call loader::load_kef() to map code + stack
// 3. Call process::add_new_user_task() to create the task
// 4. Return — the new task runs when the scheduler picks it
```

After `exec`, the shell continues running. The new process runs in parallel via cooperative scheduling.

## Panic Handling in User Mode

If a user program panics, it should print a message and terminate (not crash the kernel):

```rust
// user/src/init.rs
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    // In a real OS, we'd use a syscall to print the panic message.
    // For now, just halt.
    loop {}
}
```

kaguyaOS's kernel-side panic handler checks CPL (Current Privilege Level) and dispatches appropriately:

```rust
// src/main.rs:12-41
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    let cs: u16;
    unsafe { core::arch::asm!("mov {0:x}, cs", out(reg) cs); }

    if cs & 0x03 == 3 {
        // User mode: use syscall to print
        let msg = "PANIC in User Mode!\n";
        unsafe {
            core::arch::asm!("syscall",
                in("rax") 0,
                in("rdi") msg.as_ptr(),
                in("rsi") msg.len(),
            );
        }
    } else {
        // Kernel mode: use println!
        println!("{}", _info);
    }
    loop {}
}
```

## kaguyaOS Reference

| File | Lines | What it does |
|------|-------|-------------|
| `user/src/init.rs` | 1-248 | Shell: input, commands, exec |
| `src/console/term.rs` | 1-68 | Cell-based terminal renderer |
| `src/main.rs` | 12-41 | Dual-mode panic handler |

---

**Next:** [Chapter 12 — Device Drivers](ch12-drivers.md) — PCI, NVMe, USB, and Ethernet.
