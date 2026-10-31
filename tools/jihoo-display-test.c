#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
extern uint32_t *jihoo_display_init(uint64_t, uint64_t, uint64_t, uint64_t, uint32_t);
extern void jihoo_display_mark(uint64_t, uint64_t, uint64_t, uint64_t);
extern void jihoo_display_present(void), jihoo_display_desktop(uint64_t *);
static uint64_t allocations;
uint32_t jihoo_inl(uint16_t port) { (void)port; return UINT32_MAX; }
void jihoo_outl(uint16_t port, uint32_t value) { (void)port; (void)value; }
void jihoo_pause(void) { abort(); }
void jihoo_dma_fence(void) { __atomic_thread_fence(__ATOMIC_SEQ_CST); }
bool kaguya_map_mmio(uint64_t p, uint64_t n) { (void)p; (void)n; abort(); }
void kaguya_graphics_log(const void *p, uint64_t n) { (void)p; (void)n; }
void kaguya_kernel_panic(const void *p, uint64_t n) { fwrite(p, 1, n, stderr); abort(); }
void *kaguya_runtime_alloc(uint64_t n, uint64_t alignment) { assert(alignment<=16); void *p=malloc(n?n:1); assert(p); allocations++; return p; }
void *kaguya_runtime_realloc(void *p, uint64_t n, uint64_t alignment) { if (!p) return kaguya_runtime_alloc(n,alignment); p=realloc(p,n?n:1); assert(p); return p; }
void kaguya_runtime_free(void *p) { assert(p && allocations); allocations--; free(p); }
static uint32_t gop[2+40*48];
static void guards(void) {
    assert(gop[0]==0xdeadbeef && gop[1+40*48]==0xdeadbeef);
    for (unsigned y=0;y<48;y++) for (unsigned x=32;x<40;x++) assert(gop[1+y*40+x]==0xdeadbeef);
}
int main(void) {
    for (unsigned format=0;format<2;format++) {
        for (unsigned i=0;i<2+40*48;i++) gop[i]=0xdeadbeef;
        for (unsigned y=0;y<48;y++) for (unsigned x=0;x<32;x++) gop[1+y*40+x]=0x123456;
        uint32_t *pixels=jihoo_display_init((uintptr_t)(gop+1),40,32,48,format);
        assert(pixels && !allocations && pixels[0]==(format?0x123456:0x563412));
        jihoo_display_present(); guards();
        pixels[3*32+2]=0xaabbcc;
        jihoo_display_mark(2,3,1,1); jihoo_display_present();
        assert(gop[1+3*40+2]==(format?0xaabbcc:0xccbbaa)); guards();
        pixels[0]=0xabcdef;
        jihoo_display_mark(UINT64_MAX,0,1,1); jihoo_display_mark(0,0,0,1); jihoo_display_present();
        assert(gop[1]==0x123456);
        jihoo_display_mark(31,47,UINT64_MAX,UINT64_MAX); jihoo_display_present(); guards();
        uint64_t area[4]; jihoo_display_desktop(area); jihoo_display_present(); guards();
        assert(area[2]==0 && area[3]==0);
    }
    assert(!jihoo_display_init((uintptr_t)(gop+1),40,0,48,1));
    assert(!jihoo_display_init((uintptr_t)(gop+1),40,41,48,1));
    assert(!jihoo_display_init((uintptr_t)(gop+1),40,32,48,2));
    assert(!jihoo_display_init((uintptr_t)(gop+1),UINT64_MAX,UINT64_MAX,UINT64_MAX,1));
    puts("jihoo display: GOP conversion, dirty bounds, clipping, strides, desktop bounds and allocation cleanup passed");
}
