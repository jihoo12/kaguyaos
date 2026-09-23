#![allow(static_mut_refs)]
use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

// Re-using the allocator from the crate

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    Ready,
    Running,
    Sleeping,
    Waiting,
    Zombie,
}

pub struct Task {
    pub id: usize,
    pub stack_top: u64,    // Saved Stack Pointer (current RSP)
    pub stack_bottom: u64, // For deallocation reference (user stack if usermode)
    pub status: TaskStatus,
    /// Preferred/last CPU for normal placement and wakeup.
    pub cpu_affinity: usize,
    /// Hard CPU pin. `usize::MAX` means migratable.
    pub pinned_cpu: usize,
    pub kernel_stack_bottom: u64,
    pub kernel_stack_top: u64,
    pub gs_base: u64, // User GS base value
    pub user_rsp: u64, // User stack pointer value
    pub exit_code: usize,
    pub wake_tick: u64,
    pub waiting_for: usize,
}

/// Scheduler metadata protected by SCHEDULER_LOCK.
///
/// Task slots stay stable once inserted, so run queues can continue to carry
/// indices while later scheduler work narrows the metadata critical section.
struct SchedulerMetadata {
    tasks: Vec<Box<Task>>,
}

pub struct Scheduler {
    metadata: SchedulerMetadata,
    // One runnable queue per logical CPU. Running tasks are never present in a queue.
    // Queue storage has its own per-CPU lock. Task state and placement decisions
    // still require SCHEDULER_LOCK; lock order remains metadata -> one run queue.
    run_queues: [crate::sync::Spinlock<VecDeque<usize>>; crate::processor::MAX_AP_COUNT + 1],
}

impl Scheduler {
    #[inline]
    fn tasks(&self) -> &Vec<Box<Task>> {
        &self.metadata.tasks
    }

    #[inline]
    fn tasks_mut(&mut self) -> &mut Vec<Box<Task>> {
        &mut self.metadata.tasks
    }
}

static mut SCHEDULER: Option<Scheduler> = None;
static NEXT_TASK_ID: AtomicUsize = AtomicUsize::new(1); // 0 is reserved for main kernel task
static SCHEDULER_READY: AtomicBool = AtomicBool::new(false);
static SCHEDULER_TICKS: AtomicUsize = AtomicUsize::new(0);

/// Scheduler-owned publication of each CPU's current task slot.
///
/// Local CPUs still use PercpuData for their fast path. Cross-CPU scheduler
/// decisions use this atomic mirror instead of racing on another CPU's GS data.
static CPU_CURRENT_TASK: [AtomicUsize; crate::processor::MAX_AP_COUNT + 1] =
    [const { AtomicUsize::new(usize::MAX) }; crate::processor::MAX_AP_COUNT + 1];

#[inline]
fn publish_current_task(cpu: usize, task_index: usize) {
    CPU_CURRENT_TASK[cpu].store(task_index, Ordering::Release);
}

#[inline]
fn published_current_task(cpu: usize) -> usize {
    CPU_CURRENT_TASK[cpu].load(Ordering::Acquire)
}

/// Accessors make the global-lock boundary explicit. Queue locks do not protect
/// task fields; callers touching task metadata must already own SCHEDULER_LOCK.
#[inline]
fn task_ref(scheduler: &Scheduler, index: usize) -> &Task {
    &scheduler.metadata.tasks[index]
}

#[inline]
fn task_mut(scheduler: &mut Scheduler, index: usize) -> &mut Task {
    &mut scheduler.metadata.tasks[index]
}
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
            metadata: SchedulerMetadata { tasks: Vec::new() },
            // Keep the queue table inline. A Vec<VecDeque<_>> adds a heap
            // allocation during scheduler init and changes the kernel heap layout
            // before the first user task is created. The fixed-size table also
            // matches the statically bounded PERCPU_DATA_SLOTS topology.
            run_queues: [const { crate::sync::Spinlock::new(VecDeque::new()) }; crate::processor::MAX_AP_COUNT + 1],
        });
    }

    // Create a dummy task for the currently running kernel thread (Main Task)
    let main_task = Task {
        id: 0,
        stack_top: 0,
        stack_bottom: 0,
        status: TaskStatus::Running,
        cpu_affinity: 0,
        pinned_cpu: 0,
        kernel_stack_bottom: 0,
        kernel_stack_top: 0,
        gs_base: 0,
        user_rsp: 0,
        exit_code: 0,
        wake_tick: 0,
        waiting_for: usize::MAX,
    };

    if let Some(scheduler) = unsafe { SCHEDULER.as_mut() } {
        scheduler.metadata.tasks.push(Box::new(main_task));
    }

    // APs are started before the process scheduler is initialized. Publish the
    // fully initialized scheduler only after its task store and BSP dummy task
    // are ready.
    SCHEDULER_READY.store(true, Ordering::Release);
}

