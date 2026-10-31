#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
struct Trb { uint64_t parameter; uint32_t status, control; };
extern void jihoo_usb_ring_init(void *, uint64_t);
extern uint64_t jihoo_usb_enqueue(uint64_t, uint32_t, uint32_t);
extern bool jihoo_usb_contains(uint64_t);
extern uint32_t jihoo_usb_neutral(uint32_t);
extern void jihoo_usb_report(const uint8_t *);
extern int32_t jihoo_usb_key(void);
extern uint8_t jihoo_usb_ascii(uint8_t, uint8_t);
static _Alignas(4096) struct Trb trbs[256];
static unsigned fences;
void jihoo_dma_fence(void) { __atomic_thread_fence(__ATOMIC_SEQ_CST); fences++; }
void kaguya_kernel_panic(const void *p, uint64_t n) { fwrite(p, 1, n, stderr); abort(); }
void kaguya_runtime_free(void *p) { free(p); }
static void report(uint8_t modifier, uint8_t key) {
    uint8_t bytes[8] = {modifier,0,key,0,0,0,0,0}; jihoo_usb_report(bytes);
}
int main(void) {
    jihoo_usb_ring_init(trbs, sizeof(trbs));
    assert(trbs[255].parameter == (uintptr_t)trbs && trbs[255].control == ((6<<10)|2));
    for (unsigned i = 0; i < 1020; i++) {
        unsigned index = i % 255, cycle = ((i / 255) & 1) ^ 1;
        uint64_t address = jihoo_usb_enqueue(i+100, 8, (1<<10)|1);
        assert(address == (uintptr_t)&trbs[index] && trbs[index].parameter == i+100 && trbs[index].status == 8);
        assert(trbs[index].control == ((1<<10)|cycle));
        if (index == 254) assert(trbs[255].control == ((6<<10)|2|cycle));
    }
    assert(fences == 1020 && jihoo_usb_contains((uintptr_t)trbs));
    assert(!jihoo_usb_contains((uintptr_t)trbs-1) && !jihoo_usb_contains((uintptr_t)&trbs[255]));
    assert(jihoo_usb_neutral(0x20603) & (1<<9));
    assert((jihoo_usb_neutral(UINT32_MAX) & ((1u<<31)|(1<<16)|(127<<17)|(1<<4)|2)) == 0);
    for (uint8_t key = 4; key <= 29; key++) {
        assert(jihoo_usb_ascii(key, 0) == 'a'+key-4);
        assert(jihoo_usb_ascii(key, 2) == 'A'+key-4 && jihoo_usb_ascii(key, 32) == 'A'+key-4);
    }
    assert(jihoo_usb_ascii(42, 0) == 8 && jihoo_usb_ascii(41, 0) == 27);
    assert(jihoo_usb_ascii(52, 0) == '\'' && jihoo_usb_ascii(52, 2) == '"');
    assert(jihoo_usb_ascii(49, 0) == '\\' && jihoo_usb_ascii(56, 2) == '?');
    assert(jihoo_usb_ascii(50, 0) == 0 && jihoo_usb_ascii(255, 0) == 0);
    report(0, 4); assert(jihoo_usb_key() == 'a' && jihoo_usb_key() == -1);
    report(0, 4); assert(jihoo_usb_key() == -1);
    report(0, 0); report(2, 4); assert(jihoo_usb_key() == 'A');
    report(0, 1); assert(jihoo_usb_key() == -1);
    report(0, 0);
    for (unsigned i = 0; i < 300; i++) { report(0, 5); report(0, 0); }
    for (unsigned i = 0; i < 255; i++) assert(jihoo_usb_key() == 'b');
    assert(jihoo_usb_key() == -1);
    for (unsigned i = 0; i < 300; i++) {
        report(0, 6); report(0, 0); assert(jihoo_usb_key() == 'c');
    }
    puts("jihoo USB helpers: DMA ring cycles, PORTSC power, HID shifts/rollover, queue capacity and wrap passed");
}
