#include <assert.h>
#include <stdint.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
extern void jihoo_udp_check(void);
static uint64_t allocations,ticks;
static bool enabled=true;
void *kaguya_runtime_alloc(uint64_t n,uint64_t alignment) { assert(alignment<=16);void*p=malloc(n?n:1);assert(p);allocations++;return p; }
void *kaguya_runtime_realloc(void*p,uint64_t n,uint64_t alignment) { if(!p)return kaguya_runtime_alloc(n,alignment);p=realloc(p,n?n:1);assert(p);return p; }
void kaguya_runtime_free(void*p) { assert(p&&allocations);allocations--;free(p); }
void kaguya_kernel_panic(const void*p,uint64_t n) { fwrite(p,1,n,stderr);abort(); }
uint64_t jihoo_irq_save(void) { bool before=enabled;enabled=false;return before?0x200:0; }
void jihoo_irq_enable(void) { assert(!enabled);enabled=true; }
void jihoo_pause(void) { __asm__ volatile("pause"); }
void jihoo_dma_fence(void) { __atomic_thread_fence(__ATOMIC_SEQ_CST); }
uint64_t kaguya_network_clock(void) { return ticks; }
uint64_t kaguya_event_generation(uint64_t key) { assert(key);return 0; }
void kaguya_wait_event(uint64_t key,uint64_t end,uint64_t generation) { assert(key&&end>ticks&&generation==0);ticks=end; }
void kaguya_wake_event(uint64_t key) { assert(key&&enabled); }
void kaguya_forget_event(uint64_t key) { assert(key&&enabled); }
int main(void) {
    jihoo_udp_check();assert(enabled);uint64_t baseline=allocations;
    for(unsigned i=0;i<100;i++){jihoo_udp_check();assert(enabled&&allocations==baseline);}
    puts("jihoo UDP: port conflicts, peer filtering, bounded queues, timeout, zero-capacity receive, alias lifetime and port reuse passed");
}
