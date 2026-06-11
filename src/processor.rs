//! Application Processor (AP) initialization.
//!
//! This module brings up every non-boot CPU (AP) using the standard
//! Intel MP / ACPI INIT-SIPI-SIPI sequence:
//!
//! 1. The Bootstrap Processor (BSP) copies a 16-bit real-mode trampoline stub
//!    into a free page below 1 MiB (the "trampoline page").
//! 2. The BSP sends an INIT IPI to each AP's Local APIC ID, followed by two
//!    STARTUP IPIs that point to the trampoline page.
//! 3. Each AP wakes in real mode at the trampoline, switches to 64-bit long
//!    mode, and jumps to `ap_entry` in Rust.
//! 4. `ap_entry` loads the shared GDT/IDT, sets up its per-CPU stack, enables
//!    interrupts, and parks in a `hlt` loop (ready to be scheduled).
//!
//! # Trampoline memory layout (one 4 KiB page, physical < 1 MiB)
//!
//! ```text
//! [+0x000] 16-bit real-mode code
//! [+0xF00] 64-bit target RIP   (u64, written by BSP before SIPI)
//! [+0xF08] 64-bit target CR3   (u64, written by BSP before SIPI)
//! [+0xF10] 64-bit stack top    (u64, written by BSP before SIPI)
//! [+0xF18] u32 AP online flag  (written by AP on entry)
//! ```

use core::sync::atomic::{AtomicU32, Ordering};

use crate::acpi::MadtInfo;
use crate::io::{io_wait, outb};

// ─── Per-CPU stacks ──────────────────────────────────────────────────────────

/// Maximum number of APs we support (BSP + 63 APs = 64 logical CPUs total).
pub const MAX_AP_COUNT: usize = 63;

/// Stack size for each AP kernel stack (16 KiB, 16-byte aligned).
pub const AP_STACK_SIZE: usize = 16 * 1024;

#[repr(align(16))]
struct ApStack([u8; AP_STACK_SIZE]);

/// Static backing storage for all AP kernel stacks.
static mut AP_STACKS: [ApStack; MAX_AP_COUNT] = [const { ApStack([0; AP_STACK_SIZE]) }; MAX_AP_COUNT];

// ─── Online CPU counter (atomic, updated by each AP on entry) ────────────────

/// Number of APs that have fully come online (not counting the BSP).
static AP_ONLINE_COUNT: AtomicU32 = AtomicU32::new(0);

/// Returns the number of APs that have finished their own `ap_entry`.
pub fn online_ap_count() -> u32 {
    AP_ONLINE_COUNT.load(Ordering::Acquire)
}

// ─── Local APIC MMIO access ──────────────────────────────────────────────────

/// Write a 32-bit value to a Local APIC register at `base + offset`.
///
/// # Safety
/// `base` must be a valid, mapped LAPIC MMIO base address.
#[inline]
pub unsafe fn lapic_write(base: u64, offset: u32, value: u32) {
    unsafe {
        core::ptr::write_volatile((base + offset as u64) as *mut u32, value);
    }
}

/// Read a 32-bit value from a Local APIC register at `base + offset`.
///
/// # Safety
/// `base` must be a valid, mapped LAPIC MMIO base address.
#[inline]
pub unsafe fn lapic_read(base: u64, offset: u32) -> u32 {
    unsafe { core::ptr::read_volatile((base + offset as u64) as *const u32) }
}

// Standard xAPIC register offsets (in bytes from LAPIC base).
pub const LAPIC_ID:             u32 = 0x020; // Local APIC ID
pub const LAPIC_VERSION:        u32 = 0x030; // Local APIC Version
pub const LAPIC_TPR:            u32 = 0x080; // Task Priority Register
pub const LAPIC_EOI:            u32 = 0x0B0; // End Of Interrupt
pub const LAPIC_SVR:            u32 = 0x0F0; // Spurious Interrupt Vector Register
pub const LAPIC_ICR_LOW:        u32 = 0x300; // Interrupt Command Register [31:0]
pub const LAPIC_ICR_HIGH:       u32 = 0x310; // Interrupt Command Register [63:32]
pub const LAPIC_LVT_TIMER:      u32 = 0x320; // LVT Timer Register
pub const LAPIC_TIMER_INIT:     u32 = 0x380; // Initial Count (timer)
pub const LAPIC_TIMER_CUR:      u32 = 0x390; // Current Count (timer)
pub const LAPIC_TIMER_DIV:      u32 = 0x3E0; // Divide Configuration

