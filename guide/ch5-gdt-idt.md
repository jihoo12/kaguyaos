# Chapter 5: GDT, TSS, and IDT

These three tables are the foundation of x86 protection and interrupt handling. The GDT defines memory segments, the TSS manages stack switching, and the IDT routes interrupts and exceptions to your handlers.

## Why You Need These

After `ExitBootServices()`, UEFI has already set up a basic GDT and paging. But if you want:

- **Ring 3 (user mode)** — you need GDT entries for user code/data segments
- **Interrupt handling** — you need an IDT pointing to your handlers
- **Stack switching on interrupts** — you need a TSS with RSP0

## The GDT

The Global Descriptor Table is an array of 8-byte entries (plus a 16-byte TSS entry). Each entry describes a memory segment with base, limit, and access rights.

kaguyaOS sets up 5 segments per CPU:

```rust
// src/gdt.rs:4-8
pub const KERNEL_CODE_SEL: u16 = 0x08;  // Ring 0 code
pub const KERNEL_DATA_SEL: u16 = 0x10;  // Ring 0 data
pub const USER_DATA_SEL: u16 = 0x1B;    // Ring 3 data  (0x10 | RPL=3)
pub const USER_CODE_SEL: u16 = 0x23;    // Ring 3 code  (0x18 | RPL=3)
pub const TSS_SEL: u16 = 0x28;          // Task State Segment
```

The GDT entry structure:

```rust
// src/gdt.rs:19-26
#[repr(C, packed)]
struct GdtEntry {
    limit_low: u16,
    base_low: u16,
    base_mid: u8,
    access: u8,
    granularity: u8,
    base_high: u8,
}
```

The `access` byte controls:
- **Present bit** (bit 7): Is this segment valid?
- **Descriptor type** (bit 4): Code (1) or Data (0)
- **DPL** (bits 5-6): Descriptor Privilege Level — 0 for kernel, 3 for user
- **Executable** (bit 3): Can code run in this segment?

## The TSS

