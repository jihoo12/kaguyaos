#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

extern void jihoo_space_ownership_check(uint64_t);
extern void jihoo_kef_loading_check(uint64_t);
static void *frames[256];
static uint64_t frame_count, heap_count;
static int64_t budget = -1;
static bool fail_heap;
uint64_t jihoo_irq_save(void) { return 0; }
void jihoo_irq_enable(void) { assert(!"unexpected interrupt enable"); }
void jihoo_pause(void) {}
void kaguya_kernel_panic(const void *p, uint64_t n) { fwrite(p, 1, n, stderr); abort(); }
uint64_t frame_live(void) { return frame_count; }
uint64_t heap_live(void) { return heap_count; }
void frame_budget(int64_t n) { budget = n; }
void heap_fail_next(void) { fail_heap = true; }
uint64_t kaguya_alloc_frame(void) {
    if (budget == 0) return 0;
    if (budget > 0) budget--;
    for (unsigned i = 0; i < 256; i++) if (!frames[i]) {
        void *p = aligned_alloc(4096, 4096);
        assert(p);
        memset(p, 0, 4096);
        frames[i] = p;
        frame_count++;
        return (uintptr_t)p;
    }
    assert(!"test frame table exhausted");
    return 0;
}
void kaguya_free_frame(uint64_t frame) {
    for (unsigned i = 0; i < 256; i++) if (frames[i] == (void *)(uintptr_t)frame) {
        free(frames[i]);
        frames[i] = NULL;
        frame_count--;
        return;
    }
    assert(!"unknown or double freed frame");
}
void *kaguya_runtime_alloc(uint64_t size, uint64_t alignment) {
    assert(alignment <= 16);
    if (fail_heap) { fail_heap = false; return NULL; }
    void *p = malloc(size);
    assert(p);
    heap_count++;
    return p;
}
void kaguya_runtime_free(void *p) { assert(p && heap_count); heap_count--; free(p); }
static uint64_t leaf(uint64_t root, uint64_t address) {
    uint64_t *table = (void *)(uintptr_t)root;
    const unsigned shifts[] = {39,30,21,12};
    for (unsigned i = 0; i < 4; i++) {
        uint64_t entry = table[address >> shifts[i] & 511];
        if (!(entry & 1)) return 0;
        if (i == 3) return entry;
        table = (void *)(uintptr_t)(entry & UINT64_C(0x000ffffffffff000));
    }
    abort();
}
uint64_t physical_of(uint64_t root, uint64_t address) {
    uint64_t entry = leaf(root, address);
    return entry ? (entry & UINT64_C(0x000ffffffffff000)) + (address & 4095) : 0;
}
uint64_t flags_of(uint64_t root, uint64_t address) { return leaf(root, address) & ~UINT64_C(0x000ffffffffff000); }
int main(void) {
    uint64_t *kernel = aligned_alloc(4096, 4096);
    assert(kernel);
    memset(kernel, 0, 4096);
    kernel[0] = 0x12345003;
    for (unsigned i = 0; i < 20; i++) {
        jihoo_space_ownership_check((uintptr_t)kernel);
        jihoo_kef_loading_check((uintptr_t)kernel);
        assert(frame_count == 0 && heap_count == 0);
        assert(kernel[0] == 0x12345003);
    }
    free(kernel);
    puts("jihoo ownership/KEF: clones, last-owner cleanup, OOM, mappings, BSS, args and guards passed");
}