// ICR delivery modes (bits [10:8] of ICR_LOW).
pub const ICR_DELIVERY_FIXED:   u32 = 0 << 8;
pub const ICR_DELIVERY_INIT:    u32 = 5 << 8;
pub const ICR_DELIVERY_STARTUP: u32 = 6 << 8;

// ICR level / trigger bits.
pub const ICR_LEVEL_ASSERT:     u32 = 1 << 14;
pub const ICR_LEVEL_DEASSERT:   u32 = 0 << 14;
pub const ICR_TRIGGER_EDGE:     u32 = 0 << 15;
pub const ICR_TRIGGER_LEVEL:    u32 = 1 << 15;

// ICR destination shorthand (bits [19:18]).
pub const ICR_DEST_NONE:        u32 = 0 << 18; // use explicit APIC ID in ICR_HIGH
pub const ICR_DEST_SELF:        u32 = 1 << 18;
pub const ICR_DEST_ALL_INCL:    u32 = 2 << 18;
pub const ICR_DEST_ALL_EXCL:    u32 = 3 << 18;

// SVR: software-enable APIC, set spurious vector.
pub const LAPIC_SVR_ENABLE:     u32 = 1 << 8;
pub const LAPIC_SPURIOUS_VEC:   u32 = 0xFF;

/// Enable the Local APIC on the current CPU (BSP or AP).
///
/// Sets the Software-Enable bit in the Spurious Interrupt Vector Register and
/// maps the spurious interrupt to vector 0xFF.
///
/// # Safety
/// `lapic_base` must be a valid identity-mapped LAPIC MMIO address.
pub unsafe fn lapic_enable(lapic_base: u64) {
    unsafe {
        lapic_write(lapic_base, LAPIC_SVR, LAPIC_SVR_ENABLE | LAPIC_SPURIOUS_VEC);
        // Set Task Priority to 0 — accept all interrupts.
        lapic_write(lapic_base, LAPIC_TPR, 0);
    }
}

/// Send End-of-Interrupt to the current CPU's Local APIC.
///
/// # Safety
/// `lapic_base` must be valid.
#[inline]
pub unsafe fn lapic_eoi(lapic_base: u64) {
    unsafe { lapic_write(lapic_base, LAPIC_EOI, 0) }
}

/// Poll the ICR Delivery Status bit until it clears (send complete).
///
/// # Safety
/// `lapic_base` must be valid.
unsafe fn icr_wait_idle(lapic_base: u64) {
    loop {
        let low = unsafe { lapic_read(lapic_base, LAPIC_ICR_LOW) };
        if (low & (1 << 12)) == 0 {
            // Delivery Status = Idle.
            break;
        }
        core::hint::spin_loop();
    }
}

/// Write to the 64-bit ICR atomically: high word first, then low word
/// (the low write triggers the IPI).
///
/// # Safety
/// `lapic_base` must be valid.
unsafe fn icr_send(lapic_base: u64, dest_apic_id: u8, icr_low: u32) {
    let icr_high = (dest_apic_id as u32) << 24;
    unsafe {
        icr_wait_idle(lapic_base);
        lapic_write(lapic_base, LAPIC_ICR_HIGH, icr_high);
        lapic_write(lapic_base, LAPIC_ICR_LOW, icr_low);
    }
}

// ─── Trampoline ──────────────────────────────────────────────────────────────

