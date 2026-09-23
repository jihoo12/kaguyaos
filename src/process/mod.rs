#![allow(static_mut_refs)]
use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::vec::Vec;
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
    pub cpu_affinity: usize,
    pub kernel_stack_bottom: u64,
    pub kernel_stack_top: u64,
    pub gs_base: u64, // User GS base value
    pub user_rsp: u64, // User stack pointer value
    pub exit_code: usize,
}

pub struct Scheduler {
    tasks: Vec<Box<Task>>,
    // One runnable queue per logical CPU. Running tasks are never present in a queue.
    // For now new tasks stay on CPU 0; a follow-up change can enable AP scheduling
    // and distribute/steal tasks without changing the task store.
    run_queues: [VecDeque<usize>; crate::processor::MAX_AP_COUNT + 1],
}

static mut SCHEDULER: Option<Scheduler> = None;
static NEXT_TASK_ID: AtomicUsize = AtomicUsize::new(1); // 0 is reserved for main kernel task
static SCHEDULER_LOCK: crate::sync::Spinlock<()> = crate::sync::Spinlock::new(());

/// Number of PIT ticks a task may run before round-robin preemption.
/// The PIT currently runs at 100 Hz, so 5 ticks is roughly a 50 ms slice.
pub const DEFAULT_TIME_SLICE_TICKS: u32 = 5;

/// Initialize the global scheduler.
/// This must be called only once.
pub unsafe fn init() {
    let _guard = SCHEDULER_LOCK.lock();
    unsafe {
        SCHEDULER = Some(Scheduler {
            tasks: Vec::new(),
            // Keep the queue table inline. A Vec<VecDeque<_>> adds a heap
            // allocation during scheduler init and changes the kernel heap layout
            // before the first user task is created. The fixed-size table also
            // matches the statically bounded PERCPU_DATA_SLOTS topology.
            run_queues: [const { VecDeque::new() }; crate::processor::MAX_AP_COUNT + 1],
        });
    }

    // Create a dummy task for the currently running kernel thread (Main Task)
    let main_task = Task {
        id: 0,
        stack_top: 0,
        stack_bottom: 0,
        status: TaskStatus::Running,
        cpu_affinity: 0,
        kernel_stack_bottom: 0,
        kernel_stack_top: 0,
        gs_base: 0,
        user_rsp: 0,
        exit_code: 0,
    };

    if let Some(scheduler) = unsafe { SCHEDULER.as_mut() } {
        scheduler.tasks.push(Box::new(main_task));
    }
}

fn select_target_cpu(scheduler: &Scheduler) -> usize {
    let online_cpus = (crate::processor::online_ap_count() as usize + 1)
        .min(crate::processor::MAX_AP_COUNT + 1);

    let mut best_cpu = 0;
    let mut best_load = usize::MAX;

    for cpu in 0..online_cpus {
        let mut load = scheduler.run_queues[cpu].len();
        unsafe {
            if crate::processor::PERCPU_DATA_SLOTS[cpu].current_task_index != usize::MAX {
                load += 1;
            }
        }

        if load < best_load {
            best_load = load;
            best_cpu = cpu;
        }
    }

    best_cpu
}

