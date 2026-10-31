#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <pthread.h>
#include <stdatomic.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

extern void jihoo_owned_pipe_new(void **, void **);
extern int64_t jihoo_owned_pipe_read(void *, void *, uint64_t);
extern int64_t jihoo_owned_pipe_write(void *, const void *, uint64_t);
extern void *jihoo_owned_pipe_clone_reader(void *);
extern void *jihoo_owned_pipe_clone_writer(void *);
extern void jihoo_owned_pipe_close_reader(void *);
extern void jihoo_owned_pipe_close_writer(void *);
static _Atomic uint64_t allocations;
static _Thread_local bool enabled = true;
static pthread_mutex_t scheduler = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t changed = PTHREAD_COND_INITIALIZER;
static uint64_t event_key, generation, waiting;
static void *close_before_wait;
uint64_t jihoo_irq_save(void) { bool previous = enabled; enabled = false; return previous ? 0x200 : 0; }
void jihoo_irq_enable(void) { assert(!enabled); enabled = true; }
void jihoo_pause(void) { __asm__ volatile("pause"); }
void kaguya_kernel_panic(const void *p, uint64_t n) { fwrite(p, 1, n, stderr); abort(); }
void *kaguya_runtime_alloc(uint64_t size, uint64_t alignment) {
    assert(alignment <= 16);
    void *p = malloc(size ? size : 1);
    assert(p);
    atomic_fetch_add(&allocations, 1);
    return p;
}
void kaguya_runtime_free(void *p) {
    assert(p && atomic_load(&allocations) > 0);
    atomic_fetch_sub(&allocations, 1);
    free(p);
}
static void ensure_key(uint64_t key) {
    assert(key);
    if (!event_key) { event_key = key; generation = 0; }
    assert(event_key == key);
}
uint64_t kaguya_event_generation(uint64_t key) {
    assert(enabled); // pipe state lock must be released before scheduler calls
    pthread_mutex_lock(&scheduler);
    ensure_key(key);
    uint64_t result = generation;
    pthread_mutex_unlock(&scheduler);
    return result;
}
void kaguya_wake_event(uint64_t key) {
    assert(enabled);
    pthread_mutex_lock(&scheduler);
    ensure_key(key);
    generation++;
    pthread_cond_broadcast(&changed);
    pthread_mutex_unlock(&scheduler);
}
void kaguya_wait_event(uint64_t key, uint64_t deadline, uint64_t observed) {
    assert(enabled && deadline == 0);
    if (close_before_wait) {
        void *reader = close_before_wait;
        close_before_wait = NULL;
        jihoo_owned_pipe_close_reader(reader);
    }
    pthread_mutex_lock(&scheduler);
    ensure_key(key);
    waiting++;
    pthread_cond_broadcast(&changed);
    while (generation == observed) pthread_cond_wait(&changed, &scheduler);
    waiting--;
    pthread_mutex_unlock(&scheduler);
}
void kaguya_forget_event(uint64_t key) {
    assert(enabled);
    pthread_mutex_lock(&scheduler);
    assert(waiting == 0 && event_key == key);
    event_key = generation = 0;
    pthread_mutex_unlock(&scheduler);
}
static void clean(void) { assert(enabled && allocations == 0 && event_key == 0 && waiting == 0); }
static uint8_t input[60000], output[60000];
typedef struct { void *writer; uint64_t length; int64_t result; } Write;
static void *write_stream(void *data) {
    Write *work = data;
    work->result = jihoo_owned_pipe_write(work->writer, input, work->length);
    assert(enabled);
    jihoo_owned_pipe_close_writer(work->writer);
    return NULL;
}
int main(void) {
    alarm(20);
    void *reader, *writer;
    jihoo_owned_pipe_new(&reader, &writer);
    void *reader2 = jihoo_owned_pipe_clone_reader(reader);
    void *writer2 = jihoo_owned_pipe_clone_writer(writer);
    assert(jihoo_owned_pipe_write(writer, "abc", 3) == 3);
    assert(jihoo_owned_pipe_read(reader, output, 2) == 2 && memcmp(output, "ab", 2) == 0);
    jihoo_owned_pipe_close_writer(writer);
    assert(jihoo_owned_pipe_read(reader2, output, 8) == 1 && output[0] == 'c');
    jihoo_owned_pipe_close_writer(writer2);
    assert(jihoo_owned_pipe_read(reader, output, 8) == 0);
    jihoo_owned_pipe_close_reader(reader);
    assert(jihoo_owned_pipe_read(reader2, NULL, 0) == 0);
    jihoo_owned_pipe_close_reader(reader2);
    clean();
    jihoo_owned_pipe_new(&reader, &writer);
    jihoo_owned_pipe_close_reader(reader);
    assert(jihoo_owned_pipe_write(writer, NULL, 0) == 0);
    assert(jihoo_owned_pipe_write(writer, "a", 1) == -9);
    jihoo_owned_pipe_close_writer(writer);
    clean();
    for (unsigned i = 0; i < sizeof(input); i++) input[i] = i*17 % 251;
    jihoo_owned_pipe_new(&reader, &writer);
    Write stream = {writer, sizeof(input), -99};
    pthread_t thread;
    assert(pthread_create(&thread, NULL, write_stream, &stream) == 0);
    uint64_t received = 0;
    for (;;) {
        uint8_t block[700];
        int64_t count = jihoo_owned_pipe_read(reader, block, sizeof(block));
        assert(count >= 0);
        if (!count) break;
        assert(received + count <= sizeof(output));
        memcpy(output + received, block, count);
        received += count;
    }
    assert(pthread_join(thread, NULL) == 0);
    assert(stream.result == sizeof(input) && received == sizeof(input));
    assert(memcmp(input, output, sizeof(input)) == 0);
    jihoo_owned_pipe_close_reader(reader);
    clean();
    jihoo_owned_pipe_new(&reader, &writer);
    Write partial = {writer, 6000, -99};
    assert(pthread_create(&thread, NULL, write_stream, &partial) == 0);
    pthread_mutex_lock(&scheduler);
    while (waiting == 0) pthread_cond_wait(&changed, &scheduler);
    pthread_mutex_unlock(&scheduler);
    jihoo_owned_pipe_close_reader(reader);
    assert(pthread_join(thread, NULL) == 0);
    assert(partial.result == 4096);
    clean();
    jihoo_owned_pipe_new(&reader, &writer);
    close_before_wait = reader;
    assert(jihoo_owned_pipe_write(writer, input, 6000) == 4096);
    assert(!close_before_wait);
    jihoo_owned_pipe_close_writer(writer);
    clean();
    puts("jihoo owned pipes: clones/EOF, 60000-byte streaming, blocking, partial close and generation race passed");
}
