#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <pthread.h>
#include <stdatomic.h>
#include <stdio.h>
#include <stdlib.h>

extern void jihoo_shared_check(void);
extern void *jihoo_shared_counter_new(void);
extern void jihoo_shared_counter_add(void *);
extern uint64_t jihoo_shared_counter_finish(void *);
extern void *jihoo_shared_weak_new(void);
extern bool jihoo_shared_weak_upgrade(void *);
extern void jihoo_shared_weak_release(void *), jihoo_shared_weak_free(void *);
static _Atomic uint64_t allocations, drops;
static _Thread_local bool enabled = true;
bool irq_enabled(void) { return enabled; }
uint64_t jihoo_irq_save(void) { bool previous = enabled; enabled = false; return previous ? 0x200 : 0; }
void jihoo_irq_enable(void) { assert(!enabled); enabled = true; }
void jihoo_pause(void) { __asm__ volatile("pause"); }
void kaguya_kernel_panic(const void *p, uint64_t n) { fwrite(p, 1, n, stderr); abort(); }
void record_drop(uint64_t value) { assert(value == 42); atomic_fetch_add(&drops, 1); }
void *kaguya_runtime_alloc(uint64_t size, uint64_t alignment) {
    assert(alignment <= 16);
    void *p = malloc(size ? size : 1);
    assert(p);
    atomic_fetch_add(&allocations, 1);
    return p;
}
void *kaguya_runtime_realloc(void *p, uint64_t size, uint64_t alignment) {
    assert(alignment <= 16);
    if (!p) return kaguya_runtime_alloc(size, alignment);
    p = realloc(p, size ? size : 1);
    assert(p);
    return p;
}
void kaguya_runtime_free(void *p) {
    assert(p && atomic_load(&allocations) > 0);
    atomic_fetch_sub(&allocations, 1);
    free(p);
}
static void *increment(void *p) {
    for (unsigned i = 0; i < 10000; i++) {
        jihoo_shared_counter_add(p);
        assert(enabled);
    }
    return NULL;
}
static void *upgrade(void *p) {
    for (unsigned i=0;i<100000;i++) jihoo_shared_weak_upgrade(p);
    return NULL;
}
int main(void) {
    jihoo_shared_check();
    assert(enabled && atomic_load(&drops) == 1 && atomic_load(&allocations) == 0);
    void *owner = jihoo_shared_counter_new();
    pthread_t threads[4];
    for (unsigned i = 0; i < 4; i++) assert(pthread_create(&threads[i], NULL, increment, owner) == 0);
    for (unsigned i = 0; i < 4; i++) assert(pthread_join(threads[i], NULL) == 0);
    assert(jihoo_shared_counter_finish(owner) == 40000);
    assert(enabled && atomic_load(&allocations) == 0);
    void *weak = jihoo_shared_weak_new();
    assert(jihoo_shared_weak_upgrade(weak));
    for (unsigned i=0;i<4;i++) assert(pthread_create(&threads[i],NULL,upgrade,weak)==0);
    jihoo_shared_weak_release(weak);
    for (unsigned i=0;i<4;i++) assert(pthread_join(threads[i],NULL)==0);
    assert(!jihoo_shared_weak_upgrade(weak) && atomic_load(&drops)==2);
    assert(atomic_load(&allocations)==2); // weak header plus test handle
    jihoo_shared_weak_free(weak);
    assert(atomic_load(&allocations)==0);
    puts("jihoo shared/guards: last-owner Drop, nested interrupt restoration and four-thread counter passed");
}
