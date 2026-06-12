#![allow(bad_asm_style)]

use crate::writer::GLOBAL_WRITER;
use core::arch::asm;
use core::fmt::Write;
use core::mem::size_of;

pub const KERNEL_CODE_SEL: u16 = 0x08;

use core::sync::atomic::{AtomicBool, Ordering};
use core::cell::UnsafeCell;
use core::ops::{Deref, DerefMut};

pub struct InterruptSpinlock<T> {
    lock: AtomicBool,
    data: UnsafeCell<T>,
}

unsafe impl<T: Send> Sync for InterruptSpinlock<T> {}
unsafe impl<T: Send> Send for InterruptSpinlock<T> {}

impl<T> InterruptSpinlock<T> {
    pub const fn new(data: T) -> Self {
        Self {
            lock: AtomicBool::new(false),
            data: UnsafeCell::new(data),
        }
    }

    pub fn lock(&self) -> InterruptSpinlockGuard<T> {
        let rflags = unsafe {
            let r: u64;
            core::arch::asm!("pushfq; pop {}", out(reg) r, options(nomem, preserves_flags));
            r
        };
        unsafe {
            core::arch::asm!("cli", options(nomem, nostack, preserves_flags));
        }
        let interrupts_enabled = (rflags & (1 << 9)) != 0;

        while self
            .lock
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            core::hint::spin_loop();
        }

        InterruptSpinlockGuard {
            lock: self,
            interrupts_enabled,
        }
    }

    pub fn try_lock(&self) -> Option<InterruptSpinlockGuard<T>> {
        let rflags = unsafe {
            let r: u64;
            core::arch::asm!("pushfq; pop {}", out(reg) r, options(nomem, preserves_flags));
            r
        };
        unsafe {
            core::arch::asm!("cli", options(nomem, nostack, preserves_flags));
        }
        let interrupts_enabled = (rflags & (1 << 9)) != 0;

        if self
            .lock
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
        {
            Some(InterruptSpinlockGuard {
                lock: self,
                interrupts_enabled,
            })
        } else {
            if interrupts_enabled {
                unsafe {
                    core::arch::asm!("sti", options(nomem, nostack, preserves_flags));
                }
            }
            None
        }
    }

    pub unsafe fn force_unlock(&self) {
        self.lock.store(false, Ordering::Release);
    }
}

pub struct InterruptSpinlockGuard<'a, T> {
    lock: &'a InterruptSpinlock<T>,
    interrupts_enabled: bool,
}

impl<'a, T> Deref for InterruptSpinlockGuard<'a, T> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &*self.lock.data.get() }
    }
}

impl<'a, T> DerefMut for InterruptSpinlockGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.lock.data.get() }
    }
}

impl<'a, T> Drop for InterruptSpinlockGuard<'a, T> {
    fn drop(&mut self) {
        self.lock.lock.store(false, Ordering::Release);
        if self.interrupts_enabled {
            unsafe {
                core::arch::asm!("sti", options(nomem, nostack, preserves_flags));
            }
        }
    }
}


#[allow(dead_code)]
unsafe extern "C" {
    fn isr0();
    fn isr1();
    fn isr2();
    fn isr3();
    fn isr4();
    fn isr5();
    fn isr6();
    fn isr7();
    fn isr8();
    fn isr9();
    fn isr10();
    fn isr11();
    fn isr12();
    fn isr13();
    fn isr14();
    fn isr15();
    fn isr16();
    fn isr17();
    fn isr18();
    fn isr19();
    fn isr20();
    fn isr21();
    fn isr22();
    fn isr23();
    fn isr24();
    fn isr25();
    fn isr26();
    fn isr27();
    fn isr28();
    fn isr29();
    fn isr30();
    fn isr31();

    // IRQ Handlers
    fn irq0();
    fn irq1();
    fn irq2();
    fn irq3();
    fn irq4();
    fn irq5();
    fn irq6();
    fn irq7();
    fn irq8();
    fn irq9();
    fn irq10();
    fn irq11();
    fn irq12();
    fn irq13();
    fn irq14();
    fn irq15();
}