/// Physical address of the trampoline page (must be < 1 MiB and page-aligned).
/// We use 0x8000 (the page at vector 8 — SIPI vectors are 4 KiB pages, so
/// vector 8 → physical 0x8000).
pub const TRAMPOLINE_PHYS: u64 = 0x8000;
/// SIPI vector = physical_address >> 12.
const SIPI_VECTOR: u32 = (TRAMPOLINE_PHYS >> 12) as u32;

/// Offset within the trampoline page where BSP writes handshake data.
const TRAMP_OFF_RIP:    usize = 0xF00;
const TRAMP_OFF_CR3:    usize = 0xF08;
const TRAMP_OFF_RSP:    usize = 0xF10;
const TRAMP_OFF_ONLINE: usize = 0xF18;

/// 16-bit real-mode trampoline stub.
///
/// This blob is position-independent and must be copied to `TRAMPOLINE_PHYS`.
/// It switches the AP through:
///   real mode → protected mode (minimal 32-bit GDT) → long mode (64-bit)
/// then reads the target RIP, CR3, and RSP from known offsets within its own
/// page and jumps to `ap_entry`.
///
/// Assembled from:
/// ```asm
/// BITS 16
/// ORG 0x8000           ; matches TRAMPOLINE_PHYS
///
/// cli
/// cld
///
/// ; ── load a minimal 16-bit-compatible GDT descriptor ──────────────────
/// lgdt [cs:gdt16_ptr - 0x8000 + 0]   ; GDT embedded below
///
/// ; ── switch to protected mode ─────────────────────────────────────────
/// mov eax, cr0
/// or  eax, 1
/// mov cr0, eax
/// jmp 0x08:pm32_entry              ; far jump to flush pipeline
///
/// BITS 32
/// pm32_entry:
///     mov ax, 0x10
///     mov ds, ax
///     mov es, ax
///     mov ss, ax
///
///     ; ── enable PAE ────────────────────────────────────────────────────
///     mov eax, cr4
///     or  eax, (1<<5)          ; PAE
///     mov cr4, eax
///
///     ; ── load PML4 from trampoline handshake area ──────────────────────
///     mov eax, [0x8F08]        ; TRAMPOLINE_PHYS + TRAMP_OFF_CR3 (low 32 bits)
///     mov cr3, eax
///
///     ; ── enable long mode via IA32_EFER ────────────────────────────────
///     mov ecx, 0xC0000080
///     rdmsr
///     or  eax, (1<<8)          ; LME
///     wrmsr
///
///     ; ── enable paging (activates long mode) ───────────────────────────
///     mov eax, cr0
///     or  eax, (1<<31) | 1
///     mov cr0, eax
///
///     ; ── far jump into 64-bit code segment ─────────────────────────────
///     jmp 0x08:lm64_entry
///
/// BITS 64
/// lm64_entry:
///     mov ax, 0x10
///     mov ds, ax
///     mov es, ax
///     mov ss, ax
///     xor ax, ax
///     mov fs, ax
///     mov gs, ax
///
///     ; ── load RSP from handshake ────────────────────────────────────────
///     mov rsp, [0x8F10]
///
///     ; ── jump to Rust ap_entry ─────────────────────────────────────────
///     mov rax, [0x8F00]        ; RIP
///     jmp rax
/// ```
///
/// The blob below is the hand-assembled output for the code above.
static TRAMPOLINE_CODE: &[u8] = &[
    // ── 16-bit real mode (offset 0..32) ──
    0xFA,                           // 0x00: cli
    0xFC,                           // 0x01: cld
    0x31, 0xC0,                     // 0x02: xor ax, ax
    0x8E, 0xD8,                     // 0x04: mov ds, ax
    0x0F, 0x01, 0x16, 0x00, 0x81,   // 0x06: lgdt [0x8100] (physical 0x8100)
    0x0F, 0x20, 0xC0,               // 0x0B: mov eax, cr0
    0x66, 0x83, 0xC8, 0x01,         // 0x0E: or eax, 1
    0x0F, 0x22, 0xC0,               // 0x12: mov cr0, eax
    0x66, 0xEA,                     // 0x15: jmp far 0x08:0x8020 (selector 0x08 -> 32-bit Code segment)
    0x20, 0x80, 0x00, 0x00,         // offset (0x00008020)
    0x08, 0x00,                     // selector 0x08
    0x90, 0x90, 0x90,               // 0x1D: pad to 32 bytes (offset 0x20)

    // ── 32-bit protected mode (offset 32..128) ──
    0x66, 0xB8, 0x10, 0x00,         // 0x20: mov ax, 0x10 (selector 0x10 -> 32-bit Data segment)
    0x8E, 0xD8,                     // 0x24: mov ds, ax
    0x8E, 0xC0,                     // 0x26: mov es, ax
    0x8E, 0xD0,                     // 0x28: mov ss, ax
    0x0F, 0x20, 0xE0,               // 0x2A: mov eax, cr4
    0x83, 0xC8, 0x20,               // 0x2D: or eax, 0x20 (PAE)
    0x0F, 0x22, 0xE0,               // 0x30: mov cr4, eax
    0xA1, 0x08, 0x8F, 0x00, 0x00,   // 0x33: mov eax, [0x8F08] (CR3)
    0x0F, 0x22, 0xD8,               // 0x38: mov cr3, eax
    0xB9, 0x80, 0x00, 0x00, 0xC0,   // 0x3B: mov ecx, 0xC0000080 (EFER MSR)
    0x0F, 0x32,                     // 0x40: rdmsr
    0x0D, 0x00, 0x01, 0x00, 0x00,   // 0x42: or eax, 0x100 (LME)
    0x0F, 0x30,                     // 0x47: wrmsr
    0x0F, 0x20, 0xC0,               // 0x49: mov eax, cr0
    0x0D, 0x00, 0x00, 0x00, 0x80,   // 0x4C: or eax, 0x80000000 (PG)
    0x0F, 0x22, 0xC0,               // 0x51: mov cr0, eax
    0xEA, 0x80, 0x80, 0x00, 0x00,   // 0x54: jmp far 0x18:0x8080 (lm64_entry, selector 0x18 -> 64-bit Code segment)
    0x18, 0x00,
    // 0x5B: 37 bytes pad to 128 bytes (offset 0x80)
    0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90,
    0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90,
    0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90,
    0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90,

    // ── 64-bit long mode transition stub (offset 128..256) ──
    0x66, 0xB8, 0x10, 0x00,         // 0x80: mov ax, 0x10
    0x66, 0x8E, 0xD8,               // 0x84: mov ds, ax
    0x66, 0x8E, 0xC0,               // 0x87: mov es, ax
    0x66, 0x8E, 0xD0,               // 0x8A: mov ss, ax
    0x66, 0x31, 0xC0,               // 0x8D: xor ax, ax
    0x66, 0x8E, 0xE0,               // 0x90: mov fs, ax
    0x66, 0x8E, 0xE8,               // 0x93: mov gs, ax
    0x48, 0x8B, 0x24, 0x25, 0x10, 0x8F, 0x00, 0x00, // 0x96: mov rsp, [0x8F10]
    0x48, 0x8B, 0x04, 0x25, 0x00, 0x8F, 0x00, 0x00, // 0x9E: mov rax, [0x8F00]
    0xFF, 0xE0,                     // 0xA6: jmp rax
    // 0xA8: 88 bytes pad to 256 bytes (offset 0x100)
    0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90,
    0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90,
    0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90,
    0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90,
    0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90,
    0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90,
    0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90,
    0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90,
    0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90,

    // ── GDT descriptor at offset 256 (0x100) ──
    0x1F, 0x00,                     // limit = 31 (4 entries)
    0x10, 0x81, 0x00, 0x00,         // base = 0x00008110 (offset 0x110)
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // 10 bytes pad to 0x110

    // ── GDT entries at offset 272 (0x110) ──
    // [0] Null
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    // [1] 32-bit Code (selector 0x08): base=0, limit=0xFFFFF, access=0x9A, gran=0xCF
    0xFF, 0xFF, 0x00, 0x00, 0x00, 0x9A, 0xCF, 0x00,
    // [2] 32-bit Data (selector 0x10): base=0, limit=0xFFFFF, access=0x92, gran=0xCF
    0xFF, 0xFF, 0x00, 0x00, 0x00, 0x92, 0xCF, 0x00,
    // [3] 64-bit Code (selector 0x18): base=0, limit=0xFFFFF, access=0x9A, gran=0xAF
    0xFF, 0xFF, 0x00, 0x00, 0x00, 0x9A, 0xAF, 0x00,
];