The Task State Segment is used not for hardware task switching (that's deprecated), but for:

1. **RSP0** — the stack pointer the CPU switches to when entering Ring 0 from Ring 3 (on interrupts/syscalls)
2. **IST1-IST7** — Interrupt Stack Tables for specific critical handlers (like double faults)

```rust
// src/gdt.rs:50-66
#[repr(C, packed)]
struct Tss {
    reserved: u32,
    rsp0: u64,      // Stack for Ring 0 (kernel)
    rsp1: u64,
    rsp2: u64,
    reserved2: u64,
    ist1: u64,       // Interrupt Stack Table 1 (used for double faults)
    ist2: u64,
    ist3: u64,
    ist4: u64,
    ist5: u64,
    ist6: u64,
    ist7: u64,
    reserved3: u64,
    iomap_base: u16,
}
```

When a user-mode program triggers an interrupt or calls `syscall`, the CPU automatically loads RSP0 from the TSS, switching to the kernel stack.

## Setting Up the GDT

kaguyaOS has per-CPU GDTs (important for SMP — Chapter 15):

```rust
// src/gdt.rs:127-195 (simplified)
unsafe fn init_cpu(cpu_index: usize) {
    // 1. Build the GDT entries
    set_gdt_entry_cpu(cpu_index, 0, 0, 0, 0, 0);         // Null
    set_gdt_entry_cpu(cpu_index, 1, 0, 0xFFFFF, 0x9A, 0xA); // Kernel code
    set_gdt_entry_cpu(cpu_index, 2, 0, 0xFFFFF, 0x92, 0xC); // Kernel data
    set_gdt_entry_cpu(cpu_index, 3, 0, 0xFFFFF, 0xF2, 0xC); // User data
    set_gdt_entry_cpu(cpu_index, 4, 0, 0xFFFFF, 0xFA, 0xA); // User code

    // 2. Build the TSS GDT entry (16 bytes, spans two GDT slots)
    // ... sets up IST1 for double fault stack ...

    // 3. Load the GDT
    let gdtr = GdtPointer {
        limit: (size_of::<[GdtEntry; 5]>() + size_of::<GdtSystemEntry>() - 1) as u16,
        base: gdt_arr.as_ptr() as u64,
    };
    core::arch::asm!("lgdt [{}]", in(reg) &gdtr);

    // 4. Reload segment registers
    core::arch::asm!(
        "mov ax, {data_sel}",
        "mov ds, ax", "mov es, ax", "mov fs, ax", "mov gs, ax", "mov ss, ax",
        "push {code_sel}", "lea rax, [1f]", "push rax",
        "retfq", "1:",
        data_sel = const KERNEL_DATA_SEL,
        code_sel = const KERNEL_CODE_SEL,
    );

    // 5. Load the TSS
    core::arch::asm!("ltr ax", in(ax) TSS_SEL);
}
```

The reload sequence is critical: after `lgdt`, you must reload all segment registers and do a far jump/return to reload CS.

## The IDT

The Interrupt Descriptor Table is an array of 256 entries, one per possible interrupt vector. Each entry points to an assembly stub that saves registers and calls a Rust handler.

```rust
// src/interrupts.rs:65-75
#[repr(C, packed)]
struct IdtEntry {
    offset_low: u16,
    selector: u16,
    ist: u8,
    type_attr: u8,
    offset_mid: u16,
    offset_high: u32,
    reserved: u32,
}
```

kaguyaOS sets up vectors 0-31 for CPU exceptions and 32-47 for hardware IRQs:

```rust
// src/interrupts.rs:140-206 (simplified)
pub fn init_idt() {
    // Exceptions (0-31)
    set_gate(0, isr0 as u64, KERNEL_CODE_SEL, 0x8E);  // Divide Error
    set_gate(8, isr8 as u64, KERNEL_CODE_SEL, 0x8E);  // Double Fault
    set_gate(13, isr13 as u64, KERNEL_CODE_SEL, 0x8E); // General Protection Fault
    set_gate(14, isr14 as u64, KERNEL_CODE_SEL, 0x8E); // Page Fault

    // Hardware IRQs (32-47)
    set_gate(32, irq0 as u64, KERNEL_CODE_SEL, 0x8E);  // Timer
    set_gate(33, irq1 as u64, KERNEL_CODE_SEL, 0x8E);  // Keyboard

    // Load the IDT
    let idtr = IdtPointer {
        limit: (size_of::<[IdtEntry; 256]>() - 1) as u16,
        base: IDT.as_ptr() as u64,
    };
    core::arch::asm!("lidt [{}]", in(reg) &idtr);
}
```

The assembly stubs (e.g., `isr14` for page faults) save all registers and push the interrupt number, then jump to a common handler:

```
isr14:  ; Page Fault
    push 0          ; dummy error code (CPU pushes real one)
    push 14         ; interrupt number
    jmp isr_common
```

The `InterruptFrame` struct captures everything the CPU pushes:

```rust
// src/interrupts.rs:84-108
#[repr(C)]
struct InterruptFrame {
    r15: u64, r14: u64, r13: u64, r12: u64,
    r11: u64, r10: u64, r9: u64, r8: u64,
    rbp: u64, rdi: u64, rsi: u64,
    rdx: u64, rcx: u64, rbx: u64, rax: u64,
    int_no: u64, err_code: u64,
    rip: u64, cs: u64, rflags: u64,
    rsp: u64, ss: u64,
}
```

## Double Fault Handling

Double faults are special: if the IDT entry for a double fault doesn't point to a valid stack, the CPU triple-faults and reboots. kaguyaOS uses IST1 (a dedicated stack from the TSS) for this:

```rust
// src/gdt.rs:10
pub const DOUBLE_FAULT_IST_INDEX: u16 = 1;

// In init_idt():
IDT[8].ist = DOUBLE_FAULT_IST_INDEX as u8;  // Vector 8 = Double Fault
```

## Synchronization: The Interrupt-Safe Spinlock

Any data shared between normal code and interrupt handlers needs an interrupt-safe lock. The key insight: if your lock disables interrupts while held, an interrupt handler can't try to acquire the same lock on the same CPU (deadlock avoided).

```rust
// src/sync/mod.rs (simplified)
pub fn lock(&self) -> SpinlockGuard<T> {
    // 1. Save interrupt state
    let rflags = unsafe { /* read RFLAGS */ };
    unsafe { core::arch::asm!("cli"); }  // Disable interrupts

    // 2. Acquire the lock
    while self.lock.compare_exchange(false, true, ...).is_err() {
        core::hint::spin_loop();
    }

    // 3. Return guard (restores interrupts on drop)
    SpinlockGuard { lock: self, interrupts_enabled: (rflags & (1<<9)) != 0 }
}

impl<T> Drop for SpinlockGuard<'_, T> {
    fn drop(&mut self) {
        self.lock.lock.store(false, Ordering::Release);
        if self.interrupts_enabled {
            unsafe { core::arch::asm!("sti"); }  // Restore interrupts
        }
    }
}
```

This is used everywhere: the framebuffer writer, scheduler, heap allocator, network stack.

## What You Should See

After setting up GDT + IDT (but before enabling interrupts), nothing changes visually. The kernel runs exactly as before. But now you're ready for Chapter 6.

## kaguyaOS Reference

| File | Lines | What it does |
|------|-------|-------------|
| `src/gdt.rs` | 1-217 | Per-CPU GDT, TSS, IST setup |
| `src/interrupts.rs` | 1-210 | IDT setup, ISR/IRQ stubs |
| `src/sync/mod.rs` | 1-117 | Interrupt-safe Spinlock |
| `src/io.rs` | 1-46 | Port I/O (used by serial) |

---

**Next:** [Chapter 6 — Hardware Interrupts & Timer](ch6-interrupts.md) — Making the timer tick and the keyboard work.
