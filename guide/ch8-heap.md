# Chapter 8: The Kernel Heap

With paging in place, we can allocate physical frames. But Rust code needs `Vec`, `Box`, `String`, and `VecDeque` — all of which require a heap allocator. This chapter builds one.

## Why a Custom Allocator?

In `#![no_std]`, there's no `malloc`. You must provide a `#[global_allocator]` that implements the `GlobalAlloc` trait:

```rust
pub unsafe trait GlobalAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8;
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout);
}
```

## Setting Aside Heap Memory

During boot, allocate contiguous physical frames for the heap and map them:

```rust
// src/main.rs:292-319 (simplified)
let heap_pages = 128;  // 512 KiB
let heap_start = allocator.allocate_frame().expect("Failed to allocate heap");
let mut current_addr = heap_start;

for _ in 1..heap_pages {
    let next = allocator.allocate_frame().expect("Failed to allocate heap");
    // Verify contiguity
    current_addr = next;
}

// Identity-map the heap pages
let pml4 = memory::get_table_mut(pml4_phys);
for i in 0..heap_pages as u64 {
    memory::map_page(pml4, heap_start + i * 4096, heap_start + i * 4096,
        PAGE_WRITABLE | PAGE_PRESENT, &mut allocator);
}

// Initialize the allocator
allocator::init(heap_start as usize, (heap_pages * 4096) as usize);
```

## The Segregated Free-List Design

kaguyaOS uses a **segregated free-list allocator** — fast for small allocations, with a fallback for large ones:

```
Small allocations (≤ 4096 bytes):
┌──────────┬──────────┬──────────┬─────┬──────────┐
│ Bucket 0 │ Bucket 1 │ Bucket 2 │ ... │ Bucket 9 │
│  8 bytes │ 16 bytes │ 32 bytes │     │ 4096 B   │
└────┬─────┴────┬─────┴────┬─────┘     └────┬─────┘
     │          │          │                 │
     ▼          ▼          ▼                 ▼
   free list  free list  free list       free list
   (linked)   (linked)   (linked)        (linked)

Large allocations (> 4096 bytes):
┌─────────────────────────────────────┐
│ Boundary-tag linked list (first-fit) │
└─────────────────────────────────────┘
```

The key data structure:

```rust
// src/memory/heap.rs:97-109
struct SegregatedAllocator {
    // 10 free lists, one per size class
    free_lists: [*mut u8; NUM_BUCKETS],  // 8, 16, 32, ..., 4096

    // Large-block boundary-tag list
    large_head: *mut LargeBlock,

    // Arena: bump-allocate small blocks here
    arena_ptr: usize,
    arena_end: usize,
}
```

## O(1) Small Allocation

Allocating a small block is either popping from a free list or bumping the arena pointer:

```rust
// src/memory/heap.rs:176-222 (simplified)
unsafe fn alloc_small(&mut self, bucket: usize) -> *mut u8 {
    let class_size = BUCKET_SIZES[bucket];
    let total = SMALL_HEADER_SIZE + class_size;

    // 1. Pop from free list if available
    let head = self.free_lists[bucket];
    if !head.is_null() {
        let next = ptr::read(head as *const *mut u8);
        self.free_lists[bucket] = next;
        ptr::write(head as *mut usize, bucket);  // Write header
        return head.add(SMALL_HEADER_SIZE);      // Return payload
    }

    // 2. Carve from arena
    let aligned = align_up(self.arena_ptr, align_of::<usize>());
    if aligned + total <= self.arena_end {
        self.arena_ptr = aligned + total;
        let block = aligned as *mut u8;
        ptr::write(block as *mut usize, bucket);
        return block.add(SMALL_HEADER_SIZE);
    }

    // 3. Arena exhausted — allocate a slab and carve it up
    // ... (allocates 32 blocks at once, puts 31 on free list)
}
```

The one-word header before each payload stores the bucket index, so `dealloc` knows which free list to return to.

## Deallocation

Freeing is O(1) — read the header, push onto the right free list:

```rust
unsafe fn dealloc_small(&mut self, ptr: *mut u8) {
    let bucket = *(ptr.sub(SMALL_HEADER_SIZE) as *const usize);
    let next = self.free_lists[bucket];
    ptr::write(ptr as *mut *mut u8, next);  // Link to old head
    self.free_lists[bucket] = ptr;           // New head
}
```

## Global Allocator Implementation

```rust
pub struct KernelAllocator;

#[global_allocator]
static ALLOCATOR: KernelAllocator = KernelAllocator;

unsafe impl GlobalAlloc for KernelAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        INNER_ALLOCATOR.lock().alloc_aligned(layout.size(), layout.align())
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        INNER_ALLOCATOR.lock().dealloc_aligned(ptr, layout.size(), layout.align());
    }
}
```

## The User Heap

User-mode programs also need heap allocation (for `exec` arguments, etc.). kaguyaOS sets up a **separate** heap with pages mapped `PAGE_USER | PAGE_NO_EXECUTE`:

```rust
// src/main.rs:366-398 (simplified)
const USER_HEAP_VIRT_BASE: u64 = 0x0000_7000_0000_0000;
let user_heap_pages = 128;  // 512 KiB

// Allocate physical frames
let user_heap_phys_start = allocator.allocate_frame()...;

// Map at high virtual address with user-accessible + no-execute
let flags = PAGE_WRITABLE | PAGE_USER | PAGE_NO_EXECUTE;
for i in 0..user_heap_pages as u64 {
    let phys = user_heap_phys_start + i * 4096;
    let virt = USER_HEAP_VIRT_BASE + i * 4096;
    memory::map_page(pml4, virt, phys, flags, &mut allocator);
}

// Initialize user allocator
heap::init_user_heap(USER_HEAP_VIRT_BASE as usize,
    (user_heap_pages * 4096) as usize);
```

The high virtual address (`0x7000_0000_0000`) avoids collision with the user program's code and stack at low addresses.

## kaguyaOS Reference

| File | Lines | What it does |
|------|-------|-------------|
| `src/memory/heap.rs` | 65-109 | Constants, LargeBlock, SegregatedAllocator |
| `src/memory/heap.rs` | 123-172 | init(), bucket_for() |
| `src/memory/heap.rs` | 176-270 | alloc_small(), alloc_large_raw() |
| `src/memory/heap.rs` | 415-500 | Global allocator, user heap |

---

**Next:** [Chapter 9 — Ring 0/3 Isolation & Syscalls](ch9-syscalls.md) — Crossing the user/kernel boundary safely.
