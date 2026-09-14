# Chapter 10: User Programs & KEF Format

Now we can run code in Ring 3. But what does that code look like? This chapter defines a minimal executable format (KEF), a loader to map it into user address space, and the runtime library that bridges user code to kernel syscalls.

## Why Not ELF?

ELF is the standard executable format, but parsing it is complex (program headers, dynamic linking, relocations). For a hobby OS, a custom format lets us keep the loader simple.

## The KEF Format

KEF (Kaguya Executable Format) is a flat binary with a 12-byte header:

```
Offset  Size  Field
0       4     Magic: "KEF\0" (0x4B, 0x45, 0x46, 0x00)
4       4     Entry offset (relative to start of code)
8       4     Code offset (always 12 — right after header)
12      N     Code bytes (raw machine code)
```

That's it. No relocations, no sections, no symbols. The code is position-dependent (compiled at a fixed base address).

## The Loader

`load_kef` reads the KEF blob and maps it into user virtual address space:

```rust
// src/loader.rs:14-50 (simplified)
pub unsafe fn load_kef(
    file_data: &[u8],
    allocator: &mut FrameAllocator,
    pml4: &mut PageTable,
) -> Result<(u64, u64), &'static str> {
    // 1. Validate header
    if file_data.len() < 12 {
        return Err("File too small");
    }
    let header = &*(file_data.as_ptr() as *const KefHeader);
    if header.magic != [b'K', b'E', b'F', 0] {
        return Err("Bad magic");
    }

    let code_offset = header.code_offset as usize;
    let code_size = header.code_size as usize;
    let entry_offset = header.entry_offset as usize;

    // 2. Allocate contiguous physical frames for code
    let code_pages = (code_size + 4095) / 4096;
    let mut code_phys = allocator.allocate_frame()
        .expect("OOM allocating code page");
    for _ in 1..code_pages {
        let next = allocator.allocate_frame()
            .expect("OOM allocating code pages");
        assert!(next == code_phys + 4096 * _, "Code pages must be contiguous");
    }

    // 3. Map code pages as user-accessible
    let code_virt = code_phys;  // Identity-map for simplicity
    for i in 0..code_pages as u64 {
        map_page(pml4, code_virt + i * 4096, code_phys + i * 4096,
            PAGE_PRESENT | PAGE_WRITABLE | PAGE_USER, allocator);
    }

    // 4. Copy code from file blob
    core::ptr::copy_nonoverlapping(
        file_data[code_offset..].as_ptr(),
        code_virt as *mut u8,
        code_size,
    );

    // 5. Allocate user stack (16 KiB)
    let stack_phys = allocator.allocate_frame().expect("OOM allocating stack");
    map_page(pml4, stack_phys, stack_phys,
        PAGE_PRESENT | PAGE_WRITABLE | PAGE_USER, allocator);
    let user_rsp = stack_phys + 16384;  // Stack grows downward

    let entry_point = code_virt + entry_offset as u64;
    Ok((entry_point, user_rsp))
}
```

## Building the User Task

The scheduler needs to set up a complete CPU state to enter user mode. This means building two stack frames:

### Frame 1: IRETQ Frame (hardware-level user entry)

When the kernel executes `iretq`, the CPU pops:
```
SS      → Stack segment (user data)
RSP     → User stack pointer
RFLAGS  → Interrupt flags (IF=1 for interrupts)
CS      → Code segment (user code)
RIP     → Entry point
```

### Frame 2: Context Switch Frame (software)

When a context switch occurs, the kernel saves/restores:
```
RAX, RBX, RCX, RDX, RSI, RDI, RBP
R8-R15
RSP, RIP (return address — points to user_task_trampoline)
```

```rust
// src/process/mod.rs:62-100 (simplified)
pub fn add_new_user_task(entry: u64, user_rsp: u64, ...) {
    let kernel_stack_bottom = allocator::alloc(stack_size) as u64;
    let kernel_stack_top = kernel_stack_bottom + stack_size as u64;

    // Build IRETQ frame at the top of kernel stack
    let mut sp = kernel_stack_top as *mut u64;
    sp = sp.sub(1); unsafe { *sp = USER_DATA_SEL as u64; }  // SS
    sp = sp.sub(1); unsafe { *sp = user_rsp; }               // RSP
    sp = sp.sub(1); unsafe { *sp = 0x202; }                  // RFLAGS (IF=1)
    sp = sp.sub(1); unsafe { *sp = USER_CODE_SEL as u64; }   // CS
    sp = sp.sub(1); unsafe { *sp = entry; }                  // RIP

    // Build context switch frame
    sp = sp.sub(1); unsafe { *sp = user_task_trampoline as u64; } // Return addr
    // ... push callee-saved registers ...

    Task {
        id: NEXT_TASK_ID.fetch_add(1, Ordering::SeqCst),
        stack_top: sp as u64,
        user_rsp,
        status: TaskStatus::Ready,
        // ...
    }
}
```

## The User Runtime Library

User programs are `#![no_std]` and `#![no_main]` — they need their own entry point and syscall wrappers:

```rust
// user/src/std.rs
#![no_std]

pub unsafe fn syscall0(id: usize) -> usize {
    let ret: usize;
    core::arch::asm!("syscall",
        inlateout("rax") id => ret,
        out("rcx") _, out("r11") _,
    );
    ret
}

pub fn print(s: &str) {
    unsafe {
        syscall2(0, s.as_ptr() as usize, s.len());
    }
}

pub fn read_key() -> u8 {
    unsafe { syscall0(8) as u8 }
}

pub fn yield_task() {
    unsafe { syscall0(4); }
}

pub fn shutdown() {
    unsafe { syscall0(7); }
}

pub fn alloc(size: usize, align: usize) -> *mut u8 {
    unsafe { syscall2(1, size, align) as *mut u8 }
}
```

## Building User Programs

User programs are compiled to flat binaries (`--oformat=binary`):

```bash
# user/build.sh (simplified)
rustc --edition 2021 --target x86_64-unknown-none \
    -C "link-args=--oformat=binary" \
    -o init.kef src/init.rs
```

Then inserted into the NVMe image with `kef-tool`:

```bash
tools/kef-tool/target/debug/kef-tool insert nvme.img init.kef init.kef
```

## kaguyaOS Reference

| File | Lines | What it does |
|------|-------|-------------|
| `src/loader.rs` | 1-102 | KEF header, load_kef() |
| `src/process/mod.rs` | 62-100 | add_new_user_task() |
| `user/src/std.rs` | 1-84 | Syscall wrappers |
| `user/build.sh` | 1-30 | Compilation + insertion |

---

**Next:** [Chapter 11 — Building a Shell](ch11-shell.md) — An interactive command line in Ring 3.