#[derive(Copy, Clone, Default)]
#[repr(C, packed)]
pub struct IdtEntry {
    offset_low: u16,
    selector: u16,
    ist: u8,
    type_attr: u8,
    offset_mid: u16,
    offset_high: u32,
    zero: u32,
}

#[derive(Copy, Clone)]
#[repr(C, packed)]
pub struct IdtPointer {
    limit: u16,
    base: u64,
}

#[repr(C, packed)]
pub struct InterruptFrame {
    pub r15: u64,      // RSP+0   (last pushed)
    pub r14: u64,      // RSP+8
    pub r13: u64,      // RSP+16
    pub r12: u64,      // RSP+24
    pub r11: u64,      // RSP+32
    pub r10: u64,      // RSP+40
    pub r9: u64,       // RSP+48
    pub r8: u64,       // RSP+56
    pub rsi: u64,      // RSP+64
    pub rdi: u64,      // RSP+72
    pub rbp: u64,      // RSP+80
    pub rdx: u64,      // RSP+88
    pub rcx: u64,      // RSP+96
    pub rbx: u64,      // RSP+104
    pub rax: u64,      // RSP+112
    pub int_no: u64,   // RSP+120  <-- should be $33
    pub err_code: u64, // RSP+128  <-- should be $0
    pub rip: u64,      // RSP+136  (CPU pushed)
    pub cs: u64,       // RSP+144
    pub rflags: u64,   // RSP+152
    pub rsp: u64,      // RSP+160
    pub ss: u64,       // RSP+168
}

static mut IDT: [IdtEntry; 256] = [IdtEntry {
    offset_low: 0,
    selector: 0,
    ist: 0,
    type_attr: 0,
    offset_mid: 0,
    offset_high: 0,
    zero: 0,
}; 256];

static mut IDT_PTR: IdtPointer = IdtPointer { limit: 0, base: 0 };

pub unsafe fn set_gate(
    vector: usize,
    handler: unsafe extern "C" fn(),
    selector: u16,
    type_attr: u8,
) {
    let addr = handler as u64;
    unsafe {
        IDT[vector].offset_low = (addr & 0xFFFF) as u16;
        IDT[vector].selector = selector;
        IDT[vector].ist = 0;
        IDT[vector].type_attr = type_attr;
        IDT[vector].offset_mid = ((addr >> 16) & 0xFFFF) as u16;
        IDT[vector].offset_high = ((addr >> 32) & 0xFFFFFFFF) as u32;
        IDT[vector].zero = 0;
    }
}

