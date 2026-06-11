#![allow(static_mut_refs)]
use alloc::boxed::Box;
use alloc::vec::Vec;
use core::arch::asm;
use core::sync::atomic::{AtomicUsize, Ordering};

// Re-using the allocator from the crate

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    Ready,
    Running,
    Terminated,
}

pub struct Task {
    pub id: usize,
    pub stack_top: u64,    // Saved Stack Pointer (current RSP)
    pub stack_bottom: u64, // For deallocation reference (user stack if usermode)
    pub status: TaskStatus,
    pub kernel_stack_bottom: u64,
    pub gs_base: u64, // Pointer to KernelGsBase struct
    pub exit_code: usize,
}

pub struct Scheduler {
    tasks: Vec<Task>,
    current_task_index: usize,
}

static mut SCHEDULER: Option<Scheduler> = None;
static NEXT_TASK_ID: AtomicUsize = AtomicUsize::new(1); // 0 is reserved for main kernel task
static SCHEDULER_LOCK: crate::interrupts::InterruptSpinlock<()> = crate::interrupts::InterruptSpinlock::new(());

/// Initialize the global scheduler.
/// This must be called only once.
pub unsafe fn init() {
    let _guard = SCHEDULER_LOCK.lock();
    unsafe {
        SCHEDULER = Some(Scheduler {
            tasks: Vec::new(),
            current_task_index: 0,
        });
    }

    // Create a dummy task for the currently running kernel thread (Main Task)
    let main_task = Task {
        id: 0,
        stack_top: 0,
        stack_bottom: 0,
        status: TaskStatus::Running,
        kernel_stack_bottom: 0,
        gs_base: unsafe { crate::syscall::get_global_gs_base() },
        exit_code: 0,
    };

    if let Some(scheduler) = unsafe { SCHEDULER.as_mut() } {
        scheduler.tasks.push(main_task);
    }
}

pub fn add_new_user_task(entry_point: u64, user_rsp: u64, stack_size: usize) -> usize {
    let _guard = SCHEDULER_LOCK.lock();
    unsafe {
        if let Some(scheduler) = SCHEDULER.as_mut() {
            let id = NEXT_TASK_ID.fetch_add(1, Ordering::SeqCst);

            // 1. Allocate Kernel Stack
            let kernel_stack_bottom = crate::allocator::alloc(stack_size) as u64;
            let kernel_stack_top = kernel_stack_bottom + stack_size as u64;

            // 2. Setup KernelGsBase
            let kgs_base = Box::new(crate::syscall::KernelGsBase {
                kernel_stack: kernel_stack_top,
                user_stack: user_rsp, // Initial user RSP
                scratch: 0,
            });
            let gs_base_ptr = Box::into_raw(kgs_base) as u64;

            // 3. Setup Stack Frame for IRETQ (to enter usermode)
            // We'll simulate a stack that context_switch can jump into.
            // When we switch TO this task, context_switch will 'ret' to our entry logic.

            // Let's use a simpler approach:
            // The task will start at a kernel helper 'user_task_entry'

            let mut sp = kernel_stack_top as *mut u64;

            // Since it's a new task, we need to push the usermode registers
            // that our syscall/interrupt handler would expect, OR we just
            // set it up so context_switch 'ret's into a helper that does iretq.

            sp = sp.sub(1);
            *sp = crate::gdt::USER_DATA_SEL as u64; // SS
            sp = sp.sub(1);
            *sp = user_rsp; // RSP
            sp = sp.sub(1);
            *sp = 0x202; // RFLAGS
            sp = sp.sub(1);
            *sp = crate::gdt::USER_CODE_SEL as u64; // CS
            sp = sp.sub(1);
            *sp = entry_point; // RIP

            // Now push caller-saved registers that context_switch expects
            sp = sp.sub(1);
            *sp = user_task_trampoline as u64; // RIP for context_switch 'ret'
            sp = sp.sub(1);
            *sp = 0; // RBP
            sp = sp.sub(1);
            *sp = 0; // RBX
            sp = sp.sub(1);
            *sp = 0; // R12
            sp = sp.sub(1);
            *sp = 0; // R13
            sp = sp.sub(1);
            *sp = 0; // R14
            sp = sp.sub(1);
            *sp = 0; // R15

            let task = Task {
                id,
                stack_top: sp as u64,
                stack_bottom: user_rsp - stack_size as u64,
                status: TaskStatus::Ready,
                kernel_stack_bottom,
                gs_base: gs_base_ptr,
                exit_code: 0,
            };

            scheduler.tasks.push(task);
            id
        } else {
            0
        }
    }
}

#[unsafe(naked)]
unsafe extern "C" fn user_task_trampoline() {
    core::arch::naked_asm!("swapgs", "iretq");
}

pub fn add_new_task(entry_point: extern "C" fn(), stack_bottom: u64, stack_size: usize) {
    let _guard = SCHEDULER_LOCK.lock();
    unsafe {
        if let Some(scheduler) = SCHEDULER.as_mut() {
            let id = NEXT_TASK_ID.fetch_add(1, Ordering::SeqCst);
            // 2. Setup Stack Frame for Context Switch
            let stack_top = stack_bottom + stack_size as u64;

            // Stack grows DOWN.
            // Alignment Requirement: RSP + 8 must be 16-byte aligned.
            // So on ENTRY (instruction 0), RSP should be `...8`.
            // Our `stack_top` is 16-byte aligned (`...0`) usually.
            // So we should start filling from `stack_top - 8`.

            let mut sp = (stack_top - 8) as *mut u64;

            // Return Address (RIP) - This is where we jump when we switch TO this task
            sp = sp.sub(1);
            *sp = entry_point as u64; // RIP

            // RBP
            sp = sp.sub(1);
            *sp = 0; // Initial RBP

            // RBX
            sp = sp.sub(1);
            *sp = 0;

            // R12
            sp = sp.sub(1);
            *sp = 0;

            // R13
            sp = sp.sub(1);
            *sp = 0;

            // R14
            sp = sp.sub(1);
            *sp = 0;

            // R15
            sp = sp.sub(1);
            *sp = 0; // r15

            let task = Task {
                id,
                stack_top: sp as u64, // The saved RSP
                stack_bottom,
                status: TaskStatus::Ready,
                kernel_stack_bottom: 0,
                gs_base: 0,
                exit_code: 0,
            };

            scheduler.tasks.push(task);
        }
    }
}

