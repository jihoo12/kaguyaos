#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>

extern void jihoo_task_init(void), jihoo_task_schedule(void);
extern uint64_t jihoo_task_add(uint64_t, uint64_t, uint64_t);
extern uint64_t jihoo_task_status(uint64_t), jihoo_task_code(uint64_t), jihoo_task_wait(uint64_t);
extern void jihoo_task_exit(uint64_t) __attribute__((noreturn));
extern void jihoo_task_sleep(uint64_t);
extern uint64_t kaguya_event_generation(uint64_t);
extern void kaguya_wait_event(uint64_t, uint64_t, uint64_t), kaguya_wake_event(uint64_t), kaguya_forget_event(uint64_t);
extern void kaguya_scheduler_clock_tick(void), kaguya_scheduler_interrupt(bool);

struct PerCpu {
    uint64_t kernel_stack, user_stack, scratch;
    uint8_t apic_id, cpu_index;
    uint64_t current_task, idle_stack;
    uint32_t ticks_left;
    bool need_reschedule;
};
static struct PerCpu cpu;
static bool enabled;
static uint64_t allocations, freed, steps, child, waiter, sleeper, waker;
static void *stacks[8];
static unsigned stack_count, retired;
uint64_t jihoo_irq_save(void) { bool previous = enabled; enabled = false; return previous ? 0x200 : 0; }
void jihoo_irq_enable(void) { assert(!enabled); enabled = true; }
void jihoo_pause(void) { __asm__ volatile("pause"); }
void kaguya_kernel_panic(const void *p, uint64_t n) { fwrite(p, 1, n, stderr); abort(); }
void *kaguya_runtime_alloc(uint64_t size, uint64_t alignment) {
    assert(alignment <= 16);
    void *p = malloc(size ? size : 1);
    assert(p); allocations++; return p;
}
void *kaguya_runtime_realloc(void *p, uint64_t size, uint64_t alignment) {
    if (!p) return kaguya_runtime_alloc(size, alignment);
    p = realloc(p, size ? size : 1); assert(p); return p;
}
void kaguya_runtime_free(void *p) {
    assert(p && allocations);
    for (unsigned i = 0; i < stack_count; i++) {
        if (stacks[i] == p) { stacks[i] = NULL; retired++; break; }
    }
    allocations--; freed++; free(p);
}
struct PerCpu *kaguya_cpu_state(void) { return &cpu; }
uint64_t kaguya_online_cpus(void) { return 1; }
uint64_t kaguya_kernel_root(void) { return 0x1000; }
void kaguya_kick_cpu(uint64_t index) { (void)index; abort(); }
uint64_t jihoo_read_msr(uint32_t msr) { assert(msr == 0xc0000102); return 0; }
void kaguya_prepare_switch(uint64_t index, uint64_t stack, uint64_t user_stack, uint64_t gs, uint64_t root) {
    assert(!enabled && index == 0 && root == 0x1000 && !gs && !user_stack);
    if (stack) cpu.kernel_stack = stack;
    cpu.user_stack = user_stack;
}
static void returned(void) { jihoo_task_exit(0); }
uint64_t kaguya_kernel_return_address(void) { return (uintptr_t)returned; }
uint64_t kaguya_user_trampoline_address(void) { abort(); }
uint64_t kaguya_alloc_frame(void) { abort(); }
void kaguya_free_frame(uint64_t frame) { (void)frame; abort(); }
int64_t kaguya_console_read(void *out, uint64_t bytes) { (void)out; (void)bytes; abort(); }
int64_t kaguya_console_write(const void *in, uint64_t bytes) { (void)in; (void)bytes; abort(); }
static uint64_t add(void (*entry)(void)) {
    void *stack = kaguya_runtime_alloc(65536, 16);
    assert(stack_count < 8); stacks[stack_count++] = stack;
    uint64_t id = jihoo_task_add((uintptr_t)entry, (uintptr_t)stack, 65536);
    assert(id != UINT64_MAX); return id;
}
static void cooperative(void) {
    for (unsigned i = 0; i < 30; i++) { steps++; jihoo_task_schedule(); }
    jihoo_task_exit(17);
}
static void waiting(void) {
    assert(jihoo_task_wait(child) == 17);
    assert(jihoo_task_wait(UINT64_MAX) == UINT64_MAX);
    assert(jihoo_task_wait(waiter) == UINT64_MAX);
    steps++;
}
static void sleeping(void) {
    jihoo_task_sleep(21);
    steps++;
}
static void waking(void) {
    assert(jihoo_task_status(sleeper) == 3);
    kaguya_scheduler_clock_tick(); kaguya_scheduler_clock_tick();
    jihoo_task_schedule();
    assert(jihoo_task_status(sleeper) == 3);
    kaguya_scheduler_clock_tick(); jihoo_task_schedule();
    jihoo_task_exit(23);
}
static void event_waiter(void) {
    uint64_t seen = kaguya_event_generation(0x1234);
    kaguya_wake_event(0x1234);
    kaguya_wait_event(0x1234, 0, seen); // wake before sleep must not block
    seen = kaguya_event_generation(0x1234);
    kaguya_wait_event(0x1234, 0, seen);
    assert(jihoo_task_status(waker) == 2);
    kaguya_forget_event(0x1234);
    steps++;
}
static void event_waker(void) {
    assert(jihoo_task_status(waiter) == 4);
    kaguya_wake_event(0x1234);
}
static void preempted(void) {
    for (unsigned i = 0; i < 30; i++) {
        steps++;
        for (unsigned tick = 0; tick < 5; tick++) kaguya_scheduler_interrupt(true);
    }
}
static void drain(uint64_t first, uint64_t second) {
    for (unsigned tries = 0; tries < 1000 && (jihoo_task_status(first) != 2 || jihoo_task_status(second) != 2); tries++)
        jihoo_task_schedule();
    assert(jihoo_task_status(first) == 2 && jihoo_task_status(second) == 2);
    jihoo_task_schedule(); // consume final retirement
    assert(cpu.current_task == UINT64_MAX && !enabled);
}
int main(void) {
    alarm(20);
    jihoo_task_init();
    child = add(cooperative); waiter = add(waiting);
    drain(child, waiter);
    assert(steps == 31 && jihoo_task_code(child) == 17);
    sleeper = add(sleeping); waker = add(waking);
    drain(sleeper, waker);
    assert(steps == 32 && jihoo_task_code(waker) == 23);
    waiter = add(event_waiter); waker = add(event_waker);
    drain(waiter, waker);
    assert(steps == 33);
    child = add(preempted); waiter = add(preempted);
    drain(child, waiter);
    assert(steps == 93 && freed >= 8 && retired == 8);
    puts("jihoo scheduler: real stack switches, waits, sleep, generations, timer quanta and retirement passed");
}
