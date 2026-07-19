# Chapter 9: Ring 0/3 Isolation & Syscalls

This is where your OS becomes a real operating system: running untrusted code in Ring 3 (user mode) while the kernel stays in Ring 0. The CPU enforces the boundary — user code literally cannot execute privileged instructions.

## The Privilege Levels

x86_64 has 4 rings (0-3), but most OSes only use 2:

| Ring | Name | Can do |
|------|------|--------|
| 0 | Kernel | Everything (I/O, `cli`/`sti`, `mov cr3`, etc.) |
| 3 | User | Nothing privileged — any violation = General Protection Fault |

## Setting Up User Segments

The GDT needs user-mode code and data segments (from Chapter 5):

```rust
// User data: Ring 3, writable
set_gdt_entry_cpu(cpu, 3, 0, 0xFFFFF, 0xF2, 0xC);
// User code: Ring 3, executable
set_gdt_entry_cpu(cpu, 4, 0, 0xFFFFF, 0xFA, 0xA);
```

The RPL (Requested Privilege Level) bits in the selector encode the ring:
- `0x1B` = `0x18 | 3` = user data (Ring 3)
- `0x23` = `0x20 | 3` = user code (Ring 3)

## The syscall/ sysret Mechanism

x86_64 provides a fast user→kernel transition via the `syscall` instruction. It's faster than software interrupts because:

1. No IDT lookup — the target address is in a Model-Specific Register (MSR)
2. Minimal register save — only `rcx` (return RIP) and `r11` (return RFLAGS) are clobbered
3. Stack switch is implicit via the TSS RSP0

### MSR Configuration

```rust
// src/syscall.rs:61-82 (simplified)
pub fn init_cpu() {
    unsafe {
        // Enable SCE (System Call Extensions) in EFER
        let efer = crate::processor::rdmsr(MSR_EFER);
        crate::processor::wrmsr(MSR_EFER, efer | EFER_SCE);

        // STAR: segment selectors for kernel/user code
        // Bits 63-48: user CS (0x23) and user SS (0x1B)
        // Bits 47-32: kernel CS (0x08) and kernel SS (0x10)
        let star_val: u64 = (0x23 << 48) | (0x1B << 40)
                          | (0x08 << 32) | (0x10 << 24);
        crate::processor::wrmsr(MSR_STAR, star_val);

        // LSTAR: kernel entry point (where `syscall` jumps to)
        crate::processor::wrmsr(MSR_LSTAR, syscall_handler as u64);

        // SFMASK: mask bits from RFLAGS on syscall (clear IF to disable interrupts)
        crate::processor::wrmsr(MSR_SFMASK, 0x200);
    }
}
```

When user code executes `syscall`:
1. CPU saves `rip` → `rcx`, `rflags` → `r11`
2. CPU loads `rip` from `LSTAR`
3. CPU loads `cs`/`ss` from `STAR` (kernel segments)
4. CPU loads `rflags` with `RFLAGS & ~SFMASK` (interrupts disabled)
5. CPU loads `rsp` from TSS.RSP0 (kernel stack)

When kernel executes `sysret`:
1. CPU restores `rip` from `rcx`, `rflags` from `r11`
2. CPU loads `cs`/`ss` from `STAR` (user segments)
3. CPU loads `rsp` from... `rcx` is user RIP, `rsp` stays

## The Syscall Entry Point

kaguyaOS's `syscall_handler` is a naked function that sets up the kernel environment:

```rust
// src/syscall.rs (simplified)
#[naked]
pub unsafe extern "sysv64" fn syscall_handler() {
    core::arch::asm!(
        "swapgs",                          // Switch to kernel GS base
        "mov gs:[{off_stack}], rsp",       // Save user RSP
        "mov rsp, gs:[{off_kernel_stack}]", // Load kernel RSP
        "push rcx",                        // Save user RIP
        "push r11",                        // Save user RFLAGS
        "push rax",                        // Syscall number
        // ... save registers, call handler ...
        "pop rax",                         // Syscall number → arg
        "call {handler}",
        // ... restore registers ...
        "pop r11",                         // Restore RFLAGS
        "pop rcx",                         // Restore RIP
        "swapgs",                          // Switch back to user GS
        "sysretq",
        handler = sym syscall_dispatch,
        off_stack = const OFF_USER_STACK,
        off_kernel_stack = const OFF_KERNEL_STACK,
    );
}
```

