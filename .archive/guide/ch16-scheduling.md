# Chapter 16: Cooperative Scheduling

The final piece: multitasking. This chapter covers how kaguyaOS switches between tasks — saving and restoring CPU state — and how cooperative yielding enables user programs to run concurrently.

## Task Model

Each task represents a unit of execution:

```rust
// src/process/mod.rs:14-24
pub struct Task {
    pub id: usize,
    pub stack_top: u64,          // Points to saved context on kernel stack
    pub stack_bottom: u64,       // Kernel stack base
    pub kernel_stack_bottom: u64,
    pub user_rsp: u64,           // User-mode RSP
    pub gs_base: u64,            // Per-CPU GS base
    pub exit_code: i32,
    pub status: TaskStatus,
}

pub enum TaskStatus {
    Ready,
    Running,
    Terminated,
}
```

Tasks are stored in a `Vec<Task>` behind a spinlock:

```rust
static mut SCHEDULER: Option<Scheduler> = None;
static SCHEDULER_LOCK: Spinlock<()> = Spinlock::new(());
```

## Task 0: The Kernel Idle Task

The BSP (Bootstrap Processor) starts as Task 0. It's not a user task — it runs the kernel scheduler loop:

```rust
// src/process/mod.rs:36-60
pub fn init() {
    let mut scheduler = Scheduler { tasks: Vec::new() };

    // Task 0: kernel idle (BSP)
    scheduler.tasks.push(Task {
        id: 0,
        stack_top: 0,
        stack_bottom: 0,
        status: TaskStatus::Running,
        // ...
    });

    SCHEDULER = Some(scheduler);
}
```

## Context Switching

The core of the scheduler is `switch_task()`. It saves the current task's registers and restores the next task's:

```rust
// src/process/mod.rs (simplified)
pub fn switch_task() {
    // 1. Acquire scheduler lock
    let _lock = SCHEDULER_LOCK.lock();

    // 2. Find current task (the one that was Running)
    let current = scheduler.tasks.iter()
        .position(|t| t.status == TaskStatus::Running)
        .unwrap_or(0);

    // 3. Find next Ready task (round-robin)
    let next = scheduler.tasks.iter()
        .enumerate()
        .skip(current + 1)
        .chain(scheduler.tasks.iter().enumerate().take(current + 1))
        .find(|(_, t)| t.status == TaskStatus::Ready)
        .map(|(i, _)| i);

    if let Some(next_idx) = next {
        // 4. Mark states
        scheduler.tasks[current].status = TaskStatus::Ready;
        scheduler.tasks[next_idx].status = TaskStatus::Running;

        // 5. Switch GS base (for per-CPU data)
        // ... swapgs in context_switch ...

        // 6. Perform the actual register save/restore
        context_switch(
            &mut scheduler.tasks[current].stack_top as *mut u64,
            scheduler.tasks[next_idx].stack_top,
        );
    }
}
```

## The Assembly: context_switch

The actual register save/restore is in assembly:

```rust
// src/process/mod.rs (extern "sysv64" asm, simplified)
unsafe extern "sysv64" {
    fn context_switch(old_sp: *mut u64, new_sp: u64);
}
```

The assembly (inline or in a `.S` file):
```asm
context_switch:
    ; Save current registers on current stack
    push rbp
    push rbx
    push r12-r15

    ; Save current RSP into *old_sp
    mov [rdi], rsp

    ; Load new RSP from new_sp
    mov rsp, rsi

    ; Restore new task's registers
    pop r15-r12
    pop rbx
    pop rbp
    ret
```

On x86_64, the callee-saved registers (RBX, RBP, R12-R15) are preserved across function calls. By saving/restoring them, we can resume a task exactly where it left off.

## The BSP Scheduler Loop

The BSP runs a simple loop that never terminates:

```rust
// src/main.rs:481-489
loop {
    scheduler::switch_task();
    // Re-enable interrupts (switch_task may return with IF=0)
    core::arch::asm!("sti");
    core::arch::asm!("hlt");
}
```

The `hlt` instruction puts the CPU to sleep until the next interrupt (timer IRQ 0 at 100 Hz). This saves power and prevents busy-looping.

## Cooperative Yielding