/// Write the 64-bit handshake values into the trampoline page before kicking
/// an AP.
///
/// # Safety
/// The trampoline page at `TRAMPOLINE_PHYS` must be identity-mapped and
/// writable.
unsafe fn write_trampoline_params(target_rip: u64, target_cr3: u64, stack_top: u64) {
    let base = TRAMPOLINE_PHYS as usize;
    unsafe {
        // Zero the online flag.
        core::ptr::write_volatile((base + TRAMP_OFF_ONLINE) as *mut u32, 0);
        // Write handshake data.
        core::ptr::write_volatile((base + TRAMP_OFF_RIP) as *mut u64, target_rip);
        core::ptr::write_volatile((base + TRAMP_OFF_CR3) as *mut u64, target_cr3);
        core::ptr::write_volatile((base + TRAMP_OFF_RSP) as *mut u64, stack_top);
    }
}

/// Copy the trampoline stub into the low-memory page at `TRAMPOLINE_PHYS`.
///
/// # Safety
/// `TRAMPOLINE_PHYS` must be identity-mapped and writable.
unsafe fn install_trampoline() {
    let dst = TRAMPOLINE_PHYS as *mut u8;
    let src = TRAMPOLINE_CODE.as_ptr();
    let len = TRAMPOLINE_CODE.len();
    unsafe {
        core::ptr::copy_nonoverlapping(src, dst, len);
    }
}

