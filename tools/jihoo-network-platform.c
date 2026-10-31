#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>
__attribute__((weak)) uint64_t kaguya_alloc_frame(void) { abort(); }
__attribute__((weak)) bool kaguya_map_mmio(uint64_t base, uint64_t bytes) { (void)base; (void)bytes; abort(); }
__attribute__((weak)) void kaguya_device_log(const void *bytes,uint64_t length) { (void)bytes; (void)length; }
__attribute__((weak)) uint32_t jihoo_inl(uint16_t port) { (void)port; abort(); }
__attribute__((weak)) void jihoo_outl(uint16_t port,uint32_t value) { (void)port; (void)value; abort(); }
__attribute__((weak)) uint8_t jihoo_inb(uint16_t port) { (void)port; abort(); }
__attribute__((weak)) void jihoo_outb(uint16_t port,uint8_t value) { (void)port; (void)value; abort(); }
__attribute__((weak)) void kaguya_receive_udp(uint32_t a,uint32_t b,const void *p,uint64_t n) { (void)a;(void)b;(void)p;(void)n;abort(); }
__attribute__((weak)) void kaguya_receive_tcp(uint32_t a,uint32_t b,const void *p,uint64_t n) { (void)a;(void)b;(void)p;(void)n;abort(); }
__attribute__((weak)) void kaguya_network_timers(void) { abort(); }

__attribute__((weak)) uint64_t kaguya_network_clock(void) { return 0; }
__attribute__((weak)) uint32_t kaguya_network_random(void) { return 1; }
__attribute__((weak)) void jihoo_dma_fence(void) { __atomic_thread_fence(__ATOMIC_SEQ_CST); }
