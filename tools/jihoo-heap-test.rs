use std::alloc::{Layout, alloc, dealloc};
use std::ptr;

unsafe extern "C" {
    fn jihoo_heap_init(start: usize, size: usize);
    fn jihoo_heap_alloc(size: usize, alignment: usize) -> *mut u8;
    fn jihoo_heap_free(payload: *mut u8);
    fn jihoo_heap_realloc(payload: *mut u8, size: usize, alignment: usize) -> *mut u8;
}

const GUARD: usize = 4096;

struct Heap {
    backing: *mut u8,
    size: usize,
    layout: Layout,
}

impl Heap {
    fn new(size: usize) -> Self {
        let layout = Layout::from_size_align(size + GUARD * 2, GUARD).unwrap();
        let backing = unsafe { alloc(layout) };
        assert!(!backing.is_null());
        unsafe {
            ptr::write_bytes(backing, 0xa5, layout.size());
            jihoo_heap_init(backing.add(GUARD) as usize, size);
        }
        Self {
            backing,
            size,
            layout,
        }
    }

    fn check(&self, p: *mut u8, size: usize, align: usize) {
        assert!(!p.is_null(), "allocation failed: {size}, alignment {align}");
        let start = self.backing as usize + GUARD;
        assert!(p as usize >= start);
        assert!(p as usize + size <= start + self.size);
        assert_eq!(p as usize % align, 0);
    }

    fn guards(&self) {
        for offset in [0, GUARD + self.size] {
            let bytes = unsafe { std::slice::from_raw_parts(self.backing.add(offset), GUARD) };
            assert!(
                bytes.iter().all(|&b| b == 0xa5),
                "heap boundary overwritten"
            );
        }
    }
}

impl Drop for Heap {
    fn drop(&mut self) {
        self.guards();
        unsafe { dealloc(self.backing, self.layout) };
    }
}

unsafe fn bytes(p: *mut u8, size: usize, value: u8) {
    assert!(
        unsafe { std::slice::from_raw_parts(p, size) }
            .iter()
            .all(|&b| b == value)
    );
}

