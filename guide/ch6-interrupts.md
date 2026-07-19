# Chapter 6: Hardware Interrupts & Timer

With the IDT in place, we can now handle hardware interrupts. This chapter covers the 8259 PIC, the 8254 PIT timer, and how to route IRQs to your Rust handlers.

## Interrupt Flow

```
Hardware Device (timer, keyboard)
    │
    ▼
8259 PIC (remaps IRQ 0-15 → vectors 32-47)
    │
    ▼
CPU reads vector number from PIC
    │
    ▼
CPU looks up IDT[vector]
    │
    ▼
Jumps to ISR stub (assembly)
    │
    ▼
ISR saves registers, calls Rust irq_handler()
    │
    ▼
irq_handler() dispatches by vector number
    │
    ▼
Sends EOI (End of Interrupt) to PIC
```

## The 8259 PIC

The 8259 Programmable Interrupt Controller is a legacy chip (or emulated by the APIC) that manages hardware interrupts. By default, IRQ 0-7 map to vectors 8-15, which conflict with CPU exceptions. We must remap them.

```rust
// src/pic.rs (simplified)
pub fn init() {
    // 1. Save current masks
    let mask1 = inb(PIC1_DATA);
    let mask2 = inb(PIC2_DATA);

    // 2. Begin PIC initialization (ICW1-ICW4 sequence)
    outb(PIC1_COMMAND, ICW1_INIT | ICW1_ICW4);  // Begin init
    outb(PIC2_COMMAND, ICW1_INIT | ICW1_ICW4);
    outb(PIC1_DATA, 0x20);    // Master PIC: vectors 32-39
    outb(PIC2_DATA, 0x28);    // Slave PIC: vectors 40-47
    outb(PIC1_DATA, 4);       // Slave PIC connected to IRQ2
    outb(PIC2_DATA, 2);       // Slave PIC identity
    outb(PIC1_DATA, ICW4_8086);
    outb(PIC2_DATA, ICW4_8086);

    // 3. Restore masks
    outb(PIC1_DATA, mask1);
    outb(PIC2_DATA, mask2);
}
```

After this, IRQ 0 → vector 32, IRQ 1 → vector 33, ..., IRQ 15 → vector 47.

## The 8254 PIT Timer

The Programmable Interval Timer generates periodic interrupts. Channel 0 is connected to IRQ 0 (vector 32). The PIT runs at 1,193,182 Hz; we configure it to divide down to ~100 Hz (one tick every 10ms):

```rust
// src/pic.rs:63-75
const PIT_FREQUENCY: u32 = 1_193_182;
const TARGET_HZ: u32 = 100;
const DIVISOR: u32 = PIT_FREQUENCY / TARGET_HZ;  // 11932

// Configure PIT Channel 0
outb(0x43, 0x36);  // Channel 0, lobyte/hibyte, square wave
outb(0x40, (DIVISOR & 0xFF) as u8);       // Low byte
outb(0x40, ((DIVISOR >> 8) & 0xFF) as u8); // High byte
```

## Unmasking IRQs

By default, most IRQs are masked (disabled). We only need timer and keyboard:

```rust
// Unmask IRQ 0 (timer) and IRQ 1 (keyboard)
outb(PIC1_DATA, 0xFC);  // 11111100 = all masked except 0 and 1
```

The mask bits: 0 = unmasked, 1 = masked. `0xFC` = bits 2-7 masked, bits 0-1 unmasked.

## Enabling Interrupts

After PIC + PIT init, enable interrupts with `sti`:

```rust
// src/main.rs:131-132
pic::init();
core::arch::asm!("sti");
```

**Never call `sti` before your IDT is loaded and your handlers are ready!** An interrupt with no handler → triple fault → reboot.

## The IRQ Handler

kaguyaOS's `irq_handler` dispatches by vector number:

```rust
// src/interrupts.rs:244-278 (simplified)
pub extern "sysv64" fn irq_handler(frame: *mut InterruptFrame) {
    let int_no = unsafe { (*frame).int_no };

    match int_no {
        32 => {
            // Timer IRQ 0
            // In practice, the preemption check (cs & 3 == 3) rarely
            // triggers because the shell is in kernel mode during syscalls.
            // Real multitasking comes from cooperative yielding (Ch 16).
        }
        33 => {
            // Keyboard IRQ 1 — read scancode from port 0x60
            let scancode: u8;
            unsafe {
                core::arch::asm!("in al, dx", out(al) scancode, in(dx) 0x60u16);
            }
            // Store scancode for polling (Ch 12)
        }
        _ => {}
    }

    // Send End-of-Interrupt to PIC
    unsafe { crate::pic::notify_eoi(int_no as u8 - 32); }
}
```

## EOI: Don't Forget!

After handling an interrupt, you **must** send EOI to the PIC. Without it, the PIC won't deliver another interrupt on that line:

```rust
// src/pic.rs:79-86
pub fn notify_eoi(irq: u8) {
    if irq >= 8 {
        outb(PIC2_COMMAND, 0x20);  // EOI to slave PIC
    }
    outb(PIC1_COMMAND, 0x20);      // EOI to master PIC
}
```

## Exception Handling

CPU exceptions (vectors 0-31) are different from hardware IRQs. They indicate programming errors:

| Vector | Name | Cause |
|--------|------|-------|
| 0 | Divide Error | Division by zero |
| 6 | Invalid Opcode | Bad instruction encoding |
| 8 | Double Fault | Exception during exception handler |
| 13 | General Protection Fault | Ring violation, bad segment |
| 14 | Page Fault | Accessing unmapped page |

kaguyaOS's exception handler dumps registers for debugging:

```rust
// src/interrupts.rs:280-349 (simplified)
pub extern "sysv64" fn exception_handler(frame: *mut InterruptFrame) {
    let f = unsafe { &*frame };

    println!("\nEXCEPTION OCCURRED!");
    println!("INTERRUPT: {:#x} ({})", f.int_no,
        EXCEPTION_MESSAGES[f.int_no as usize]);
    println!("ERROR CODE: {:#x}", f.err_code);
    println!("RIP: {:#x}  RSP: {:#x}", f.rip, f.rsp);
    println!("RAX: {:#x}  RBX: {:#x}", f.rax, f.rbx);
    // ... more registers ...

    if f.int_no == 14 {
        // Page fault — read CR2 for the faulting address
        let cr2: u64;
        unsafe { core::arch::asm!("mov {}, cr2", out(reg) cr2) };
        println!("CR2 (faulting address): {:#x}", cr2);
    }

    loop {}  // Halt on exception
}
```

## kaguyaOS Reference

| File | Lines | What it does |
|------|-------|-------------|
| `src/pic.rs` | 1-86 | PIC remapping, PIT config, EOI |
| `src/interrupts.rs` | 244-349 | IRQ/exception dispatch |
| `src/interrupts.rs` | 12-63 | ASM ISR/IRQ stubs |
| `src/interrupts.rs` | 84-108 | InterruptFrame struct |

---

**Next:** [Chapter 7 — Physical Memory & Paging](ch7-memory.md) — Managing RAM and setting up virtual memory.