pub fn add_new_user_task(entry_point: u64, user_rsp: u64, stack_size: usize, rdi: u64, rsi: u64) -> usize {
    let _guard = SCHEDULER_LOCK.lock();
    unsafe {
        if let Some(scheduler) = SCHEDULER.as_mut() {
            let id = NEXT_TASK_ID.fetch_add(1, Ordering::SeqCst);

            // 1. Allocate Kernel Stack
            let kernel_stack_bottom = crate::memory::heap::alloc(stack_size) as u64;
            let kernel_stack_top = kernel_stack_bottom + stack_size as u64;

            // 2. Setup Stack Frame for IRETQ (to enter usermode)
            let mut sp = kernel_stack_top as *mut u64;

            // IRETQ frame
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

            // Context switch frame (push order: R15,R14,R13,R12,RBX,RBP,RDI,RSI)
            sp = sp.sub(1);
            *sp = user_task_trampoline as *const () as u64; // return address
            sp = sp.sub(1);
            *sp = 0; // R15
            sp = sp.sub(1);
            *sp = 0; // R14
            sp = sp.sub(1);
            *sp = 0; // R13
            sp = sp.sub(1);
            *sp = 0; // R12
            sp = sp.sub(1);
            *sp = 0; // RBX
            sp = sp.sub(1);
            *sp = 0; // RBP
            sp = sp.sub(1);
            *sp = rdi; // RDI = args pointer
            sp = sp.sub(1);
            *sp = rsi; // RSI = args length

            let task = Task {
                id,
                stack_top: sp as u64,
                stack_bottom: user_rsp - stack_size as u64,
                status: TaskStatus::Ready,
                cpu_affinity: 0,
                kernel_stack_bottom,
                kernel_stack_top,
                gs_base: 0,
                user_rsp,
                exit_code: 0,
            };

            scheduler.tasks.push(Box::new(task));
            let task_index = scheduler.tasks.len() - 1;
            let target_cpu = select_target_cpu(scheduler);
            scheduler.tasks[task_index].cpu_affinity = target_cpu;
            scheduler.run_queues[target_cpu].push_back(task_index);
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
                cpu_affinity: 0,
                kernel_stack_bottom: stack_bottom,
                kernel_stack_top: stack_top,
                gs_base: 0,
                user_rsp: 0,
                exit_code: 0,
            };

            scheduler.tasks.push(Box::new(task));
            let task_index = scheduler.tasks.len() - 1;
            let target_cpu = select_target_cpu(scheduler);
            scheduler.tasks[task_index].cpu_affinity = target_cpu;
            scheduler.run_queues[target_cpu].push_back(task_index);
        }
    }
}

pub fn switch_task() {
    unsafe {
        let guard = SCHEDULER_LOCK.lock();
        if let Some(scheduler) = SCHEDULER.as_mut() {
            let percpu = crate::processor::get_percpu_data();
            if percpu.is_null() {
                return;
            }
            let current_index = (*percpu).current_task_index;
            let cpu_index = (*percpu).cpu_index as usize;

            // Each CPU selects only from its own run queue. Keeping queue ownership
            // local is the foundation for AP scheduling and later work stealing.
            let next_index = loop {
                match scheduler.run_queues[cpu_index].pop_front() {
                    Some(index) if scheduler.tasks[index].status == TaskStatus::Ready => break index,
                    Some(_) => continue, // Defensive: discard a stale queue entry.
                    None => {
                        if current_index != usize::MAX
                            && scheduler.tasks[current_index].status == TaskStatus::Terminated
                        {
                            core::mem::drop(guard);
                            crate::println!("All tasks could be terminated, or deadlock. Halting.");
                            loop {
                                core::arch::asm!("hlt");
                            }
                        }
                        return;
                    }
                }
            };

            // A running task goes to the tail, giving round-robin fairness.
            // Terminated tasks are deliberately not requeued.
            if current_index != usize::MAX
                && scheduler.tasks[current_index].status == TaskStatus::Running
            {
                scheduler.tasks[current_index].status = TaskStatus::Ready;
                scheduler.run_queues[cpu_index].push_back(current_index);
            }

            scheduler.tasks[next_index].status = TaskStatus::Running;
            scheduler.tasks[next_index].cpu_affinity = cpu_index;
            (*percpu).current_task_index = next_index;
            (*percpu).scheduler_ticks_left = DEFAULT_TIME_SLICE_TICKS;
            (*percpu).need_resched = false;

            let mut dummy_sp = 0u64;
            let old_stack_ref = if current_index != usize::MAX {
                &mut scheduler.tasks[current_index].stack_top as *mut u64
            } else {
                &mut dummy_sp as *mut u64
            };
            let new_stack = scheduler.tasks[next_index].stack_top;

            let new_kernel_stack_top = scheduler.tasks[next_index].kernel_stack_top;
            if new_kernel_stack_top != 0 {
                (*percpu).kernel_stack = new_kernel_stack_top;
                crate::gdt::set_tss_stack_cpu(cpu_index, new_kernel_stack_top);
            }

            if current_index != usize::MAX {
                scheduler.tasks[current_index].user_rsp = (*percpu).user_stack;
            }
            (*percpu).user_stack = scheduler.tasks[next_index].user_rsp;

            if current_index != usize::MAX {
                let old_user_gs =
                    crate::processor::rdmsr(crate::processor::MSR_IA32_KERNEL_GS_BASE);
                scheduler.tasks[current_index].gs_base = old_user_gs;
            }

            let new_user_gs = scheduler.tasks[next_index].gs_base;
            crate::processor::wrmsr(crate::processor::MSR_IA32_KERNEL_GS_BASE, new_user_gs);

            core::mem::drop(guard);
            context_switch(old_stack_ref, new_stack);
        }
    }
}