The `swapgs` instruction swaps the GS base between a kernel-mode and user-mode value. kaguyaOS uses this to store per-CPU data (including stack pointers) in the GS-segmented area.

## The Dispatch Table

Syscalls are dispatched by number from `rax`:

```rust
// src/syscall.rs (simplified)
unsafe fn syscall_dispatch(
    id: u64, arg1: u64, arg2: u64, arg3: u64, arg4: u64,
) -> isize {
    match id {
        0  => sys_print(arg1, arg2),          // print(ptr, len)
        1  => sys_alloc(arg1, arg2),          // alloc(size, align)
        2  => sys_free(arg1),                 // free(ptr)
        3  => sys_realloc(arg1, arg2, arg3),  // realloc(ptr, size, align)
        4  => sys_yield(),                    // yield_task()
        5  => sys_exec(arg1, arg2),           // exec(filename_ptr, filename_len)
        6  => sys_poll_xhci(),                // poll keyboard
        7  => sys_shutdown(),                 // power off
        8  => sys_read_key(),                 // read keyboard scancode
        9  => sys_wait(arg1),                 // wait(task_id)
        10 => sys_list_files(),               // list files
        11 => sys_create_file(arg1, arg2, arg3, arg4),
        12 => sys_read_file(arg1, arg2, arg3),
        13 => sys_delete_file(arg1, arg2),
        14 => sys_clear(),                    // clear screen
        15 => sys_get_task_status(arg1),
        16 => sys_get_task_exit_code(arg1),
        17 => sys_format(),                   // format filesystem
        18 => sys_run_network_poller(),       // start AP network polling
        19 => sys_exec_with_args(arg1, arg2, arg3, arg4),
        20 => sys_run_ap_scheduler(),         // park AP in scheduler loop
        21 => sys_exec2(arg1, arg2, arg3, arg4),
        22 => sys_set_terminal_mode(arg1),
        23 => sys_net_send_ping(arg1),
        24 => sys_net_recv_ping(arg1, arg2),
        _  => {
            println!("Unknown syscall: {}", id);
            -1
        }
    }
}
```

## User-Space Side

The user-space `std.rs` provides Rust wrappers around raw `syscall` instructions:

```rust
// user/src/std.rs:6-24
pub unsafe fn syscall0(id: usize) -> usize {
    let ret: usize;
    core::arch::asm!(
        "syscall",
        inlateout("rax") id => ret,
        out("rcx") _,
        out("r11") _,
        options(nostack, preserves_flags)
    );
    ret
}

pub fn print(s: &str) {
    unsafe { syscall0(0); }  // simplified — actual uses syscall2
}
```

## Validating User Pointers

Before dereferencing any pointer from user space, we must validate it:

```rust
// src/syscall.rs:7-22
unsafe fn user_range_ok(ptr: *const u8, len: usize) -> bool {
    // Check that the pointer range is in user-space addresses
    // and that every page is mapped in the current page tables
    let pml4 = memory::get_table_mut(memory::current_pml4_phys());
    memory::is_range_mapped(pml4, ptr as u64, len as u64)
}
```

Without this, a malicious user program could pass a kernel address to `sys_print` and leak kernel memory.

## kaguyaOS Reference

| File | Lines | What it does |
|------|-------|-------------|
| `src/syscall.rs` | 25-82 | MSR setup, init_cpu() |
| `src/syscall.rs` | 107+ | syscall_handler (naked asm) |
| `src/syscall.rs` | 200+ | syscall_dispatch |
| `user/src/std.rs` | 1-84 | syscall stubs (syscall0-4) |

---

**Next:** [Chapter 10 — User Programs & KEF Format](ch10-user-programs.md) — Loading and running user-mode binaries.
