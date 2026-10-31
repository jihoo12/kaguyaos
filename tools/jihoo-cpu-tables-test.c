#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

extern void jihoo_table_pointer(uint8_t *, uint64_t, uint16_t);
extern void jihoo_tss_stack(uint8_t *, uint64_t);
extern void jihoo_gdt_build(uint8_t *, uint8_t *, uint64_t);
extern void jihoo_idt_gate(uint8_t *, uint64_t, uint8_t);
void kaguya_kernel_panic(const void *p, uint64_t n) { fwrite(p, 1, n, stderr); abort(); }
static uint64_t read_le(const uint8_t *p, unsigned n) {
    uint64_t result = 0;
    for (unsigned i = 0; i < n; i++) result |= (uint64_t)p[i] << (i*8);
    return result;
}
int main(void) {
    uint8_t gdt[58], tss[106], pointer[12], gate[18];
    memset(gdt, 0xa5, sizeof(gdt));
    memset(tss, 0xa5, sizeof(tss));
    // Deliberately unaligned buffers verify packed x86 table layouts.
    jihoo_gdt_build(gdt + 1, tss + 1, UINT64_C(0xffffabcd12345670));
    assert(gdt[0] == 0xa5 && gdt[57] == 0xa5 && tss[0] == 0xa5 && tss[105] == 0xa5);
    const uint64_t segments[] = {0, 0x00af9a000000ffff, 0x00cf92000000ffff,
                                  0x00cff2000000ffff, 0x00affa000000ffff};
    for (unsigned i = 0; i < 5; i++) assert(read_le(gdt + 1 + 8*i, 8) == segments[i]);
    uint8_t *d = gdt + 1 + 40;
    uint64_t base = read_le(d+2, 3) | read_le(d+7, 1) << 24 | read_le(d+8, 4) << 32;
    assert(base == (uintptr_t)(tss+1) && read_le(d, 2) == 103 && d[5] == 0x89 && d[6] == 0);
    assert(read_le(d+12, 4) == 0);
    for (unsigned i = 0; i < 104; i++)
        if (!(i >= 36 && i < 44) && i != 102) assert(tss[1+i] == 0);
    assert(read_le(tss+1+36, 8) == UINT64_C(0xffffabcd12345670));
    assert(read_le(tss+1+102, 2) == 104);
    jihoo_tss_stack(tss+1, UINT64_C(0x123456789abcdef0));
    assert(read_le(tss+1+4, 8) == UINT64_C(0x123456789abcdef0));
    assert(read_le(tss+1+36, 8) == UINT64_C(0xffffabcd12345670));
    memset(pointer, 0xa5, sizeof(pointer));
    jihoo_table_pointer(pointer+1, UINT64_C(0xfedcba9876543210), 4095);
    assert(read_le(pointer+1,2) == 4095 && read_le(pointer+3,8) == UINT64_C(0xfedcba9876543210));
    assert(pointer[0] == 0xa5 && pointer[11] == 0xa5);
    const uint64_t handlers[] = {0, UINT64_C(0x12345678), UINT64_C(0xffffabcd12345678), UINT64_MAX};
    for (unsigned h = 0; h < 4; h++) for (unsigned ist = 0; ist < 256; ist++) {
        memset(gate, 0xa5, sizeof(gate));
        jihoo_idt_gate(gate+1, handlers[h], ist);
        d = gate+1;
        assert((read_le(d, 2) | read_le(d+6, 2) << 16 | read_le(d+8,4) << 32) == handlers[h]);
        assert(read_le(d+2,2) == 8 && d[4] == (ist & 7) && d[5] == 0x8e && read_le(d+12,4) == 0);
        assert(gate[0] == 0xa5 && gate[17] == 0xa5);
    }
    puts("jihoo CPU tables: packed GDT/TSS/IDT bytes, full-width addresses and guards passed");
}