fn main() {
    unsafe {
        let heap = Heap::new(1024 * 1024);
        let mut live = Vec::new();
        for size in [
            0, 1, 8, 9, 16, 17, 32, 33, 64, 65, 128, 129, 256, 257, 512, 513, 1024, 1025, 2048,
            2049, 4096, 4097, 8192,
        ] {
            for align in [1, 2, 4, 8, 16, 64, 256, 4096] {
                let p = jihoo_heap_alloc(size, align);
                heap.check(p, size, align);
                let value = live.len() as u8;
                ptr::write_bytes(p, value, size);
                live.push((p, size, value));
            }
        }
        for &(p, size, value) in &live {
            bytes(p, size, value);
        }
        for &(p, _, _) in live.iter().rev() {
            jihoo_heap_free(p);
        }
        for size in [8, 16, 32, 64, 128, 256, 512, 1024, 2048, 4096] {
            let p = jihoo_heap_alloc(size, 8);
            jihoo_heap_free(p);
            assert_eq!(
                jihoo_heap_alloc(size, 8),
                p,
                "bucket did not reuse freed block"
            );
            jihoo_heap_free(p);
        }
        jihoo_heap_free(ptr::null_mut());
        for align in [0, 3, 6, usize::MAX] {
            assert!(jihoo_heap_alloc(32, align).is_null());
        }
        assert!(jihoo_heap_alloc(usize::MAX, 8).is_null());
        assert!(jihoo_heap_alloc(usize::MAX - 32, 16).is_null());
        assert!(jihoo_heap_alloc(1, 1usize << 63).is_null());
        drop(heap);

        // Exhaust the bump arena, then use and reuse the fallback slabs.
        let heap = Heap::new(65536);
        let mut live = Vec::new();
        loop {
            let p = jihoo_heap_alloc(32, 8);
            if p.is_null() {
                break;
            }
            heap.check(p, 32, 8);
            assert!(!live.contains(&p), "allocator returned a live block twice");
            ptr::write_bytes(p, 0x36, 32);
            live.push(p);
            assert!(live.len() < 65536 / 32);
        }
        assert!(
            live.len() > (65536 / 4 * 3) / 40,
            "slab fallback not exercised"
        );
        for &p in &live {
            bytes(p, 32, 0x36);
            jihoo_heap_free(p);
        }
        for _ in 0..live.len() {
            let p = jihoo_heap_alloc(32, 8);
            heap.check(p, 32, 8);
        }
        assert!(jihoo_heap_alloc(32, 8).is_null());
        drop(heap);

        // Coalesce in both directions and recover the entire large region.
        for order in [[0, 1, 2], [2, 0, 1], [1, 2, 0]] {
            let heap = Heap::new(65536);
            let blocks = [
                jihoo_heap_alloc(4100, 8),
                jihoo_heap_alloc(4100, 8),
                jihoo_heap_alloc(4100, 8),
            ];
            for &p in &blocks {
                heap.check(p, 4100, 8);
            }
            assert!(jihoo_heap_alloc(4100, 8).is_null());
            for i in order {
                jihoo_heap_free(blocks[i]);
            }
            let p = jihoo_heap_alloc(16000, 8);
            heap.check(p, 16000, 8);
            jihoo_heap_free(p);
        }

        let heap = Heap::new(1024 * 1024);
        for align in [8, 16, 4096] {
            let mut p = jihoo_heap_realloc(ptr::null_mut(), 17, align);
            heap.check(p, 17, align);
            ptr::write_bytes(p, 0x5a, 17);
            let mut old_size = 17;
            for size in [1024, 9000, 7, 32000, 1] {
                let next = jihoo_heap_realloc(p, size, align);
                heap.check(next, size, align);
                bytes(next, size.min(old_size), 0x5a);
                ptr::write_bytes(next, 0x5a, size);
                p = next;
                old_size = size;
            }
            assert!(jihoo_heap_realloc(p, 1024 * 1024, align).is_null());
            bytes(p, old_size, 0x5a);
            jihoo_heap_free(p);
        }
        drop(heap);

        // Reproducible mixed allocation/free/reallocation with live-data checks.
        let heap = Heap::new(1024 * 1024);
        let mut live: Vec<(*mut u8, usize, usize, u8)> = Vec::new();
        let mut rng = 0x123456789abcdefu64;
        for step in 0..20000 {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            for &(p, size, _, value) in &live {
                bytes(p, size, value);
            }
            if !live.is_empty() && (rng % 3 == 0 || live.len() >= 100) {
                let index = rng as usize % live.len();
                let (p, size, align, value) = live[index];
                if rng & 8 == 0 {
                    jihoo_heap_free(p);
                    live.swap_remove(index);
                } else {
                    let new_size = (rng >> 16) as usize % 12000 + 1;
                    let next = jihoo_heap_realloc(p, new_size, align);
                    if next.is_null() {
                        bytes(p, size, value);
                    } else {
                        heap.check(next, new_size, align);
                        bytes(next, size.min(new_size), value);
                        ptr::write_bytes(next, value, new_size);
                        live[index] = (next, new_size, align, value);
                    }
                }
            } else {
                let size = (rng >> 16) as usize % 12000 + 1;
                let align = 1 << (rng as usize % 13);
                let p = jihoo_heap_alloc(size, align);
                if !p.is_null() {
                    heap.check(p, size, align);
                    let value = step as u8;
                    ptr::write_bytes(p, value, size);
                    live.push((p, size, align, value));
                }
            }
        }
        for (p, _, _, _) in live {
            jihoo_heap_free(p);
        }
        heap.guards();
    }
    println!(
        "jihoo heap: allocation, reuse, alignment, coalescing, slabs, realloc, OOM and mixed stress passed"
    );
}