// ─── INIT-SIPI-SIPI sequence ─────────────────────────────────────────────────

/// Send the standard INIT-SIPI-SIPI sequence to a single AP identified by its
/// Local APIC ID.
///
/// After the sequence, we wait up to ~200 ms for the AP to set its online flag
/// in the trampoline page.
///
/// # Safety
/// `lapic_base` must be the identity-mapped LAPIC MMIO address for the BSP.
/// The trampoline page must have been installed and its params written.
unsafe fn start_ap(lapic_base: u64, dest_id: u8) {
    let online_flag_ptr = (TRAMPOLINE_PHYS as usize + TRAMP_OFF_ONLINE) as *const u32;

    // Clear the online flag before sending the IPI.
    unsafe {
        core::ptr::write_volatile(
            (TRAMPOLINE_PHYS as usize + TRAMP_OFF_ONLINE) as *mut u32,
            0,
        );
    }

    // ── INIT IPI (assert) ─────────────────────────────────────────────────
    unsafe {
        icr_send(
            lapic_base,
            dest_id,
            ICR_DELIVERY_INIT | ICR_LEVEL_ASSERT | ICR_TRIGGER_LEVEL,
        );
    }

    // Wait ~10 ms using legacy I/O port delay (rough but portable).
    for _ in 0..10_000 {
        unsafe { io_wait() };
    }

    // ── INIT IPI (deassert) ───────────────────────────────────────────────
    unsafe {
        icr_send(
            lapic_base,
            dest_id,
            ICR_DELIVERY_INIT | ICR_LEVEL_DEASSERT | ICR_TRIGGER_LEVEL,
        );
    }

    for _ in 0..10_000 {
        unsafe { io_wait() };
    }

    // ── SIPI × 2 ─────────────────────────────────────────────────────────
    for _ in 0..2u8 {
        unsafe {
            icr_send(
                lapic_base,
                dest_id,
                ICR_DELIVERY_STARTUP | ICR_LEVEL_ASSERT | SIPI_VECTOR,
            );
        }
        // ~200 µs between SIPIs (spec says ≥ 200 µs).
        for _ in 0..200 {
            unsafe { io_wait() };
        }
    }

    // ── Wait for AP to come online (timeout ~ 1 s) ────────────────────────
    let mut timeout = 1_000_000u32;
    loop {
        let flag = unsafe { core::ptr::read_volatile(online_flag_ptr) };
        if flag != 0 {
            break;
        }
        timeout = match timeout.checked_sub(1) {
            Some(v) => v,
            None => {
                crate::println!(
                    "[SMP] AP APIC ID={} timed out waiting for online flag",
                    dest_id
                );
                return;
            }
        };
        core::hint::spin_loop();
    }

    crate::println!("[SMP] AP APIC ID={} came online", dest_id);
}