pub unsafe fn init_idt() {
    unsafe {
        // Initialize with default/generic handlers if needed, but here we set specific exceptions

        set_gate(0, isr0, KERNEL_CODE_SEL, 0x8E);
        set_gate(1, isr1, KERNEL_CODE_SEL, 0x8E);
        set_gate(2, isr2, KERNEL_CODE_SEL, 0x8E);
        set_gate(3, isr3, KERNEL_CODE_SEL, 0x8E);
        set_gate(4, isr4, KERNEL_CODE_SEL, 0x8E);
        set_gate(5, isr5, KERNEL_CODE_SEL, 0x8E);
        set_gate(6, isr6, KERNEL_CODE_SEL, 0x8E);
        set_gate(7, isr7, KERNEL_CODE_SEL, 0x8E);
        set_gate(8, isr8, KERNEL_CODE_SEL, 0x8E);

        // Set IST Stack for Double Fault
        IDT[8].ist = crate::gdt::DOUBLE_FAULT_IST_INDEX as u8;
        set_gate(9, isr9, KERNEL_CODE_SEL, 0x8E);
        set_gate(10, isr10, KERNEL_CODE_SEL, 0x8E);
        set_gate(11, isr11, KERNEL_CODE_SEL, 0x8E);
        set_gate(12, isr12, KERNEL_CODE_SEL, 0x8E);
        set_gate(13, isr13, KERNEL_CODE_SEL, 0x8E);
        set_gate(14, isr14, KERNEL_CODE_SEL, 0x8E);
        set_gate(15, isr15, KERNEL_CODE_SEL, 0x8E);
        set_gate(16, isr16, KERNEL_CODE_SEL, 0x8E);
        set_gate(17, isr17, KERNEL_CODE_SEL, 0x8E);
        set_gate(18, isr18, KERNEL_CODE_SEL, 0x8E);
        set_gate(19, isr19, KERNEL_CODE_SEL, 0x8E);
        set_gate(20, isr20, KERNEL_CODE_SEL, 0x8E);
        set_gate(21, isr21, KERNEL_CODE_SEL, 0x8E);
        set_gate(22, isr22, KERNEL_CODE_SEL, 0x8E);
        set_gate(23, isr23, KERNEL_CODE_SEL, 0x8E);
        set_gate(24, isr24, KERNEL_CODE_SEL, 0x8E);
        set_gate(25, isr25, KERNEL_CODE_SEL, 0x8E);
        set_gate(26, isr26, KERNEL_CODE_SEL, 0x8E);
        set_gate(27, isr27, KERNEL_CODE_SEL, 0x8E);
        set_gate(28, isr28, KERNEL_CODE_SEL, 0x8E);
        set_gate(29, isr29, KERNEL_CODE_SEL, 0x8E);
        set_gate(30, isr30, KERNEL_CODE_SEL, 0x8E);
        set_gate(31, isr31, KERNEL_CODE_SEL, 0x8E);

        // IRQs (start at 32)
        set_gate(32, irq0, KERNEL_CODE_SEL, 0x8E);
        set_gate(33, irq1, KERNEL_CODE_SEL, 0x8E);
        set_gate(34, irq2, KERNEL_CODE_SEL, 0x8E);
        set_gate(35, irq3, KERNEL_CODE_SEL, 0x8E);
        set_gate(36, irq4, KERNEL_CODE_SEL, 0x8E);
        set_gate(37, irq5, KERNEL_CODE_SEL, 0x8E);
        set_gate(38, irq6, KERNEL_CODE_SEL, 0x8E);
        set_gate(39, irq7, KERNEL_CODE_SEL, 0x8E);
        set_gate(40, irq8, KERNEL_CODE_SEL, 0x8E);
        set_gate(41, irq9, KERNEL_CODE_SEL, 0x8E);
        set_gate(42, irq10, KERNEL_CODE_SEL, 0x8E);
        set_gate(43, irq11, KERNEL_CODE_SEL, 0x8E);
        set_gate(44, irq12, KERNEL_CODE_SEL, 0x8E);
        set_gate(45, irq13, KERNEL_CODE_SEL, 0x8E);
        set_gate(46, irq14, KERNEL_CODE_SEL, 0x8E);
        set_gate(47, irq15, KERNEL_CODE_SEL, 0x8E);

        IDT_PTR.limit = (size_of::<[IdtEntry; 256]>() - 1) as u16;
        IDT_PTR.base = &raw const IDT as *const _ as u64;

        asm!(
            "lidt [{}]",
            in(reg) &raw const IDT_PTR,
            options(readonly, nostack, preserves_flags)
        );
    }
}

const EXCEPTION_MESSAGES: [&str; 32] = [
    "DIVISION BY ZERO",
    "DEBUG",
    "NON MASKABLE INTERRUPT",
    "BREAKPOINT",
    "INTO DETECTED OVERFLOW",
    "OUT OF BOUNDS",
    "INVALID OPCODE",
    "NO COPROCESSOR",
    "DOUBLE FAULT",
    "COPROCESSOR SEGMENT OVERRUN",
    "BAD TSS",
    "SEGMENT NOT PRESENT",
    "STACK FAULT",
    "GENERAL PROTECTION FAULT",
    "PAGE FAULT",
    "UNKNOWN INTERRUPT",
    "CO-PROCESSOR FAULT",
    "ALIGNMENT CHECK",
    "MACHINE CHECK",
    "SIMD FLOATING POINT EXCEPTION",
    "VIRTUALIZATION EXCEPTION",
    "CONTROL PROTECTION EXCEPTION",
    "RESERVED",
    "RESERVED",
    "RESERVED",
    "RESERVED",
    "RESERVED",
    "RESERVED",
    "HYPervisor INJECTION EXCEPTION",
    "VMX COMMUNICATION EXCEPTION",
    "SECURITY EXCEPTION",
    "RESERVED",
];

