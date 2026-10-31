#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

typedef struct { uint32_t *base; uint64_t stride, width, height; } Surface;
typedef struct {
    Surface surface;
    uint32_t format;
    uint64_t origin_x, origin_y, width, height, x, y;
} Console;
typedef struct { uint32_t character, foreground, background; } Cell;

extern void jihoo_console_init(Console *, Surface *, uint32_t);
extern void jihoo_console_viewport(Console *, uint64_t, uint64_t, uint64_t, uint64_t);
extern void jihoo_console_clear(Console *, uint32_t);
extern void jihoo_console_cell(Console *, uint64_t, uint64_t, uint32_t, uint32_t, uint32_t);
extern void jihoo_console_write(Console *, const void *, uint64_t);
extern void jihoo_console_region(Console *, uint64_t, uint64_t, const Cell *, uint64_t, uint64_t);
void kaguya_kernel_panic(const void *p, uint64_t n) {
    fwrite(p, 1, n, stderr);
    assert(!"unexpected jihoo panic");
    __builtin_trap();
}

static uint32_t memory[2 + 40 * 48];
static const uint32_t sentinel = 0xdeadbeef;
static void reset(void) { for (unsigned i = 0; i < sizeof(memory)/sizeof(*memory); i++) memory[i] = sentinel; }
static void guards(void) {
    assert(memory[0] == sentinel && memory[1 + 40*48] == sentinel);
    for (unsigned y = 0; y < 48; y++) for (unsigned x = 32; x < 40; x++)
        assert(memory[1 + y*40 + x] == sentinel);
}
int main(void) {
    Surface surface = { memory + 1, 40, 32, 48 };
    Console c;
    reset();
    jihoo_console_init(&c, &surface, 1);
    jihoo_console_clear(&c, 0x123456);
    for (unsigned y = 0; y < 48; y++) for (unsigned x = 0; x < 32; x++)
        assert(surface.base[y*40+x] == 0x123456);
    guards();
    // A: the same Basic Latin glyph as the Rust font8x8 reference.
    const uint8_t glyph_a[8] = {0x0c,0x1e,0x33,0x33,0x3f,0x33,0x33,0};
    jihoo_console_cell(&c, 1, 2, 'A', 0x12ab34, 0x567890);
    for (unsigned y = 0; y < 8; y++) for (unsigned x = 0; x < 8; x++)
        assert(surface.base[(y+16)*40+x+16] == ((glyph_a[y] >> x & 1) ? 0x12ab34 : 0x567890));
    assert(surface.base[24*40+16] == 0x123456); // preserve cell line spacing
    uint32_t saved[sizeof(memory)/sizeof(*memory)];
    memcpy(saved, memory, sizeof(memory));
    jihoo_console_cell(&c, UINT64_MAX, 0, 'A', 0, 0);
    jihoo_console_cell(&c, 0, UINT64_MAX, 'A', 0, 0);
    jihoo_console_cell(&c, 0, 0, 128, 0, 0);
    assert(memcmp(saved, memory, sizeof(memory)) == 0);
    jihoo_console_init(&c, &surface, 0);
    jihoo_console_clear(&c, 0x123456);
    assert(surface.base[0] == 0x563412);
    jihoo_console_cell(&c, 0, 0, 'A', 0x12ab34, 0x567890);
    assert(surface.base[0] == 0x907856 && surface.base[2] == 0x34ab12);
    jihoo_console_viewport(&c, 8, 16, UINT64_MAX, UINT64_MAX);
    assert(c.width == 24 && c.height == 32);
    jihoo_console_clear(&c, 0);
    assert(surface.base[0] == 0x907856 && surface.base[16*40+8] == 0);
    jihoo_console_viewport(&c, 32, 0, 1, 1); // invalid origin preserves viewport
    assert(c.origin_x == 8 && c.width == 24);
    jihoo_console_write(&c, "AAA", 3);
    assert(c.x == 24 && c.y == 0);
    jihoo_console_write(&c, "B\bC\nD", 5);
    assert(c.x == 8 && c.y == 0); // wrap, backspace, then clear at bottom
    jihoo_console_write(&c, "\b\b", 2);
    assert(c.x == 0);
    jihoo_console_write(&c, "\xc3\xa9", 2);
    assert(c.x == 0);
    Cell cells[] = {{'A',0xffffff,0}, {'B',0xffffff,0}, {'C',0xffffff,0}};
    jihoo_console_region(&c, 0, 0, cells, 3, 2);
    memcpy(saved, memory, sizeof(memory));
    jihoo_console_region(&c, 0, 0, cells, 3, 0);
    jihoo_console_region(&c, UINT64_MAX, UINT64_MAX, cells, 3, 1);
    assert(memcmp(saved, memory, sizeof(memory)) == 0);
    jihoo_console_viewport(&c, 31, 47, 10, 10);
    jihoo_console_write(&c, "A", 1);
    assert(c.width == 1 && c.height == 1 && c.x == 0);
    guards();
    puts("jihoo console: glyphs, formats, clipping, viewport, wrap, cells and guards passed");
}