// ─── AP entry point (called from trampoline in 64-bit mode) ──────────────────

/// Entry point for every AP.
///
/// At this point the AP is in 64-bit long mode, running on the stack provided
/// by the BSP via the trampoline handshake area. The AP:
///
/// 1. Loads the BSP's shared GDT and IDT (so exceptions are handled).
/// 2. Enables its own Local APIC.
/// 3. Sets the online flag in the trampoline page.
/// 4. Increments `AP_ONLINE_COUNT`.
/// 5. Enables interrupts and parks in `hlt`.
///
/// # Safety
/// Called from trampoline assembly — ABI is effectively "callee sets up
/// everything from scratch". Do NOT declare as `extern "C"` and expect a
/// proper frame.
#[unsafe(no_mangle)]
pub unsafe extern "sysv64" fn ap_entry() {
    unsafe {
        // 1. Load shared GDT & IDT (already initialised by BSP).
        crate::gdt::init();
        crate::interrupts::init_idt();

        // 2. Enable this AP's Local APIC.
        let lapic_base = lapic_base_from_msr();
        lapic_enable(lapic_base);

        // 3. Signal online.
        core::ptr::write_volatile(
            (TRAMPOLINE_PHYS as usize + TRAMP_OFF_ONLINE) as *mut u32,
            1,
        );

        // 4. Count this AP.
        AP_ONLINE_COUNT.fetch_add(1, Ordering::Release);

        // 5. Enable interrupts and park.
        core::arch::asm!("sti");
        loop {
            core::arch::asm!("hlt", options(nomem, nostack, preserves_flags));
        }
    }
}

// ─── BSP-side: bring up all APs ──────────────────────────────────────────────

/// Bring up all Application Processors described by `madt`.
///
/// Must be called on the BSP after:
/// * Paging is active (identity-mapped low 1 MiB)
/// * GDT and IDT are initialised
/// * Interrupts are enabled on the BSP
///
/// # Arguments
/// * `madt`     – Parsed MADT information from `acpi::parse_madt`.
/// * `bsp_apic_id` – The Local APIC ID of the Bootstrap Processor.
///
/// # Safety
/// * The LAPIC MMIO region at `madt.local_apic_address` must be mapped.
/// * Physical page `TRAMPOLINE_PHYS` (0x8000) must be identity-mapped and writable.
pub unsafe fn start_all_aps(madt: &MadtInfo, bsp_apic_id: u8) {
    let lapic_base = madt.local_apic_address;

    crate::println!("[SMP] BSP APIC ID={}, LAPIC base={:#x}", bsp_apic_id, lapic_base);
    crate::println!("[SMP] {} CPU(s) detected", madt.cpu_count);

    if madt.cpu_count <= 1 {
        crate::println!("[SMP] Single-CPU system, skipping AP bring-up");
        return;
    }

    // Enable the BSP's own LAPIC (may already be enabled by firmware).
    unsafe { lapic_enable(lapic_base) };

    // Install trampoline code into the low-memory page.
    unsafe { install_trampoline() };

    // Read the current CR3 so APs share the BSP's page tables.
    let cr3: u64;
    unsafe {
        core::arch::asm!("mov {}, cr3", out(reg) cr3, options(nomem, nostack, preserves_flags));
    }

    let mut ap_index: usize = 0;

    for i in 0..(madt.cpu_count as usize) {
        let apic_id = madt.apic_ids[i];

        // Skip the BSP.
        if apic_id == bsp_apic_id {
            continue;
        }

        if ap_index >= MAX_AP_COUNT {
            crate::println!("[SMP] Too many APs, skipping APIC ID={}", apic_id);
            continue;
        }

        // Assign a unique stack for this AP.
        let stack_top = unsafe {
            let stack = &raw const AP_STACKS[ap_index] as *const ApStack;
            (stack as u64) + AP_STACK_SIZE as u64
        };

        crate::println!(
            "[SMP] Starting AP {} (APIC ID={}, stack top={:#x})",
            ap_index, apic_id, stack_top
        );

        // Write handshake params for this AP.
        unsafe {
            write_trampoline_params(ap_entry as u64, cr3, stack_top);
        }

        // Fire INIT-SIPI-SIPI.
        unsafe { start_ap(lapic_base, apic_id) };

        ap_index += 1;
    }

    crate::println!(
        "[SMP] All APs started. Online AP count: {}",
        AP_ONLINE_COUNT.load(Ordering::Acquire)
    );
}