fn select_target_cpu(scheduler: &Scheduler) -> usize {
    let online_cpus = (crate::processor::online_ap_count() as usize + 1)
        .min(crate::processor::MAX_AP_COUNT + 1);

    // Prefer an idle AP with an empty queue. Otherwise fall back to the BSP.
    // Both BSP and AP user tasks are timer-preemptible; keeping this placement
    // policy conservative avoids changing load balancing in the idle-context PR.
    for cpu in 1..online_cpus {
        if published_current_task(cpu) == usize::MAX
            && scheduler.run_queues[cpu].lock().is_empty()
        {
            return cpu;
        }
    }

    0
}

fn pop_ready_task(scheduler: &mut Scheduler, cpu_index: usize) -> Option<usize> {
    let mut queue = scheduler.run_queues[cpu_index].lock();
    while let Some(index) = queue.pop_front() {
        if scheduler.metadata.tasks[index].status == TaskStatus::Ready {
            return Some(index);
        }
    }
    None
}

fn steal_ready_task(scheduler: &mut Scheduler, thief_cpu: usize) -> Option<usize> {
    let online_cpus = (crate::processor::online_ap_count() as usize + 1)
        .min(crate::processor::MAX_AP_COUNT + 1);

    // Do not migrate the only queued task away from its owner. Sleeping tasks
    // wake back onto their affinity CPU, and that CPU may currently be running
    // the task that will wake them (for example init waiting for a ping child).
    // Steal only excess queued work so every non-idle owner keeps one runnable
    // task that can drive its local scheduler/wakeup path.
    // Inspect one queue at a time while SCHEDULER_LOCK keeps task metadata and
    // placement stable. Never hold two run-queue locks simultaneously.
    let mut victim_cpu = None;
    let mut victim_len = 1usize;
    for cpu in 0..online_cpus {
        if cpu == thief_cpu {
            continue;
        }
        let len = scheduler.run_queues[cpu].lock().len();
        if len > victim_len {
            victim_cpu = Some(cpu);
            victim_len = len;
        }
    }
    let victim_cpu = victim_cpu?;
    let mut queue = scheduler.run_queues[victim_cpu].lock();

    while queue.len() > 1 {
        // Hard-pinned tasks stay on their owner CPU. cpu_affinity is only a
        // soft preferred/last CPU and may change when ordinary work is stolen.
        let steal_pos = queue
            .iter()
            .rposition(|&index| {
                scheduler.metadata.tasks[index].status == TaskStatus::Ready
                    && scheduler.metadata.tasks[index].pinned_cpu == usize::MAX
            })?;
        let index = queue.remove(steal_pos)?;
        scheduler.metadata.tasks[index].cpu_affinity = thief_cpu;
        return Some(index);
    }
    None
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
                pinned_cpu: usize::MAX,
                kernel_stack_bottom,
                kernel_stack_top,
                gs_base: 0,
                user_rsp,
                exit_code: 0,
                wake_tick: 0,
                waiting_for: usize::MAX,
            };

            scheduler.metadata.tasks.push(Box::new(task));
            let task_index = scheduler.metadata.tasks.len() - 1;
            // Bootstrap init owns the initial userspace control flow and shell,
            // so pin it explicitly to the BSP. Other user tasks remain migratable.
            let target_cpu = if id == 1 {
                scheduler.metadata.tasks[task_index].pinned_cpu = 0;
                0
            } else {
                select_target_cpu(scheduler)
            };
            scheduler.metadata.tasks[task_index].cpu_affinity = target_cpu;
            scheduler.run_queues[target_cpu].lock().push_back(task_index);
            if target_cpu != 0 {
                crate::processor::send_ipi(target_cpu, crate::interrupts::SCHEDULER_WAKE_VECTOR);
            }
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
                pinned_cpu: usize::MAX,
                kernel_stack_bottom: stack_bottom,
                kernel_stack_top: stack_top,
                gs_base: 0,
                user_rsp: 0,
                exit_code: 0,
                wake_tick: 0,
                waiting_for: usize::MAX,
            };

            scheduler.metadata.tasks.push(Box::new(task));
            let task_index = scheduler.metadata.tasks.len() - 1;
            let target_cpu = select_target_cpu(scheduler);
            scheduler.metadata.tasks[task_index].cpu_affinity = target_cpu;
            scheduler.run_queues[target_cpu].lock().push_back(task_index);
            if target_cpu != 0 {
                crate::processor::send_ipi(target_cpu, crate::interrupts::SCHEDULER_WAKE_VECTOR);
            }
        }
    }
}

