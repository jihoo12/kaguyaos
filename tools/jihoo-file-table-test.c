#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <pthread.h>
#include <stdatomic.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

typedef int32_t (*Transfer)(void *, uint64_t, uint32_t, void *);
extern void *jihoo_table_new(void *, uint64_t, Transfer, Transfer);
extern void jihoo_table_drop(void *);
extern void *jihoo_table_clone(void *);
extern void *jihoo_table_inherit(void *, const uint64_t *, int32_t *);
extern int64_t jihoo_table_open(void *, const void *, uint64_t, uint64_t);
extern int32_t jihoo_table_close(void *, uint64_t);
extern int64_t jihoo_table_kind(void *, uint64_t);
extern int64_t jihoo_table_read(void *, uint64_t, void *, uint64_t);
extern int64_t jihoo_table_write(void *, uint64_t, const void *, uint64_t);
extern int64_t jihoo_table_seek(void *, uint64_t, int64_t, uint64_t);
extern int32_t jihoo_table_pipe(void *, uint64_t *, uint64_t *);
static _Atomic uint64_t allocations;
static _Thread_local bool enabled = true;
static pthread_mutex_t scheduler = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t changed = PTHREAD_COND_INITIALIZER;
static uint64_t event_key, generation, waiting;
uint64_t jihoo_irq_save(void) { bool previous = enabled; enabled = false; return previous ? 0x200 : 0; }
void jihoo_irq_enable(void) { assert(!enabled); enabled = true; }
void jihoo_pause(void) { __asm__ volatile("pause"); }
void kaguya_kernel_panic(const void *p, uint64_t n) { fwrite(p, 1, n, stderr); abort(); }
void *kaguya_runtime_alloc(uint64_t size, uint64_t alignment) {
    assert(alignment <= 16);
    void *p = malloc(size ? size : 1);
    assert(p); atomic_fetch_add(&allocations, 1); return p;
}
void *kaguya_runtime_realloc(void *p, uint64_t size, uint64_t alignment) {
    if (!p) return kaguya_runtime_alloc(size, alignment);
    p = realloc(p, size ? size : 1); assert(p); return p;
}
void kaguya_runtime_free(void *p) { assert(p && allocations); atomic_fetch_sub(&allocations, 1); free(p); }
static void ensure_key(uint64_t key) {
    assert(key);
    if (!event_key) { event_key = key; generation = 0; }
    assert(event_key == key);
}
uint64_t kaguya_event_generation(uint64_t key) {
    assert(enabled); pthread_mutex_lock(&scheduler); ensure_key(key);
    uint64_t result = generation; pthread_mutex_unlock(&scheduler); return result;
}
void kaguya_wake_event(uint64_t key) {
    assert(enabled); pthread_mutex_lock(&scheduler); ensure_key(key);
    generation++; pthread_cond_broadcast(&changed); pthread_mutex_unlock(&scheduler);
}
void kaguya_wait_event(uint64_t key, uint64_t deadline, uint64_t observed) {
    assert(enabled && deadline == 0); pthread_mutex_lock(&scheduler); ensure_key(key);
    waiting++; pthread_cond_broadcast(&changed);
    while (generation == observed) pthread_cond_wait(&changed, &scheduler);
    waiting--; pthread_mutex_unlock(&scheduler);
}
void kaguya_forget_event(uint64_t key) {
    assert(enabled); pthread_mutex_lock(&scheduler);
    assert(waiting == 0 && event_key == key); event_key = generation = 0;
    pthread_mutex_unlock(&scheduler);
}
int64_t kaguya_console_read(void *out, uint64_t count) {
    assert(enabled); if (!count) return 0; *(uint8_t *)out = 'K'; return 1;
}
int64_t kaguya_console_write(const void *in, uint64_t count) {
    assert(enabled); assert(count == 3 && memcmp(in, "log", 3) == 0); return count;
}
static uint8_t disk[(81+4*8)*512], input[6000], output[6000];
static int32_t read_blocks(void *context, uint64_t lba, uint32_t count, void *out) {
    assert(!enabled && context == disk && count && lba <= sizeof(disk)/512 && count <= sizeof(disk)/512-lba);
    memcpy(out, disk+lba*512, count*512); return 0;
}
static int32_t write_blocks(void *context, uint64_t lba, uint32_t count, void *in) {
    assert(!enabled && context == disk && count && lba <= sizeof(disk)/512 && count <= sizeof(disk)/512-lba);
    memcpy(disk+lba*512, in, count*512); return 0;
}
static int64_t open_file(void *table, uint64_t flags) { return jihoo_table_open(table, "file", 4, flags); }
typedef struct { void *table; uint64_t writer; int64_t result; } Write;
static void *write_stream(void *data) {
    Write *work = data;
    work->result = jihoo_table_write(work->table, work->writer, input, sizeof(input));
    assert(enabled); return NULL;
}
int main(void) {
    alarm(20);
    void *table = jihoo_table_new(disk, sizeof(disk)/512, read_blocks, write_blocks);
    assert(jihoo_table_read(table, 0, output, 1) == 1 && output[0] == 'K');
    assert(jihoo_table_write(table, 1, "log", 3) == 3);
    assert(jihoo_table_kind(table, 0) == 0 && jihoo_table_kind(table, 999) == -7);
    assert(open_file(table, 0) == -2 && open_file(table, 32) == -2 && open_file(table, 5) == -2);
    assert(open_file(table, 1) == -6);
    int64_t fd = open_file(table, 15);
    assert(fd == 3 && jihoo_table_kind(table, fd) == 1);
    assert(jihoo_table_write(table, fd, "abc", 3) == 3);
    assert(jihoo_table_seek(table, fd, 0, 0) == 0);
    assert(jihoo_table_read(table, fd, output, 3) == 3 && memcmp(output, "abc", 3) == 0);
    int64_t reader = open_file(table, 1);
    assert(reader == 4 && jihoo_table_write(table, reader, "x", 1) == -7);
    assert(jihoo_table_read(table, reader, output, 1) == 1 && output[0] == 'a');
    uint64_t stdio[] = {reader, fd, 2}; int32_t error = -99;
    void *child = jihoo_table_inherit(table, stdio, &error);
    assert(child && error == 0);
    assert(jihoo_table_read(child, 0, output, 1) == 1 && output[0] == 'b');
    assert(jihoo_table_read(table, reader, output, 1) == 1 && output[0] == 'b');
    void *alias = jihoo_table_clone(table);
    assert(jihoo_table_read(alias, reader, output, 1) == 1 && output[0] == 'c');
    jihoo_table_drop(alias);
    assert(jihoo_table_close(table, reader) == 0);
    assert(jihoo_table_read(child, 0, output, 1) == 1 && output[0] == 'c');
    jihoo_table_drop(child);
    stdio[1] = 999;
    assert(!jihoo_table_inherit(table, stdio, &error) && error == -7);
    int64_t append = open_file(table, 18);
    assert(append == 4 && jihoo_table_seek(table, append, 0, 0) == 0);
    assert(jihoo_table_write(table, append, "z", 1) == 1 && jihoo_table_read(table, append, output, 1) == -7);
    assert(jihoo_table_seek(table, fd, -1, 2) == 3);
    assert(jihoo_table_read(table, fd, output, 1) == 1 && output[0] == 'z');
    assert(jihoo_table_seek(table, fd, -1, 0) == -2);
    assert(jihoo_table_seek(table, fd, INT64_MIN, 1) == -2);
    assert(jihoo_table_seek(table, fd, 0x100000000, 0) == -2);
    assert(jihoo_table_seek(table, fd, 0, 3) == -2);
    assert(jihoo_table_close(table, fd) == 0 && jihoo_table_close(table, fd) == -7);
    assert(jihoo_table_close(table, append) == 0);
    uint64_t r, w;
    assert(jihoo_table_pipe(table, &r, &w) == 0 && jihoo_table_kind(table, r) == 2 && jihoo_table_kind(table, w) == 2);
    assert(jihoo_table_seek(table, r, 0, 0) == -7);
    assert(jihoo_table_read(table, w, output, 1) == -7 && jihoo_table_write(table, r, input, 1) == -7);
    stdio[0] = r; stdio[1] = w;
    child = jihoo_table_inherit(table, stdio, &error); assert(child && !error);
    assert(jihoo_table_close(table, w) == 0 && jihoo_table_write(child, 1, "y", 1) == 1);
    assert(jihoo_table_read(table, r, output, 2) == 1 && output[0] == 'y');
    jihoo_table_drop(child);
    assert(jihoo_table_read(table, r, output, 1) == 0 && jihoo_table_close(table, r) == 0 && event_key == 0);
    for (unsigned i = 0; i < sizeof(input); i++) input[i] = i*17 % 251;
    assert(jihoo_table_pipe(table, &r, &w) == 0);
    Write work = {table,w,-99}; pthread_t thread;
    assert(pthread_create(&thread, NULL, write_stream, &work) == 0);
    pthread_mutex_lock(&scheduler);
    while (!waiting) pthread_cond_wait(&changed, &scheduler);
    pthread_mutex_unlock(&scheduler);
    assert(jihoo_table_close(table, w) == 0); // in-flight writer still owns a counted end
    uint64_t received = 0;
    for (;;) {
        uint8_t block[500]; int64_t count = jihoo_table_read(table, r, block, sizeof(block));
        assert(count >= 0 && received + count <= sizeof(output));
        if (!count) break;
        memcpy(output+received, block, count); received += count;
    }
    assert(pthread_join(thread, NULL) == 0 && work.result == 6000 && received == 6000 && memcmp(input, output, 6000) == 0);
    assert(jihoo_table_close(table, r) == 0 && event_key == 0);
    for (unsigned i = 3; i < 31; i++) assert(open_file(table, 1) == i);
    uint64_t before = allocations;
    assert(jihoo_table_pipe(table, &r, &w) == -8 && allocations == before && event_key == 0);
    assert(open_file(table, 1) == 31 && open_file(table, 1) == -8);
    assert(jihoo_table_pipe(table, &r, &w) == -8 && allocations == before && event_key == 0);
    stdio[0] = 3; stdio[1] = 4;
    child = jihoo_table_inherit(table, stdio, &error); assert(child && !error);
    jihoo_table_drop(table);
    assert(jihoo_table_read(child, 0, output, 1) == 1 && output[0] == 'a');
    jihoo_table_drop(child);
    assert(enabled && allocations == 0 && event_key == 0 && waiting == 0);
    puts("jihoo file table: flags, offsets, inheritance, FD capacity, pipes and close-during-I/O passed");
}
