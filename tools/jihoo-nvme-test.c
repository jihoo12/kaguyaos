#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
extern void jihoo_nvme_queue(uint64_t, uint64_t, uint64_t, uint64_t, uint64_t, uint32_t);
extern void jihoo_nvme_submit(const uint32_t *);
extern int32_t jihoo_nvme_poll(uint16_t);
extern bool jihoo_nvme_prps(uint64_t, uint32_t, uint64_t *, uint32_t *);
static _Alignas(4096) uint64_t list[512];
static uint32_t sq[32], cq[8], doorbells[32], command[16];
void jihoo_dma_fence(void) { __atomic_thread_fence(__ATOMIC_SEQ_CST); }
void kaguya_runtime_free(void *p) { free(p); }
void kaguya_kernel_panic(const void *p, uint64_t n) { fwrite(p, 1, n, stderr); abort(); }
static uint64_t get64(unsigned i) { return command[i] | ((uint64_t)command[i+1]<<32); }
int main(void) {
    jihoo_nvme_queue((uintptr_t)sq, (uintptr_t)cq, (uintptr_t)doorbells, 16, 1, 2);
    for (unsigned i = 0; i < 16; i++) command[i] = i + 100;
    jihoo_nvme_submit(command);
    assert(memcmp(sq, command, 64) == 0 && doorbells[8] == 1);
    jihoo_nvme_submit(command); assert(doorbells[8] == 0);
    assert(jihoo_nvme_poll(100) == -1 && doorbells[12] == 0);
    cq[3] = (1<<16)|99; assert(jihoo_nvme_poll(100) == -1 && doorbells[12] == 1);
    cq[7] = (1<<16)|100; assert(jihoo_nvme_poll(100) == 0 && doorbells[12] == 0);
    cq[3] = (7<<17)|100; assert(jihoo_nvme_poll(100) == 7 && doorbells[12] == 1);
    assert(!jihoo_nvme_prps(0, 1, list, command));
    assert(!jihoo_nvme_prps(4096, 0, list, command));
    assert(!jihoo_nvme_prps(UINT64_MAX-511, 1, list, command));
    assert(jihoo_nvme_prps(0x1080, 1, list, command) && get64(6) == 0x1080 && get64(8) == 0);
    assert(command[0] == ((100<<16)|2) && command[1] == 1);
    assert(jihoo_nvme_prps(0x1f00, 1, list, command) && get64(8) == 0x2000);
    assert(jihoo_nvme_prps(0x1080, 24, list, command) && get64(8) == (uintptr_t)list);
    assert(list[0] == 0x2000 && list[1] == 0x3000 && list[2] == 0x4000);
    assert(jihoo_nvme_prps(0x1000, 513*8, list, command) && list[511] == 0x201000);
    assert(!jihoo_nvme_prps(0x1080, 513*8, list, command));
    assert(!jihoo_nvme_prps(0x1000, 24, list+1, command));
    assert(!jihoo_nvme_prps(0x1000, 65537, list, command));
    puts("jihoo NVMe: submission layout, doorbell stride, completion phases/status, PRP boundaries and overflow passed");
}