In kaguyaOS, preemption via timer IRQ is limited (the shell is usually in kernel mode during syscalls, so `cs & 3 == 3` never triggers). The actual mechanism for multitasking is **cooperative yielding**:

```rust
// User program calls yield:
pub fn yield_task() {
    unsafe { syscall0(4); }  // Syscall 4
}

// Kernel handler:
fn sys_yield() -> isize {
    // Mark current task as Ready, switch to next
    scheduler::switch_task();
    0
}
```

The shell yields after every key poll:
```rust
// user/src/init.rs
loop {
    let key = std::read_key();
    if key != 0 {
        handle_key(key);
    }
    std::yield_task();  // Give other tasks a chance
}
```

This means:
- The shell runs, processes input, then yields
- A child process (e.g., `ping.kef`) runs, sends/receives packets, yields
- The shell runs again
- They appear to run "at the same time"

## Task Termination

When a user program finishes (or crashes), the kernel removes it:

```rust
// src/process/mod.rs
pub fn terminate_task(exit_code: i32) {
    let current_id = current_task_id();
    let mut scheduler = SCHEDULER.lock();

    if let Some(task) = scheduler.tasks.iter_mut().find(|t| t.id == current_id) {
        task.status = TaskStatus::Terminated;
        task.exit_code = exit_code;
        // Free kernel stack...
    }

    // Force switch to next task
    switch_task();
}
```

## The exec Syscall

The `exec` syscall creates a new user task:

1. Read the KEF file from FAT16
2. Call `load_kef()` to map code and stack
3. Call `add_new_user_task()` to create the task
4. Return to the caller (the shell continues)

```rust
// src/process/mod.rs:62-100
pub fn add_new_user_task(entry: u64, user_rsp: u64, stack_size: usize, ...) {
    // Allocate kernel stack
    let kernel_stack_bottom = crate::memory::heap::alloc(stack_size) as u64;

    // Build IRETQ frame (for entering Ring 3)
    // Build context switch frame (for the scheduler)
    // Push task to scheduler
}
```

## Summary: The Complete Task Lifecycle

```
                    ┌─────────────────┐
                    │   kernel_main   │
                    └────────┬────────┘
                             │ load init.kef
                             ▼
                    ┌─────────────────┐
                    │ Task 1: init.rs │  First user task
                    │   (shell)       │
                    └────────┬────────┘
                             │ exec ping.kef
                             ▼
              ┌──────────────┴──────────────┐
              │                             │
    ┌─────────┴─────────┐       ┌──────────┴──────────┐
    │ Task 1: shell     │       │ Task 2: ping.kef    │
    │ yield_task()      │◄─────►│ yield_task()         │
    │ read_key()        │       │ send/receive pings   │
    │ exec commands     │       │ print RTT stats      │
    └───────────────────┘       └──────────────────────┘

    Both tasks alternate via cooperative yields.
    Timer IRQ fires at 100 Hz but doesn't force switches
    (shell is in kernel mode during syscalls).
```

## kaguyaOS Reference

| File | Lines | What it does |
|------|-------|-------------|
| `src/process/mod.rs` | 1-413 | Scheduler, task management, context switch |
| `src/main.rs` | 481-489 | BSP scheduler loop |
| `user/src/init.rs` | 210-215 | Shell cooperative yield |

---

## What's Next?

Congratulations — you've built (or at least understand how to build) a complete operating system:

- UEFI boot → bare-metal kernel
- Framebuffer console and println!
- GDT/TSS for Ring 0/3, IDT for interrupts
- Physical memory management and paging
- A kernel heap allocator
- Syscall/ sysret for user↔kernel transitions
- A custom executable format and loader
- An interactive shell
- Device drivers (NVMe, USB, Ethernet)
- A FAT16 filesystem
- Network stack (ARP, ICMP)
- Multi-core support
- Cooperative multitasking

**Where to go from here:**
- Preemptive scheduling (properly route timer IRQs to user-mode tasks)
- Virtual memory per-process (separate page tables for each task)
- ELF loader (instead of the custom KEF format)
- More filesystem features (subdirectories, file permissions)
- TCP/IP stack (for real network connectivity)
- A window manager (for graphical applications)
- Dynamic linking
- An interpreter/shell scripting language

The OS world is vast. This project gives you the foundation — everything else is iteration.