#[unsafe(no_mangle)]
pub unsafe extern "sysv64" fn irq_handler(frame: *mut InterruptFrame) {
    let int_no = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!((*frame).int_no)) };
    if !(32..48).contains(&int_no) {
        let mut writer_guard = GLOBAL_WRITER.lock();
        if let Some(writer) = writer_guard.as_mut() {
            let _ = writeln!(writer, "Invalid IRQ vector: {:#x}", int_no);
        }
        return;
    }

    let irq = int_no - 32;

    match irq {
        0 => { /* Timer */ }
        1 => {
            //
        }
        _ =>
        {
            let mut writer_guard = GLOBAL_WRITER.lock();
            if let Some(writer) = writer_guard.as_mut() {
                let _ = writeln!(writer, "Unknown IRQ: {}", irq);
            }
        }
    }

    unsafe { crate::pic::notify_eoi(irq as u8) };
}

#[unsafe(no_mangle)]
pub unsafe extern "sysv64" fn exception_handler(frame: *mut InterruptFrame) {
    let int_no = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!((*frame).int_no)) };
    let err_code = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!((*frame).err_code)) };
    let rip = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!((*frame).rip)) };
    let rsp = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!((*frame).rsp)) };
    let cs = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!((*frame).cs)) };
    let rax = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!((*frame).rax)) };
    let rbx = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!((*frame).rbx)) };
    let rcx = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!((*frame).rcx)) };
    let rdx = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!((*frame).rdx)) };
    let rsi = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!((*frame).rsi)) };
    let rdi = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!((*frame).rdi)) };
    let rbp = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!((*frame).rbp)) };

    let mut writer_guard = if let Some(guard) = GLOBAL_WRITER.try_lock() {
        guard
    } else {
        unsafe {
            GLOBAL_WRITER.force_unlock();
        }
        GLOBAL_WRITER.lock()
    };

    if let Some(writer) = writer_guard.as_mut() {
        let _ = writeln!(writer, "\nEXCEPTION OCCURRED!");
        let _ = write!(writer, "INTERRUPT: {:#x} ", int_no);
        if (int_no as usize) < EXCEPTION_MESSAGES.len() {
            let _ = writeln!(writer, "({})", EXCEPTION_MESSAGES[int_no as usize]);
        } else {
            let _ = writeln!(writer, "");
        }
        let _ = writeln!(writer, "ERROR CODE: {:#x}", err_code);
        let _ = writeln!(writer, "RIP: {:#x}", rip);
        let _ = writeln!(
            writer,
            "RAX: {:#x}  RBX: {:#x}  RCX: {:#x}  RDX: {:#x}",
            rax, rbx, rcx, rdx
        );
        let _ = writeln!(
            writer,
            "RSI: {:#x}  RDI: {:#x}  RBP: {:#x}  RSP: {:#x}",
            rsi, rdi, rbp, rsp
        );

        if int_no == 14 {
            let cr2: u64;
            unsafe {
                core::arch::asm!("mov {}, cr2", out(reg) cr2, options(nomem, nostack, preserves_flags));
            }
            let _ = writeln!(writer, "CR2 (ADDR): {:#x}", cr2);
        }
    }

    // If the exception occurred in user mode (ring 3), kill the faulting
    // process and switch back to the next ready task (the shell) instead
    // of halting the entire system.
    let cpl = cs & 0x3;
    if cpl == 3 {
        if let Some(writer) = writer_guard.as_mut() {
            let _ = writeln!(writer, "Killing user process due to exception.");
        }
        core::mem::drop(writer_guard);

        // Swap GS base to access kernel-space per-CPU data (since CPU doesn't do it automatically on exception)
        unsafe {
            core::arch::asm!("swapgs", options(nomem, nostack, preserves_flags));
        }

        // terminate_task marks the current task Terminated, records an error
        // exit code, then calls switch_task which context-switches away.
        // We should never return here.
        crate::scheduler::terminate_task(0xDEAD);
    }

    // Kernel-mode fault (or no other task to switch to) — halt.
    loop {
        unsafe {
            core::arch::asm!("hlt", options(nomem, nostack, preserves_flags));
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "sysv64" fn debug_print_int_no(frame: *mut u8, val: u64) {
    let mut writer_guard = GLOBAL_WRITER.lock();
    if let Some(writer) = writer_guard.as_mut() {
        let _ = writeln!(
            writer,
            "DEBUG: RSP={:#x}, offset120={:#x}",
            frame as u64, val
        );
    }
}

// Assembly stubs
core::arch::global_asm!(
    r#"
.att_syntax
.macro ISR_NOERR n
    .global isr\n
    isr\n:
        pushq $0
        pushq $\n
        jmp isr_common
.endm

.macro ISR_ERR n
    .global isr\n
    isr\n:
        pushq $\n
        jmp isr_common
.endm

ISR_NOERR 0
ISR_NOERR 1
ISR_NOERR 2
ISR_NOERR 3
ISR_NOERR 4
ISR_NOERR 5
ISR_NOERR 6
ISR_NOERR 7
ISR_ERR   8
ISR_NOERR 9
ISR_ERR   10
ISR_ERR   11
ISR_ERR   12
ISR_ERR   13
ISR_ERR   14
ISR_NOERR 15
ISR_NOERR 16
ISR_ERR   17
ISR_NOERR 18
ISR_NOERR 19
ISR_NOERR 20
ISR_ERR   21
ISR_NOERR 22
ISR_NOERR 23
ISR_NOERR 24
ISR_NOERR 25
ISR_NOERR 26
ISR_NOERR 27
ISR_NOERR 28
ISR_ERR   29
ISR_ERR   30
ISR_NOERR 31

.macro IRQ n, num
    .global irq\n
    irq\n:
        pushq $0
        pushq $\num
        jmp irq_common
.endm

IRQ 0, 32
IRQ 1, 33
IRQ 2, 34
IRQ 3, 35
IRQ 4, 36
IRQ 5, 37
IRQ 6, 38
IRQ 7, 39
IRQ 8, 40
IRQ 9, 41
IRQ 10, 42
IRQ 11, 43
IRQ 12, 44
IRQ 13, 45
IRQ 14, 46
IRQ 15, 47

.global irq_common
irq_common:
    pushq %rax
    pushq %rbx
    pushq %rcx
    pushq %rdx
    pushq %rbp
    pushq %rdi
    pushq %rsi
    pushq %r8
    pushq %r9
    pushq %r10
    pushq %r11
    pushq %r12
    pushq %r13
    pushq %r14
    pushq %r15

    cld
    movq %rsp, %rdi
    movq %rsp, %rax
    andq $-16, %rsp
    subq $16, %rsp
    movq %rax, (%rsp)
    call irq_handler
    movq (%rsp), %rsp

    popq %r15
    popq %r14
    popq %r13
    popq %r12
    popq %r11
    popq %r10
    popq %r9
    popq %r8
    popq %rsi
    popq %rdi
    popq %rbp
    popq %rdx
    popq %rcx
    popq %rbx
    popq %rax

    addq $16, %rsp
    iretq

.global isr_common
isr_common:
    pushq %rax
    pushq %rbx
    pushq %rcx
    pushq %rdx
    pushq %rbp
    pushq %rdi
    pushq %rsi
    pushq %r8
    pushq %r9
    pushq %r10
    pushq %r11
    pushq %r12
    pushq %r13
    pushq %r14
    pushq %r15

    cld
    movq %rsp, %rdi
    movq %rsp, %rax
    andq $-16, %rsp
    subq $16, %rsp
    movq %rax, (%rsp)
    call exception_handler
    movq (%rsp), %rsp

    popq %r15
    popq %r14
    popq %r13
    popq %r12
    popq %r11
    popq %r10
    popq %r9
    popq %r8
    popq %rsi
    popq %rdi
    popq %rbp
    popq %rdx
    popq %rcx
    popq %rbx
    popq %rax

    addq $16, %rsp
    iretq
"#
);