pub fn switch_task() {
    unsafe {
        let guard = SCHEDULER_LOCK.lock();
        if let Some(scheduler) = SCHEDULER.as_mut() {
            // Keep the hot scheduling decision focused on the current CPU.
            // Global sleeper/zombie maintenance is performed by CPU0 below,
            // avoiding repeated full task-table scans on every AP reschedule.
            let percpu = crate::processor::get_percpu_data();
            if percpu.is_null() {
                return;
            }
            let current_index = (*percpu).current_task_index;
            let cpu_index = (*percpu).cpu_index as usize;

            // CPU0 owns global scheduler maintenance. Timer IRQs remain lock-free;
            // this work runs only after entering the normal scheduler path.
            if cpu_index == 0 {
                let now = SCHEDULER_TICKS.load(Ordering::Relaxed) as u64;
                wake_sleeping_tasks_locked(scheduler, now);
                reap_zombies(scheduler);
            }

            // Prefer local work. Only an otherwise-idle CPU steals one Ready
            // task from the most loaded remote queue.
            let Some(next_index) = pop_ready_task(scheduler, cpu_index)
                .or_else(|| steal_ready_task(scheduler, cpu_index))
            else {
                if current_index != usize::MAX
                    && matches!(
                        scheduler.metadata.tasks[current_index].status,
                        TaskStatus::Zombie | TaskStatus::Sleeping | TaskStatus::Waiting
                    )
                {
                    // Every CPU keeps a saved idle scheduler context. A blocked
                    // or zombie task must switch back to it when no runnable work
                    // exists locally or remotely.
                    if (*percpu).idle_stack != 0 {
                        let old_stack_ref =
                            &mut scheduler.metadata.tasks[current_index].stack_top as *mut u64;
                        let idle_stack = (*percpu).idle_stack;
                        scheduler.metadata.tasks[current_index].user_rsp = (*percpu).user_stack;
                        (*percpu).current_task_index = usize::MAX;
                        publish_current_task(cpu_index, usize::MAX);
                        (*percpu).user_stack = 0;
                        (*percpu).scheduler_ticks_left = 0;
                        (*percpu).need_resched = false;
                        crate::processor::wrmsr(
                            crate::processor::MSR_IA32_KERNEL_GS_BASE,
                            0,
                        );
                        core::mem::drop(guard);
                        context_switch(old_stack_ref, idle_stack);
                        return;
                    }
                }
                return;
            };

            // A running task goes to the tail, giving round-robin fairness.
            // Sleeping/waiting/zombie tasks are deliberately not requeued.
            if current_index != usize::MAX
                && scheduler.metadata.tasks[current_index].status == TaskStatus::Running
            {
                scheduler.metadata.tasks[current_index].status = TaskStatus::Ready;
                scheduler.run_queues[cpu_index].lock().push_back(current_index);
            }

            // A sleeping task must never be selected from a stale queue entry.
            // This can happen when the current task was already queued before it
            // entered sleep. Skip it until the timer wakeup marks it Ready again.
            if scheduler.metadata.tasks[next_index].status != TaskStatus::Ready {
                return;
            }

            let pinned_cpu = scheduler.metadata.tasks[next_index].pinned_cpu;
            if pinned_cpu != usize::MAX && pinned_cpu != cpu_index {
                // Defensive: a pinned task should never enter another CPU's queue.
                scheduler.run_queues[pinned_cpu.min(crate::processor::MAX_AP_COUNT)]
                    .lock()
                    .push_back(next_index);
                return;
            }
            scheduler.metadata.tasks[next_index].wake_tick = 0;
            scheduler.metadata.tasks[next_index].status = TaskStatus::Running;
            scheduler.metadata.tasks[next_index].cpu_affinity = cpu_index;
            (*percpu).current_task_index = next_index;
            publish_current_task(cpu_index, next_index);
            (*percpu).scheduler_ticks_left = DEFAULT_TIME_SLICE_TICKS;
            (*percpu).need_resched = false;

            let old_stack_ref = if current_index != usize::MAX {
                &mut scheduler.metadata.tasks[current_index].stack_top as *mut u64
            } else {
                // Save this CPU's scheduler loop as its idle context. Both BSP
                // and AP tasks can later return here after sleeping or exiting.
                &mut (*percpu).idle_stack as *mut u64
            };
            let new_stack = scheduler.metadata.tasks[next_index].stack_top;

            let new_kernel_stack_top = scheduler.metadata.tasks[next_index].kernel_stack_top;
            if new_kernel_stack_top != 0 {
                (*percpu).kernel_stack = new_kernel_stack_top;
                crate::gdt::set_tss_stack_cpu(cpu_index, new_kernel_stack_top);
            }

            if current_index != usize::MAX {
                scheduler.metadata.tasks[current_index].user_rsp = (*percpu).user_stack;
            }
            (*percpu).user_stack = scheduler.metadata.tasks[next_index].user_rsp;

            if current_index != usize::MAX {
                let old_user_gs =
                    crate::processor::rdmsr(crate::processor::MSR_IA32_KERNEL_GS_BASE);
                scheduler.metadata.tasks[current_index].gs_base = old_user_gs;
            }

            let new_user_gs = scheduler.metadata.tasks[next_index].gs_base;
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
/// Advance the scheduler's global wall clock. The BSP PIT is the sole owner
/// of this clock for now, so AP-local timer interrupts must not call this.
pub fn scheduler_clock_tick() {
    SCHEDULER_TICKS.fetch_add(1, Ordering::Relaxed);
}

#[inline]
pub fn scheduler_clock_now() -> u64 {
    SCHEDULER_TICKS.load(Ordering::Relaxed) as u64
}

/// Account one scheduling quantum tick for the current CPU.
///
/// Both the BSP PIT and AP Local APIC timer may call this, but only when they
/// interrupted user mode. Kernel execution remains non-preemptive.
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


fn wake_sleeping_tasks_locked(scheduler: &mut Scheduler, now: u64) {
    for index in 0..scheduler.metadata.tasks.len() {
        if scheduler.metadata.tasks[index].status == TaskStatus::Sleeping
            && scheduler.metadata.tasks[index].wake_tick <= now
        {
            scheduler.metadata.tasks[index].status = TaskStatus::Ready;
            // Preserve the task's CPU affinity. The owning CPU will observe
            // the ready task from its local scheduler path.
            let cpu = if scheduler.metadata.tasks[index].pinned_cpu != usize::MAX {
                scheduler.metadata.tasks[index].pinned_cpu.min(crate::processor::MAX_AP_COUNT)
            } else {
                scheduler.metadata.tasks[index].cpu_affinity.min(crate::processor::MAX_AP_COUNT)
            };
            scheduler.run_queues[cpu].lock().push_back(index);
        }
    }
}

pub fn wait_task(task_id: usize) -> usize {
    let guard = SCHEDULER_LOCK.lock();
    let mut should_switch = false;
    let result = unsafe {
        let Some(scheduler) = SCHEDULER.as_mut() else {
            return usize::MAX;
        };
        let Some(target_index) = scheduler.metadata.tasks.iter().position(|task| task.id == task_id) else {
            return usize::MAX;
        };
        if scheduler.metadata.tasks[target_index].status == TaskStatus::Zombie {
            scheduler.metadata.tasks[target_index].exit_code
        } else {
            let percpu = crate::processor::get_percpu_data();
            if percpu.is_null() {
                return usize::MAX;
            }
            let current_index = (*percpu).current_task_index;
            if current_index == usize::MAX || current_index == target_index {
                return usize::MAX;
            }
            scheduler.metadata.tasks[current_index].status = TaskStatus::Waiting;
            scheduler.metadata.tasks[current_index].waiting_for = task_id;
            should_switch = true;
            0
        }
    };
    core::mem::drop(guard);
    if should_switch {
        switch_task();
        let guard = SCHEDULER_LOCK.lock();
        let exit_code = unsafe {
            SCHEDULER.as_ref()
                .and_then(|scheduler| scheduler.metadata.tasks.iter().find(|task| task.id == task_id))
                .map(|task| task.exit_code)
                .unwrap_or(usize::MAX)
        };
        core::mem::drop(guard);
        exit_code
    } else {
        result
    }
}

pub fn sleep_current(milliseconds: usize) {
    if milliseconds == 0 { switch_task(); return; }
    let ticks = ((milliseconds as u64).saturating_add(9) / 10).max(1);
    let deadline = (SCHEDULER_TICKS.load(Ordering::Relaxed) as u64).saturating_add(ticks);
    let guard = SCHEDULER_LOCK.lock();
    unsafe {
        if let Some(scheduler) = SCHEDULER.as_mut() {
            let percpu = crate::processor::get_percpu_data();
            if !percpu.is_null() {
                let current_index = (*percpu).current_task_index;
                if current_index != usize::MAX {
                    scheduler.metadata.tasks[current_index].wake_tick = deadline;
                    scheduler.metadata.tasks[current_index].status = TaskStatus::Sleeping;
                }
            }
        }
    }
    core::mem::drop(guard);
    switch_task();
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

/// Reclaim resources owned by zombie tasks that are no longer running on any CPU.
///
/// Task slots stay allocated so run-queue/current-task indices remain stable. This
/// first reaping step releases the per-task kernel stack, which is the largest
/// scheduler-owned allocation. User stacks belong to the shared userspace heap
/// and are left alone until address spaces/lifetimes are separated.
fn reap_zombies(scheduler: &mut Scheduler) {
    let current_indices: [usize; crate::processor::MAX_AP_COUNT + 1] =
        core::array::from_fn(published_current_task);

    for (index, task) in scheduler.metadata.tasks.iter_mut().enumerate() {
        if task.status != TaskStatus::Zombie || task.kernel_stack_bottom == 0 {
            continue;
        }

        // A terminating task switches away using its kernel stack. Do not free
        // that stack until no CPU advertises this slot as its current task.
        if current_indices.contains(&index) {
            continue;
        }

        unsafe {
            crate::memory::heap::free(task.kernel_stack_bottom as *mut u8);
        }
        task.kernel_stack_bottom = 0;
        task.kernel_stack_top = 0;
    }
}

pub fn terminate_task(exit_code: usize) {
    let guard = SCHEDULER_LOCK.lock();
    unsafe {
        if let Some(scheduler) = SCHEDULER.as_mut() {
            let percpu = crate::processor::get_percpu_data();
            if !percpu.is_null() {
                let current_index = (*percpu).current_task_index;
                if current_index != usize::MAX {
                    scheduler.metadata.tasks[current_index].status = TaskStatus::Zombie;
                    scheduler.metadata.tasks[current_index].exit_code = exit_code;
                    let terminated_id = scheduler.metadata.tasks[current_index].id;

                    // Wake tasks blocked in wait_task() for this task. Preserve
                    // the waiter's CPU affinity so its syscall can resume on the
                    // CPU whose kernel stack/context it already owns.
                    for index in 0..scheduler.metadata.tasks.len() {
                        if scheduler.metadata.tasks[index].status == TaskStatus::Waiting
                            && scheduler.metadata.tasks[index].waiting_for == terminated_id
                        {
                            scheduler.metadata.tasks[index].status = TaskStatus::Ready;
                            scheduler.metadata.tasks[index].waiting_for = usize::MAX;
                            let cpu = scheduler.metadata.tasks[index]
                                .cpu_affinity
                                .min(crate::processor::MAX_AP_COUNT);
                            scheduler.run_queues[cpu].lock().push_back(index);
                        }
                    }

                    crate::println!("Task {} terminated with exit code {}.", scheduler.metadata.tasks[current_index].id, exit_code);
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
                    return scheduler.metadata.tasks[current_index].id;
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
            for task in &scheduler.metadata.tasks {
                if task.id == task_id {
                    return match task.status {
                        TaskStatus::Ready => 0,
                        TaskStatus::Running => 1,
                        TaskStatus::Sleeping => 3,
                        TaskStatus::Waiting => 4,
                        TaskStatus::Zombie => 2,
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
            for task in &scheduler.metadata.tasks {
                if task.id == task_id {
                    return task.exit_code;
                }
            }
        }
        0
    }
}

pub fn run_ap_scheduler() -> ! {
    // APs consume their own run queue and use the LAPIC timer for user-mode
    // preemption. switch_task() saves this loop as the CPU's idle context.
    unsafe {
        core::arch::asm!("sti");
    }

    // AP startup happens before process::init() on the BSP. Do not enter the
    // scheduler/device-poll loop until the global scheduler has been published.
    // The acquire pairs with process::init()'s release store.
    while !SCHEDULER_READY.load(Ordering::Acquire) {
        core::hint::spin_loop();
    }

    // Do not use get_percpu_data() for this diagnostic: AP entry has already
    // established the per-CPU GS base, but this helper may return null in the
    // AP's current GS/swapgs state. switch_task() performs its own checked
    // lookup and is the path we actually need to validate.
    loop {
        switch_task();

        // Keep the existing AP-side NIC progress behavior while this PR is
        // being diagnosed. This avoids changing scheduler and network behavior
        // at the same time.
        unsafe {
            crate::drivers::net::poll();
        }

        // Sleep until a local timer/device interrupt or scheduler wake IPI arrives.
        // Queue producers kick an AP after placing runnable work on its queue.
        unsafe {
            core::arch::asm!("sti; hlt", options(nomem, nostack));
        }
    }
}