// ─── Local APIC ID of the current CPU ────────────────────────────────────────

/// Read the Local APIC ID of the currently executing CPU from CPUID leaf 1.
///
/// This works on both BSP and APs (before and after LAPIC mapping).
pub fn current_apic_id() -> u8 {
    // rbx/ebx is reserved by LLVM for internal use and cannot be named as
    // an asm operand. We work around this by saving rbx ourselves and
    // reading the APIC ID out of rbx into a scratch register before restoring.
    let apic_id_bits: u32;
    unsafe {
        core::arch::asm!(
            "push rbx",
            "cpuid",
            "mov {0:e}, ebx",    // copy ebx -> output operand while rbx is live
            "pop rbx",
            out(reg) apic_id_bits,
            in("eax") 1u32,
            out("ecx") _,
            out("edx") _,
            lateout("eax") _,
            options(nomem),       // cannot use nostack because we push/pop
        );
    }
    // APIC ID is in bits [31:24] of EBX from CPUID.1
    ((apic_id_bits >> 24) & 0xFF) as u8
}

// ─── LAPIC timer (basic, for per-CPU periodic tick) ──────────────────────────

/// Configure the Local APIC one-shot timer on the current CPU.
///
/// The APIC timer counts down `initial_count` ticks at the bus clock divided
/// by `divisor` and fires vector `vector` when it reaches zero.
///
/// Call this from the AP (or BSP) after `lapic_enable`.
///
/// # Arguments
/// * `lapic_base`    – Identity-mapped LAPIC MMIO base.
/// * `vector`        – IDT vector to fire (e.g. 0x30 = IRQ 16).
/// * `initial_count` – Initial counter value.
/// * `divisor_cfg`   – Divide Configuration Register value (0 = /2, …, 0xB = /1).
/// * `periodic`      – `true` for periodic mode, `false` for one-shot.
///
/// # Safety
/// `lapic_base` must be a valid LAPIC MMIO address.
pub unsafe fn lapic_timer_start(
    lapic_base: u64,
    vector: u8,
    initial_count: u32,
    divisor_cfg: u32,
    periodic: bool,
) {
    const LVT_TIMER_PERIODIC: u32 = 1 << 17;

    let mode = if periodic { LVT_TIMER_PERIODIC } else { 0 };
    unsafe {
        lapic_write(lapic_base, LAPIC_TIMER_DIV, divisor_cfg);
        lapic_write(lapic_base, LAPIC_LVT_TIMER, vector as u32 | mode);
        lapic_write(lapic_base, LAPIC_TIMER_INIT, initial_count);
    }
}

