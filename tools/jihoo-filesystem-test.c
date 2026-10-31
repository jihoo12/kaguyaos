#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef struct { uint64_t slot; uint32_t generation; } FileId;
typedef struct { uint8_t name[96]; uint64_t length, size; uint16_t first; } Listing;
typedef int32_t (*Transfer)(void *, uint64_t, uint32_t, void *);
extern void *jihoo_volume_new(void *, uint64_t, Transfer, Transfer);
extern void jihoo_volume_close(void *);
extern int32_t jihoo_volume_check(void *);
extern int32_t jihoo_volume_format(void *);
extern int32_t jihoo_volume_create(void *, const void *, uint64_t, const void *, uint64_t);
extern int32_t jihoo_volume_delete(void *, const void *, uint64_t);
extern int32_t jihoo_volume_open(void *, const void *, uint64_t, bool, bool, FileId *);
extern int64_t jihoo_volume_read(void *, const FileId *, uint64_t, void *, uint64_t);
extern int32_t jihoo_volume_write(void *, const FileId *, uint64_t, bool, const void *, uint64_t, uint64_t *, uint64_t *);
extern int64_t jihoo_volume_size(void *, const FileId *);
extern int64_t jihoo_volume_read_file(void *, const void *, uint64_t, void *, uint64_t);
extern int64_t jihoo_volume_list(void *, Listing *, uint64_t);
static uint64_t allocations;
void kaguya_kernel_panic(const void *p, uint64_t n) { fwrite(p, 1, n, stderr); abort(); }
void *kaguya_runtime_alloc(uint64_t size, uint64_t alignment) {
    assert(alignment <= 16);
    void *p = malloc(size ? size : 1);
    assert(p);
    allocations++;
    return p;
}
void *kaguya_runtime_realloc(void *p, uint64_t size, uint64_t alignment) {
    if (!p) return kaguya_runtime_alloc(size, alignment);
    p = realloc(p, size ? size : 1);
    assert(p);
    return p;
}
void kaguya_runtime_free(void *p) { assert(p && allocations); allocations--; free(p); }
typedef struct { uint8_t *data; uint64_t sectors, reads, writes, fail_read, fail_write; } Disk;
static int32_t read_blocks(void *context, uint64_t lba, uint32_t count, void *out) {
    Disk *d = context;
    assert(out && count && lba <= d->sectors && count <= d->sectors - lba);
    if (++d->reads == d->fail_read) return -3;
    memcpy(out, d->data + lba*512, count*512);
    return 0;
}
static int32_t write_blocks(void *context, uint64_t lba, uint32_t count, void *in) {
    Disk *d = context;
    assert(in && count && lba <= d->sectors && count <= d->sectors - lba);
    if (++d->writes == d->fail_write) return -3;
    memcpy(d->data + lba*512, in, count*512);
    return 0;
}
static Disk disk_new(uint64_t sectors) {
    Disk d = {.sectors = sectors, .fail_read = UINT64_MAX, .fail_write = UINT64_MAX};
    d.data = malloc(sectors*512);
    assert(d.data);
    memset(d.data, 0xa5, sectors*512);
    return d;
}
static uint8_t input[20000], output[24000];
static Listing entries[256];
static int32_t open_file(void *v, const char *name, bool create, bool truncate, FileId *id) {
    return jihoo_volume_open(v, name, strlen(name), create, truncate, id);
}
static int32_t create_file(void *v, const char *name, const void *data, uint64_t length) {
    return jihoo_volume_create(v, name, strlen(name), data, length);
}
static void write_exact(void *v, FileId id, uint64_t offset, bool append, const void *data, uint64_t length, uint64_t expected) {
    uint64_t used = UINT64_MAX, written = UINT64_MAX;
    assert(jihoo_volume_write(v, &id, offset, append, data, length, &used, &written) == 0);
    assert(used == expected && written == length);
}
int main(void) {
    assert(sizeof(FileId) == 16 && sizeof(Listing) == 120);
    for (unsigned i = 0; i < sizeof(input); i++) input[i] = i*17 % 251;
    Disk d = disk_new(8*1024*1024/512);
    void *v = jihoo_volume_new(&d, d.sectors, read_blocks, write_blocks);
    FileId id, old, truncated;
    assert(jihoo_volume_check(v) == -4);
    assert(open_file(v, "alpha", true, false, &id) == -4 && d.writes == 0);
    assert(create_file(v, "alpha", input, 20) == -4 && d.writes == 0);
    assert(jihoo_volume_list(v, entries, 256) == -4 && d.writes == 0);
    assert(jihoo_volume_format(v) == 0 && jihoo_volume_check(v) == 0);
    const uint8_t header[38] = {0x36,0x31,0x54,0x41,0x46,0x47,0x41,0x4b, 0,2, 8,0,0,0,
        1,0,0,0, 64,0,0,0, 65,0,0,0, 16,0,0,0, 81,0,0,0, 0xf5,7,0,0};
    assert(memcmp(d.data, header, sizeof(header)) == 0);
    for (unsigned i = 38; i < 512; i++) assert(d.data[i] == 0);
    assert(d.data[512] == 0xf8 && d.data[513] == 0xff && d.data[514] == 0xff && d.data[515] == 0xff);
    assert(create_file(v, "", input, 1) == -2);
    assert(create_file(v, "1234567890123456789012", input, 1) == -2);
    assert(create_file(v, "alpha", input, 9000) == 0);
    assert(jihoo_volume_read_file(v, "alpha", 5, output, sizeof(output)) == 9000);
    assert(memcmp(input, output, 9000) == 0);
    assert(open_file(v, "alpha", false, false, &id) == 0);
    memset(output, 0xa5, sizeof(output));
    assert(jihoo_volume_read(v, &id, 4090, output, 100) == 100);
    assert(memcmp(input+4090, output, 100) == 0 && output[100] == 0xa5);
    assert(jihoo_volume_read(v, &id, UINT64_MAX, output, 1) == 0);
    write_exact(v, id, 0, true, "abc", 3, 9000);
    write_exact(v, id, 10000, false, "end", 3, 10000);
    assert(jihoo_volume_size(v, &id) == 10003);
    assert(jihoo_volume_read_file(v, "alpha", 5, output, sizeof(output)) == 10003);
    assert(memcmp(output, input, 9000) == 0 && memcmp(output+9000, "abc", 3) == 0);
    for (unsigned i = 9003; i < 10000; i++) assert(output[i] == 0);
    assert(memcmp(output+10000, "end", 3) == 0);
    uint64_t used = 0, written = 0;
    assert(jihoo_volume_write(v, &id, UINT64_MAX, false, NULL, 0, &used, &written) == 0 && used == UINT64_MAX && written == 0);
    assert(jihoo_volume_write(v, &id, UINT64_MAX, false, input, 1, &used, &written) == -5);
    assert(jihoo_volume_list(v, entries, 256) == 1);
    assert(entries[0].length == 5 && memcmp(entries[0].name, "alpha", 5) == 0 && entries[0].size == 10003);
    old = id;
    assert(create_file(v, "alpha", input+10, 17) == 0);
    assert(jihoo_volume_size(v, &old) == -6);
    assert(open_file(v, "alpha", false, false, &id) == 0);
    assert(open_file(v, "alpha", false, true, &truncated) == 0);
    assert(truncated.slot == id.slot && truncated.generation == id.generation && jihoo_volume_size(v, &id) == 0);
    assert(jihoo_volume_delete(v, "alpha", 5) == 0 && jihoo_volume_size(v, &id) == -6);
    assert(jihoo_volume_delete(v, "alpha", 5) == -6);
    const char *unicode = "가나다라마바123";
    assert(strlen(unicode) == 21 && create_file(v, unicode, input, 11) == 0);
    assert(jihoo_volume_list(v, entries, 256) == 1 && entries[0].length == 21 && memcmp(entries[0].name, unicode, 21) == 0);
    // Corrupt on-disk UTF-8: valid prefix before a bad byte, then truncated prefix.
    uint8_t *record = d.data + 65*512 + 32;
    memset(record, 0, 32);
    const uint8_t invalid[] = {0xe1,0x80,'x',0xf0,0x9f};
    memcpy(record, invalid, sizeof(invalid)); record[28] = 1;
    assert(jihoo_volume_list(v, entries, 256) == 2);
    assert(entries[1].length == 7 && memcmp(entries[1].name, "�x�", 7) == 0);
    assert(jihoo_volume_format(v) == 0);
    for (unsigned i = 0; i < 256; i++) {
        char name[16]; snprintf(name, sizeof(name), "f%03u", i);
        assert(create_file(v, name, NULL, 0) == 0);
    }
    assert(create_file(v, "full", NULL, 0) == -5);
    assert(jihoo_volume_list(v, entries, 256) == 256);
    assert(open_file(v, "f000", false, false, &old) == 0);
    assert(jihoo_volume_format(v) == 0 && jihoo_volume_size(v, &old) == -6);
    assert(create_file(v, "failure", input, 30) == 0);
    assert(open_file(v, "failure", false, false, &id) == 0);
    d.fail_read = d.reads + 1;
    memset(output, 0xa5, sizeof(output));
    assert(jihoo_volume_read(v, &id, 0, output, 20) == -3 && output[0] == 0xa5);
    d.fail_read = UINT64_MAX;
    for (unsigned failure = 1; failure <= 2; failure++) {
        d.fail_write = d.writes + failure;
        assert(jihoo_volume_write(v, &id, 0, false, input+1, 20, &used, &written) == -3);
        d.fail_write = UINT64_MAX;
        assert(jihoo_volume_size(v, &id) == 30);
    }
    FileId invalid_id = {256,0};
    assert(jihoo_volume_size(v, &invalid_id) == -6);
    jihoo_volume_close(v);
    assert(allocations == 0);
    free(d.data);
    d = disk_new(81+4*8);
    v = jihoo_volume_new(&d, d.sectors, read_blocks, write_blocks);
    assert(jihoo_volume_format(v) == 0 && open_file(v, "small", true, false, &id) == 0);
    assert(jihoo_volume_write(v, &id, 0, false, input, 20000, &used, &written) == 0 && used == 0 && written == 16384);
    assert(jihoo_volume_size(v, &id) == 16384);
    assert(jihoo_volume_write(v, &id, 0, true, input, 1, &used, &written) == -5);
    assert(jihoo_volume_read_file(v, "small", 5, output, sizeof(output)) == 16384 && memcmp(input, output, 16384) == 0);
    jihoo_volume_close(v);
    free(d.data);
    assert(allocations == 0);
    puts("jihoo filesystem: no autoformat, metadata, UTF-8, stale handles, sparse/partial I/O and failures passed");
}