/// Account one timer tick for the current CPU.
///
/// The IRQ path only calls this for a task interrupted in user mode. When the
/// quantum expires we defer the actual context switch until after the PIC EOI,
/// avoiding a switch while the timer interrupt is still in-service.
pub fn scheduler_tick() {
    unsafe {
        let percpu = crate::processor::get_percpu_data();
        if percpu.is_null() || (*percpu).current_task_index == usize::MAX {
            return;
        }

        if (*percpu).scheduler_ticks_left > 0 {
            (*percpu).scheduler_ticks_left -= 1;
        }
        if (*percpu).scheduler_ticks_left == 0 {
            (*percpu).need_resched = true;
        }
    }
}

/// Consume a pending reschedule request and switch tasks if necessary.
pub fn reschedule_if_needed() {
    unsafe {
        let percpu = crate::processor::get_percpu_data();
        if percpu.is_null() || !(*percpu).need_resched {
            return;
        }
        (*percpu).need_resched = false;
    }
    switch_task();
}

pub fn terminate_task(exit_code: usize) {
    let guard = SCHEDULER_LOCK.lock();
    unsafe {
        if let Some(scheduler) = SCHEDULER.as_mut() {
            let percpu = crate::processor::get_percpu_data();
            if !percpu.is_null() {
                let current_index = (*percpu).current_task_index;
                if current_index != usize::MAX {
                    scheduler.tasks[current_index].status = TaskStatus::Terminated;
                    scheduler.tasks[current_index].exit_code = exit_code;

                    crate::println!("Task {} terminated with exit code {}.", scheduler.tasks[current_index].id, exit_code);
                }
            }

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
        "push rdi",
        "push rsi",
        // Save current RSP to the old_stack_ptr location
        "mov [rdi], rsp",
        // Load new RSP
        "mov rsp, rsi",
        "pop rsi",
        "pop rdi",
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
            let percpu = crate::processor::get_percpu_data();
            if !percpu.is_null() {
                let current_index = (*percpu).current_task_index;
                if current_index != usize::MAX {
                    return scheduler.tasks[current_index].id;
                }
            }
        }
        0
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

pub fn run_ap_scheduler() -> ! {
    // APs poll the network NIC continuously.
    // They never run user tasks — only the BSP schedules tasks.
    // NOTE: We cannot use `hlt` here because no IRQs are routed to the AP
    // (PIC only delivers to BSP). Instead we busy-poll with a small delay.
    unsafe {
        core::arch::asm!("sti");
        loop {
            crate::drivers::net::poll();
            // Yield some CPU time; ~10k spin-loops ≈ a few hundred µs.
            for _ in 0..10_000 {
                core::hint::spin_loop();
            }
        }
    }
}