/// Stop (mask) the Local APIC timer on the current CPU.
///
/// # Safety
/// `lapic_base` must be valid.
pub unsafe fn lapic_timer_stop(lapic_base: u64) {
    const LVT_MASKED: u32 = 1 << 16;
    unsafe {
        lapic_write(lapic_base, LAPIC_LVT_TIMER, LVT_MASKED);
        lapic_write(lapic_base, LAPIC_TIMER_INIT, 0);
    }
}

/// Read the current remaining count of the Local APIC timer.
///
/// # Safety
/// `lapic_base` must be valid.
pub unsafe fn lapic_timer_current(lapic_base: u64) -> u32 {
    unsafe { lapic_read(lapic_base, LAPIC_TIMER_CUR) }
}

// ─── MSR helpers ─────────────────────────────────────────────────────────────

/// Read a Model Specific Register.
///
/// # Safety
/// The MSR must exist on this CPU; reading a non-existent MSR triggers #GP.
#[inline]
pub unsafe fn rdmsr(msr: u32) -> u64 {
    let (lo, hi): (u32, u32);
    unsafe {
        core::arch::asm!(
            "rdmsr",
            in("ecx") msr,
            out("eax") lo,
            out("edx") hi,
            options(nomem, nostack, preserves_flags),
        );
    }
    ((hi as u64) << 32) | (lo as u64)
}

/// Write a Model Specific Register.
///
/// # Safety
/// The MSR must exist and be writable. Writing a reserved bit triggers #GP.
#[inline]
pub unsafe fn wrmsr(msr: u32, value: u64) {
    let lo = value as u32;
    let hi = (value >> 32) as u32;
    unsafe {
        core::arch::asm!(
            "wrmsr",
            in("ecx") msr,
            in("eax") lo,
            in("edx") hi,
            options(nomem, nostack, preserves_flags),
        );
    }
}

/// IA32_EFER MSR number.
pub const MSR_IA32_EFER:        u32 = 0xC000_0080;
/// IA32_APIC_BASE MSR number.
pub const MSR_IA32_APIC_BASE:   u32 = 0x0000_001B;
/// IA32_GS_BASE MSR number (used for per-CPU data).
pub const MSR_IA32_GS_BASE:     u32 = 0xC000_0101;
/// IA32_KERNEL_GS_BASE MSR number.
pub const MSR_IA32_KERNEL_GS_BASE: u32 = 0xC000_0102;

/// Read the LAPIC base address from the `IA32_APIC_BASE` MSR.
///
/// Bits [51:12] of the MSR hold the physical base (page-aligned).
///
/// # Safety
/// CPUID must confirm xAPIC presence.
pub unsafe fn lapic_base_from_msr() -> u64 {
    unsafe { rdmsr(MSR_IA32_APIC_BASE) & 0x000F_FFFF_FFFF_F000 }
}

// ─── Per-CPU GS-base (simple per-CPU data pointer) ───────────────────────────

/// Per-CPU data stored at GS:0.
#[repr(C)]
pub struct PercpuData {
    /// Physical APIC ID of this CPU.
    pub apic_id: u8,
    /// Logical 0-based CPU index.
    pub cpu_index: u8,
    /// Kernel stack top for this CPU (used to restore RSP0 in TSS on reschedule).
    pub kernel_stack_top: u64,
}

/// Set the GS base for the current CPU to `ptr`, making per-CPU data
/// accessible via `GS:0`.
///
/// # Safety
/// `ptr` must remain valid for the lifetime of this CPU.
pub unsafe fn set_percpu_data(ptr: *mut PercpuData) {
    unsafe { wrmsr(MSR_IA32_GS_BASE, ptr as u64) };
}

/// Read the GS base (i.e. the per-CPU data pointer) for the current CPU.
///
/// # Safety
/// GS base must have been set via `set_percpu_data`.
pub unsafe fn get_percpu_data() -> *mut PercpuData {
    unsafe { rdmsr(MSR_IA32_GS_BASE) as *mut PercpuData }
}
