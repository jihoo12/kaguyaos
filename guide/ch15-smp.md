# Chapter 15: Symmetric Multi-Processing

Modern CPUs have multiple cores. This chapter covers how kaguyaOS brings up Application Processors (APs) using the standard Intel INIT-SIPI-SIPI sequence and gives each core its own GDT, IDT, and stack.

## Terminology

| Term | Meaning |
|------|---------|
| BSP | Bootstrap Processor — the CPU that boots the OS |
| AP | Application Processor — additional CPUs, started by the BSP |
| LAPIC | Local APIC — per-CPU interrupt controller |
| I/O APIC | Routes external IRQs to CPUs |
| SIPI | Startup IPI — Inter-Processor Interrupt to start an AP |

## Finding Other CPUs

The ACPI MADT (Multiple APIC Description Table) lists all CPUs:

```rust
// src/acpi.rs (simplified)
pub struct MadtInfo {
    pub cpu_count: usize,
    pub io_apic_address: u64,
    // CPU APIC IDs, etc.
}
```

```rust
// src/main.rs:160-165
if let Some(info) = unsafe { tables.madt_info() } {
    println!("Found {} CPUs, I/O APIC @ {:#x}",
        info.cpu_count, info.io_apic_address);
}
```

## The AP Bring-Up Sequence

APs start in a low-power state. To wake them up, the BSP sends a specific sequence of IPIs (Inter-Processor Interrupts) via the Local APIC:

```
1. INIT IPI         → Resets the AP
2. Delay (10ms)
3. SIPI IPI (vector 0x08) → AP starts executing at phys 0x8000
4. Delay (1ms)
5. SIPI IPI (vector 0x08) → Retry if first didn't work
6. Wait for AP to signal it's online
```

The vector in the SIPI IPI (0x08) tells the AP to start executing at physical address `0x08 * 0x1000 = 0x8000`.

## The Trampoline

The AP starts in **real mode** (16-bit, no paging). We need a "trampoline" — a small piece of code at physical address 0x8000 that transitions from real mode to 64-bit long mode:

```rust
// src/processor.rs:168
const TRAMPOLINE_PHYS: u64 = 0x8000;
```

The trampoline:
1. Loads a GDT with 32-bit code/data segments
2. Enables protected mode (PE bit in CR0)
3. Enables PAE and paging (CR4.PAE, CR0.PG)
4. Loads the BSP's PML4 into CR3
5. Enables long mode (EFER.LME)
6. Far-jumps to 64-bit code
7. Loads the 64-bit GDT (per-CPU), sets up stacks
8. Signals the BSP: "I'm online"

## Per-CPU Data

Each CPU needs its own:
- GDT and TSS (with its own RSP0)
- IDT (same content, but each CPU loads its own)
- Kernel stack
- GS base (for accessing per-CPU data via `swapgs`)

kaguyaOS stores per-CPU data in a static array indexed by CPU number:

```rust
// src/processor.rs
pub static mut PERCPU_DATA_SLOTS: [*mut PerCpuData; MAX_CPUS] = [core::ptr::null_mut(); MAX_CPUS];

pub struct PerCpuData {
    pub apic_id: u8,
    pub cpu_index: usize,
    // ... stack pointers, etc.
}
```

## Sending IPIs

The Local APIC's ICR (Interrupt Command Register) is used to send IPIs:

```rust
// src/processor.rs:138-161
fn icr_send(vector: u8, destination: u8, delivery: u32, shorthand: u32) {
    let icr_high = (destination as u64) << 56;
    let icr_low = (delivery | shorthand | vector as u32) as u64;

    icr_wait_idle();
    lapic_write(LAPIC_ICR_HIGH, icr_high);
    lapic_write(LAPIC_ICR_LOW, icr_low);
}

fn send_init_ipi(dest_apic_id: u8) {
    icr_send(0, dest_apic_id, ICR_DEST_FIXED | ICR_TRIGGER_EDGE, 0);
}

fn send_sipi(vector: u8, dest_apic_id: u8) {
    icr_send(vector, dest_apic_id, ICR_DEST_FIXED | ICR_TRIGGER_EDGE, 0);
}
```

## AP Entry Point

After the trampoline transitions to long mode, the AP runs `ap_entry()`:

```rust
// src/processor.rs:480-510 (simplified)
unsafe extern "sysv64" fn ap_entry() {
    // 1. Load per-CPU GDT
    gdt::init_cpu(my_cpu_index);

    // 2. Load IDT
    interrupts::init_idt();

    // 3. Set up per-CPU stack
    let stack_top = AP_STACKS[my_cpu_index].add(AP_STACK_SIZE);
    // ... set RSP ...

    // 4. Enable Local APIC
    processor::lapic_enable();

    // 5. Set up syscalls
    syscall::init_cpu();

    // 6. Signal BSP: "I'm online"
    AP_ONLINE_COUNT.fetch_add(1, Ordering::SeqCst);
    // Write to a flag page that the BSP is polling

    // 7. Park — wait for work
    loop {
        core::arch::asm!("hlt");
    }
}
```

## What APs Do

In kaguyaOS, APs have two roles:

1. **Network polling** — the e1000 NIC is polled by an AP in a busy loop, keeping ICMP reply packets flowing
2. **Scheduler standby** — APs can be parked in the scheduler loop for cooperative task execution

```rust
// BSP calls this to put APs to work
pub fn run_ap_scheduler() {
    // The AP enters the same switch_task() loop as the BSP
    loop {
        switch_task();
        core::arch::asm!("sti", "hlt");
    }
}
```

## kaguyaOS Reference

| File | Lines | What it does |
|------|-------|-------------|
| `src/processor.rs` | 1-615 | AP trampoline, INIT-SIPI-SIPI, per-CPU data |
| `src/acpi.rs` | ~880+ | MADT parsing (CPU list, I/O APIC) |
| `src/main.rs` | 324-347 | ACPI init + start_all_aps() |

---

**Next:** [Chapter 16 — Cooperative Scheduling](ch16-scheduling.md) — Task switching and multitasking.