pub fn switch_task() {
    unsafe {
        let guard = SCHEDULER_LOCK.lock();
        if let Some(scheduler) = SCHEDULER.as_mut() {
            let current_index = scheduler.current_task_index;

            // Round-robin: find next Ready task
            let mut next_index = (current_index + 1) % scheduler.tasks.len();
            let mut found = false;

            // Loop once to find a ready task
            for _ in 0..scheduler.tasks.len() {
                if scheduler.tasks[next_index].status == TaskStatus::Ready {
                    found = true;
                    break;
                }
                // If current task is running, it's also a candidate (if it's the only one, or we just cycle back)
                // But wait, if we are currently Running, we should be marked Ready if we yield?
                // Or we just stay Running if it's round robin.

                // Let's adhere to: Only switch to Ready tasks.
                // The current task is "Running". If we yield, it becomes "Ready".

                next_index = (next_index + 1) % scheduler.tasks.len();
            }

            if !found {
                // If no other task is Ready, check if current is still runnable.
                if scheduler.tasks[current_index].status == TaskStatus::Terminated {
                    // We are terminated and no one else to run? deadlock/halt
                    core::mem::drop(guard);
                    crate::println!("All tasks could be terminated, or deadlock. Halting.");
                    loop {
                        core::arch::asm!("hlt");
                    }
                }
                // Just continue current task
                return;
            }

            if next_index == current_index {
                // No switch needed
                return;
            }

            // Update statuses
            let old_index = current_index;

            if scheduler.tasks[old_index].status == TaskStatus::Running {
                scheduler.tasks[old_index].status = TaskStatus::Ready;
            }

            scheduler.tasks[next_index].status = TaskStatus::Running;
            scheduler.current_task_index = next_index;

            let old_stack_ref = &mut scheduler.tasks[old_index].stack_top as *mut u64;
            let new_stack = scheduler.tasks[next_index].stack_top;
            let new_gs_base = scheduler.tasks[next_index].gs_base;

            // Update Active GS Base (MSR_GS_BASE)
            if new_gs_base != 0 {
                wrmsr(0xC0000101, new_gs_base);
            }

            // Drop SCHEDULER_LOCK immediately before context switch to prevent deadlock
            core::mem::drop(guard);

            // Perform the low-level switch
            context_switch(old_stack_ref, new_stack);
        }
    }
}

unsafe fn wrmsr(msr: u32, value: u64) {
    let low = value as u32;
    let high = (value >> 32) as u32;
    unsafe {
        asm!("wrmsr", in("ecx") msr, in("eax") low, in("edx") high, options(nomem, nostack, preserves_flags));
    }
}

pub fn terminate_task(exit_code: usize) {
    let guard = SCHEDULER_LOCK.lock();
    unsafe {
        if let Some(scheduler) = SCHEDULER.as_mut() {
            let current_index = scheduler.current_task_index;
            scheduler.tasks[current_index].status = TaskStatus::Terminated;
            scheduler.tasks[current_index].exit_code = exit_code;

            crate::println!("Task {} terminated with exit code {}.", scheduler.tasks[current_index].id, exit_code);

            // Drop lock before calling switch_task which has its own lock!
            core::mem::drop(guard);
            switch_task();
        }
    }
}

#[unsafe(naked)]
#[unsafe(no_mangle)]
unsafe extern "sysv64" fn context_switch(old_stack_ptr: *mut u64, new_stack_ptr: u64) {
    core::arch::naked_asm!(
        "push r15",
        "push r14",
        "push r13",
        "push r12",
        "push rbx",
        "push rbp",
        // Save current RSP to the old_stack_ptr location
        "mov [rdi], rsp",
        // Load new RSP
        "mov rsp, rsi",
        "pop rbp",
        "pop rbx",
        "pop r12",
        "pop r13",
        "pop r14",
        "pop r15",
        "ret", // Jumps to return address on top of new stack
    );
}

// Helper to get current task id
pub fn current_task_id() -> usize {
    let _guard = SCHEDULER_LOCK.lock();
    unsafe {
        if let Some(scheduler) = SCHEDULER.as_ref() {
            scheduler.tasks[scheduler.current_task_index].id
        } else {
            0
        }
    }
}

pub fn get_task_status(task_id: usize) -> usize {
    let _guard = SCHEDULER_LOCK.lock();
    unsafe {
        if let Some(scheduler) = SCHEDULER.as_ref() {
            for task in &scheduler.tasks {
                if task.id == task_id {
                    return match task.status {
                        TaskStatus::Ready => 0,
                        TaskStatus::Running => 1,
                        TaskStatus::Terminated => 2,
                    };
                }
            }
        }
        3 // Not found
    }
}

pub fn get_task_exit_code(task_id: usize) -> usize {
    let _guard = SCHEDULER_LOCK.lock();
    unsafe {
        if let Some(scheduler) = SCHEDULER.as_ref() {
            for task in &scheduler.tasks {
                if task.id == task_id {
                    return task.exit_code;
                }
            }
        }
        0
    }
}
