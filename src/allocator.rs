use core::alloc::{GlobalAlloc, Layout};
use core::cell::UnsafeCell;
use core::mem;
use core::ops::{Deref, DerefMut};
use core::ptr;
use core::sync::atomic::{AtomicBool, Ordering};

// ---------------------------------------------------------------------------
// Segregated Free List Allocator
// ---------------------------------------------------------------------------
//
// Design overview
// ---------------
// Small allocations (size <= MAX_SMALL_SIZE) are served from a fixed set of
// size-class buckets.  Each bucket is a singly-linked free list whose nodes
// are embedded directly inside the free block — no separate metadata block
// sits in front of a live allocation for the common path.
//
// Large allocations (size > MAX_SMALL_SIZE) fall back to a classic
// boundary-tag linked list (first-fit) — the same structure that was used
// everywhere before, now only paid for when really needed.
//
// Bucket layout
// -------------
//   index │ block size (bytes)
//   ──────┼───────────────────
//     0   │   8
//     1   │  16
//     2   │  32
//     3   │  64
//     4   │ 128
//     5   │ 256
//     6   │ 512
//     7   │ 1024
//     8   │ 2048
//     9   │ 4096   (MAX_SMALL_SIZE)
//
// Each free node in a bucket stores a single pointer to the next free node
// at offset 0 of the block body — nothing extra.  When a block is allocated
// we write the bucket index at a one-word header immediately before the
// returned pointer so `dealloc` can put the block back in the right list in
// O(1).  For large blocks the header stores LARGE_BLOCK_SENTINEL and the
// usual boundary-tag block pointer (two words before the payload).
//
// Memory layout for a small allocation
// ─────────────────────────────────────
//   [ bucket_index: usize ] [ payload … ]
//    ^^^^ 1 word header      ^^^^ returned to caller
//
// Memory layout for a large allocation (mirrors original OffsetHeader scheme)
// ──────────────────────────────────────────────────────────────────────────
//   [ LargeBlock ] … padding … [ LARGE_BLOCK_SENTINEL: usize ]
//                               [ block_addr:           usize ]
//                               [ payload … ]
//
// Complexity
// ----------
//   alloc  (small)  O(1)   — pop head of bucket list, or carve from arena
//   dealloc(small)  O(1)   — push onto bucket list
//   alloc  (large)  O(n)   — first-fit scan (unchanged from before)
//   dealloc(large)  O(1)   — mark free + coalesce neighbours (O(1) with ptrs)

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const NUM_BUCKETS: usize = 10;
/// Maximum size served by the segregated-list fast path.
const MAX_SMALL_SIZE: usize = 4096;
/// Sentinel stored in the one-word header of a small block.
const LARGE_BLOCK_SENTINEL: usize = usize::MAX;

/// Size classes for the 10 buckets (must be power-of-two, ascending).
const BUCKET_SIZES: [usize; NUM_BUCKETS] = [8, 16, 32, 64, 128, 256, 512, 1024, 2048, 4096];

/// Header prepended to every small-block payload: one `usize` (bucket index).
const SMALL_HEADER_SIZE: usize = mem::size_of::<usize>();

/// Size of the LargeBlock boundary-tag header.
const LARGE_BLOCK_SIZE: usize = mem::size_of::<LargeBlock>();

// ---------------------------------------------------------------------------
// Large-block boundary-tag structure (used for size > MAX_SMALL_SIZE)
// ---------------------------------------------------------------------------

#[repr(C)]
struct LargeBlock {
    /// Usable bytes in this block (excluding the LargeBlock header itself).
    size: usize,
    next: *mut LargeBlock,
    prev: *mut LargeBlock,
    free: bool,
}

// ---------------------------------------------------------------------------
// Segregated free list allocator state
// ---------------------------------------------------------------------------

struct SegregatedAllocator {
    /// Heads of the per-size-class free lists.  Each entry is either null or
    /// points to the first free block in that class.
    free_lists: [*mut u8; NUM_BUCKETS],

    /// Head of the large-block linked list.
    large_head: *mut LargeBlock,

    /// Bump pointer into the raw arena — used only when a bucket's free list
    /// is empty and we need to carve a fresh block.
    arena_ptr: usize,
    arena_end: usize,
}

unsafe impl Send for SegregatedAllocator {}

impl SegregatedAllocator {
    pub const fn new() -> Self {
        Self {
            free_lists: [ptr::null_mut(); NUM_BUCKETS],
            large_head: ptr::null_mut(),
            arena_ptr: 0,
            arena_end: 0,
        }
    }

    pub unsafe fn init(&mut self, start: usize, size: usize) {
        // The first part of the heap is managed as the bump arena for small
        // blocks.  We hand the remainder to the large-block list so big
        // allocations can still use the full heap.
        //
        // Split: first 3/4 → arena, last 1/4 → large-block list.
        // (Tune this ratio for your workload.)
        let arena_bytes = (size / 4) * 3;
        self.arena_ptr = start;
        self.arena_end = start + arena_bytes;

        // Initialise the large-block region.
        let lb_start = start + arena_bytes;
        let lb_size = size - arena_bytes;
        if lb_size > LARGE_BLOCK_SIZE {
            let block = lb_start as *mut LargeBlock;
            unsafe {
                (*block).size = lb_size - LARGE_BLOCK_SIZE;
                (*block).next = ptr::null_mut();
                (*block).prev = ptr::null_mut();
                (*block).free = true;
            }
            self.large_head = block;
        }

        crate::println!(
            "Heap initialised at {:#x}, size {}: arena [{:#x}–{:#x}), large [{:#x}–{:#x})",
            start,
            size,
            start,
            self.arena_end,
            lb_start,
            lb_start + lb_size,
        );
    }

    // -----------------------------------------------------------------------
    // Small allocation helpers
    // -----------------------------------------------------------------------

    /// Return the bucket index for `size`, or `None` if size > MAX_SMALL_SIZE.
    #[inline]
    fn bucket_for(size: usize) -> Option<usize> {
        for (i, &class_size) in BUCKET_SIZES.iter().enumerate() {
            if size <= class_size {
                return Some(i);
            }
        }
        None
    }

    /// Allocate a small block from `bucket`.  Returns a pointer to the
    /// *payload* (i.e. the word after the bucket-index header).
    unsafe fn alloc_small(&mut self, bucket: usize) -> *mut u8 {
        let class_size = BUCKET_SIZES[bucket];
        let total = SMALL_HEADER_SIZE + class_size;

        // 1. Pop from the free list if available.
        let head = self.free_lists[bucket];
        if !head.is_null() {
            // The first word of a free block is the next-pointer.
            let next = unsafe { ptr::read(head as *const *mut u8) };
            self.free_lists[bucket] = next;

            // Write the bucket index into the header slot.
            unsafe { ptr::write(head as *mut usize, bucket) };
            return unsafe { head.add(SMALL_HEADER_SIZE) };
        }

        // 2. Carve from the bump arena.
        let aligned = align_up(self.arena_ptr, mem::align_of::<usize>());
        if aligned + total <= self.arena_end {
            self.arena_ptr = aligned + total;
            let block = aligned as *mut u8;
            unsafe { ptr::write(block as *mut usize, bucket) };
            return unsafe { block.add(SMALL_HEADER_SIZE) };
        }

        // 3. Arena exhausted — fall back to the large-block region.
        //    Allocate a slab of (e.g.) 32 × class_size and cut it up.
        let slab_count: usize = 32;
        let slab_size = total * slab_count;
        let slab = self.alloc_large_raw(slab_size);
        if slab.is_null() {
            return ptr::null_mut();
        }
        // Carve blocks out of slab and push all but the first onto the list.
        let slab_addr = slab as usize;
        for i in 1..slab_count {
            let blk = (slab_addr + i * total) as *mut u8;
            // Push blk onto free list.
            unsafe {
                ptr::write(blk as *mut *mut u8, self.free_lists[bucket]);
            }
            self.free_lists[bucket] = blk;
        }
        // Return the zeroth block.
        unsafe { ptr::write(slab as *mut usize, bucket) };
        unsafe { slab.add(SMALL_HEADER_SIZE) }
    }

    /// Return a small block to its bucket's free list.
    unsafe fn free_small(&mut self, payload: *mut u8) {
        // The header lives one word before the payload.
        let block = unsafe { payload.sub(SMALL_HEADER_SIZE) };
        let bucket = unsafe { ptr::read(block as *const usize) };

        // Overwrite the header slot with the next-pointer.
        unsafe { ptr::write(block as *mut *mut u8, self.free_lists[bucket]) };
        self.free_lists[bucket] = block;
    }

    // -----------------------------------------------------------------------
    // Large allocation helpers (boundary-tag first-fit)
    // -----------------------------------------------------------------------

    /// Allocate `size` raw bytes from the large-block list.
    /// Returns a pointer to the payload (just past the LargeBlock header).
    unsafe fn alloc_large_raw(&mut self, mut size: usize) -> *mut u8 {
        // Align size to pointer width.
        let mask = mem::align_of::<usize>() - 1;
        size = (size + mask) & !mask;

        let mut current = self.large_head;
        while !current.is_null() {
            unsafe {
                if (*current).free && (*current).size >= size {
                    // Split if there is enough room for a new header + ≥16 bytes.
                    if (*current).size >= size + LARGE_BLOCK_SIZE + 16 {
                        let next_addr = (current as usize) + LARGE_BLOCK_SIZE + size;
                        let next = next_addr as *mut LargeBlock;

                        (*next).size = (*current).size - size - LARGE_BLOCK_SIZE;
                        (*next).next = (*current).next;
                        (*next).prev = current;
                        (*next).free = true;

                        if !(*next).next.is_null() {
                            (*(*next).next).prev = next;
                        }

                        (*current).size = size;
                        (*current).next = next;
                    }

                    (*current).free = false;
                    return ((current as usize) + LARGE_BLOCK_SIZE) as *mut u8;
                }
                current = (*current).next;
            }
        }
        ptr::null_mut()
    }

    /// Free a raw payload pointer obtained from `alloc_large_raw`.
    unsafe fn free_large_raw(&mut self, ptr: *mut u8) {
        if ptr.is_null() {
            return;
        }
        let block = (ptr as usize - LARGE_BLOCK_SIZE) as *mut LargeBlock;
        unsafe {
            if (*block).free {
                return; // guard against double-free
            }
            (*block).free = true;

            // Coalesce with next.
            let next = (*block).next;
            if !next.is_null() && (*next).free {
                (*block).size += (*next).size + LARGE_BLOCK_SIZE;
                (*block).next = (*next).next;
                if !(*block).next.is_null() {
                    (*(*block).next).prev = block;
                }
            }

            // Coalesce with prev.
            let prev = (*block).prev;
            if !prev.is_null() && (*prev).free {
                (*prev).size += (*block).size + LARGE_BLOCK_SIZE;
                (*prev).next = (*block).next;
                if !(*block).next.is_null() {
                    (*(*block).next).prev = prev;
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Public allocation interface (used by KernelAllocator)
    // -----------------------------------------------------------------------

    /// Allocate `size` bytes with *at least* `align` alignment.
    ///
    /// For small requests a one-word header stores the bucket index.
    /// For large requests the OffsetHeader scheme stores the large-block ptr.
    pub unsafe fn alloc_aligned(&mut self, size: usize, align: usize) -> *mut u8 {
        // ── Small path ──────────────────────────────────────────────────────
        // We only use the small path when alignment ≤ SMALL_HEADER_SIZE,
        // because BUCKET_SIZES are aligned to their own size (power-of-two ≥ 8)
        // and the payload starts exactly SMALL_HEADER_SIZE bytes into the block.
        if align <= SMALL_HEADER_SIZE {
            if let Some(bucket) = Self::bucket_for(size) {
                return unsafe { self.alloc_small(bucket) };
            }
        }

        // ── Large path (also handles over-aligned small requests) ────────────
        // Layout: [LargeBlock] … padding … [sentinel:usize][block_addr:usize][payload…]
        let two_words = 2 * mem::size_of::<usize>();
        let extra = align + two_words; // worst-case padding + two-word prefix
        let raw = unsafe { self.alloc_large_raw(size + extra) };
        if raw.is_null() {
            return ptr::null_mut();
        }

        let raw_addr = raw as usize;
        let block_addr = raw_addr - LARGE_BLOCK_SIZE;

        // Find the first address ≥ raw_addr + two_words that is `align`-aligned.
        let candidate = raw_addr + two_words;
        let remainder = candidate % align;
        let padding = if remainder == 0 { 0 } else { align - remainder };
        let aligned_addr = candidate + padding;

        // Write two-word prefix just before the payload.
        let sentinel_ptr = (aligned_addr - two_words) as *mut usize;
        unsafe {
            ptr::write(sentinel_ptr, LARGE_BLOCK_SENTINEL);
            ptr::write(sentinel_ptr.add(1), block_addr);
        }

        aligned_addr as *mut u8
    }

    /// Deallocate a pointer previously returned by `alloc_aligned`.
    pub unsafe fn dealloc_aligned(&mut self, ptr: *mut u8) {
        if ptr.is_null() {
            return;
        }
        let addr = ptr as usize;
        let one_word = mem::size_of::<usize>();

        // Check small-block header first (1 word back) — safe even for the
        // first allocation in the heap because the header is within the heap.
        let small_val = unsafe { ptr::read((addr - one_word) as *const usize) };

        if small_val < NUM_BUCKETS {
            // Small block: the word before the payload holds the bucket index.
            unsafe { self.free_small(ptr) };
        } else {
            // Must be a large block: sentinel + block_addr live 2 words back.
            let block_addr = unsafe { ptr::read((addr - one_word) as *const usize) };
            let raw_ptr = (block_addr + LARGE_BLOCK_SIZE) as *mut u8;
            unsafe { self.free_large_raw(raw_ptr) };
        }
    }

    /// Return the usable capacity of a live allocation (for realloc).
    pub unsafe fn capacity_of(&self, ptr: *mut u8) -> usize {
        let addr = ptr as usize;
        let one_word = mem::size_of::<usize>();

        // Check small-block header first (1 word back).
        let small_val = unsafe { ptr::read((addr - one_word) as *const usize) };

        if small_val < NUM_BUCKETS {
            // Small block: capacity is the full class size.
            BUCKET_SIZES[small_val]
        } else {
            // Large block: block_addr at addr - 1 word.
            let block_addr = small_val;
            let block = block_addr as *mut LargeBlock;
            let block_end = block_addr + LARGE_BLOCK_SIZE + unsafe { (*block).size };
            block_end - addr
        }
    }
}

// ---------------------------------------------------------------------------
// Utility
// ---------------------------------------------------------------------------

#[inline]
fn align_up(addr: usize, align: usize) -> usize {
    (addr + align - 1) & !(align - 1)
}

// ---------------------------------------------------------------------------
// Spinlock (unchanged from original)
// ---------------------------------------------------------------------------

pub struct Spinlock<T> {
    lock: AtomicBool,
    data: UnsafeCell<T>,
}

unsafe impl<T: Send> Sync for Spinlock<T> {}
unsafe impl<T: Send> Send for Spinlock<T> {}

impl<T> Spinlock<T> {
    pub const fn new(data: T) -> Self {
        Self {
            lock: AtomicBool::new(false),
            data: UnsafeCell::new(data),
        }
    }

    pub fn lock(&self) -> SpinlockGuard<T> {
        while self
            .lock
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            core::hint::spin_loop();
        }
        SpinlockGuard { lock: self }
    }
}

pub struct SpinlockGuard<'a, T> {
    lock: &'a Spinlock<T>,
}

impl<'a, T> Deref for SpinlockGuard<'a, T> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &*self.lock.data.get() }
    }
}

impl<'a, T> DerefMut for SpinlockGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.lock.data.get() }
    }
}

impl<'a, T> Drop for SpinlockGuard<'a, T> {
    fn drop(&mut self) {
        self.lock.lock.store(false, Ordering::Release);
    }
}

// ---------------------------------------------------------------------------
// Global allocator
// ---------------------------------------------------------------------------

pub struct KernelAllocator;

static INNER_ALLOCATOR: Spinlock<SegregatedAllocator> = Spinlock::new(SegregatedAllocator::new());

#[global_allocator]
static ALLOCATOR: KernelAllocator = KernelAllocator;

// ---------------------------------------------------------------------------
// User-mode heap allocator
// ---------------------------------------------------------------------------
//
// This is a completely separate heap from the kernel's INNER_ALLOCATOR.
// Its backing pages are mapped with PAGE_USER set, so the userspace task can
// actually dereference pointers it gets back from sys_alloc/sys_realloc.
// sys_alloc/sys_free/sys_realloc in syscall.rs must go through this
// allocator, NOT through alloc_aligned/dealloc_aligned (those operate on
// INNER_ALLOCATOR, whose pages are kernel-only).

pub static USER_ALLOCATOR: Spinlock<SegregatedAllocator> = Spinlock::new(SegregatedAllocator::new());

/// Initialise the user heap. `start`/`size` must describe a region that has
/// already been mapped into the page tables with PAGE_USER | PAGE_WRITABLE.
pub unsafe fn init_user_heap(start: usize, size: usize) {
    unsafe { USER_ALLOCATOR.lock().init(start, size) };
}

/// Allocate from the user heap. Used by sys_alloc.
pub unsafe fn user_heap_alloc(layout: Layout) -> *mut u8 {
    unsafe { USER_ALLOCATOR.lock().alloc_aligned(layout.size(), layout.align()) }
}

/// Free a pointer previously returned by `user_heap_alloc`. Used by sys_free.
pub unsafe fn user_heap_free(ptr: *mut u8) {
    if ptr.is_null() {
        return;
    }
    unsafe { USER_ALLOCATOR.lock().dealloc_aligned(ptr) };
}

/// Reallocate a pointer previously returned by `user_heap_alloc`. Used by sys_realloc.
pub unsafe fn user_heap_realloc(ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
    if ptr.is_null() {
        return unsafe {
            user_heap_alloc(Layout::from_size_align_unchecked(new_size, layout.align()))
        };
    }

    let new_ptr = unsafe {
        user_heap_alloc(Layout::from_size_align_unchecked(new_size, layout.align()))
    };

    if !new_ptr.is_null() {
        let old_cap = unsafe { USER_ALLOCATOR.lock().capacity_of(ptr) };
        let copy_size = new_size.min(old_cap);
        unsafe {
            ptr::copy_nonoverlapping(ptr, new_ptr, copy_size);
            user_heap_free(ptr);
        }
    }

    new_ptr
}

// ---------------------------------------------------------------------------
// Privilege-level detection & user-mode syscall stubs (unchanged)
// ---------------------------------------------------------------------------

#[inline(always)]
fn get_cpl() -> u16 {
    let cs: u16;
    unsafe {
        core::arch::asm!("mov {0:x}, cs", out(reg) cs);
    }
    cs & 0x03
}

#[inline(always)]
unsafe fn user_alloc(layout: Layout) -> *mut u8 {
    let ret: usize;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") 1usize,
            in("rdi") layout.size(),
            in("rsi") layout.align(),
            lateout("rax") ret,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack, preserves_flags)
        );
    }
    ret as *mut u8
}

#[inline(always)]
unsafe fn user_free(ptr: *mut u8) {
    let _ret: usize;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") 2usize,
            in("rdi") ptr,
            lateout("rax") _ret,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack, preserves_flags)
        );
    }
}

#[inline(always)]
unsafe fn user_realloc(ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
    let ret: usize;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") 10usize,
            in("rdi") ptr,
            in("rsi") new_size,
            in("rdx") layout.align(),
            lateout("rax") ret,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack, preserves_flags)
        );
    }
    ret as *mut u8
}

// ---------------------------------------------------------------------------
// GlobalAlloc implementation
// ---------------------------------------------------------------------------

unsafe impl GlobalAlloc for KernelAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if get_cpl() == 3 {
            return unsafe { user_alloc(layout) };
        }
        unsafe {
            INNER_ALLOCATOR
                .lock()
                .alloc_aligned(layout.size(), layout.align())
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        if ptr.is_null() {
            return;
        }
        if get_cpl() == 3 {
            unsafe { user_free(ptr) };
            return;
        }
        unsafe { INNER_ALLOCATOR.lock().dealloc_aligned(ptr) };
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if get_cpl() == 3 {
            return unsafe { user_realloc(ptr, layout, new_size) };
        }

        if ptr.is_null() {
            return unsafe {
                self.alloc(Layout::from_size_align_unchecked(new_size, layout.align()))
            };
        }

        let new_ptr =
            unsafe { self.alloc(Layout::from_size_align_unchecked(new_size, layout.align())) };

        if !new_ptr.is_null() {
            let old_cap = unsafe { INNER_ALLOCATOR.lock().capacity_of(ptr) };
            let copy_size = new_size.min(old_cap);
            unsafe {
                ptr::copy_nonoverlapping(ptr, new_ptr, copy_size);
                self.dealloc(ptr, layout);
            }
        }

        new_ptr
    }
}

// ---------------------------------------------------------------------------
// Public kernel API (mirrors original interface exactly)
// ---------------------------------------------------------------------------

/// Initialise the heap.
pub unsafe fn init(start: usize, size: usize) {
    unsafe { INNER_ALLOCATOR.lock().init(start, size) };
}

/// Allocate `size` bytes (8-byte alignment).
pub unsafe fn alloc(size: usize) -> *mut u8 {
    let layout = Layout::from_size_align(size, 8).unwrap();
    unsafe { KernelAllocator.alloc(layout) }
}

/// Allocate with a specific layout.
pub unsafe fn alloc_aligned(layout: Layout) -> *mut u8 {
    unsafe { KernelAllocator.alloc(layout) }
}

/// Free a pointer.
pub unsafe fn free(ptr: *mut u8) {
    let layout = Layout::from_size_align(1, 1).unwrap();
    unsafe { KernelAllocator.dealloc(ptr, layout) };
}

/// Reallocate with 8-byte alignment.
pub unsafe fn realloc(ptr: *mut u8, size: usize) -> *mut u8 {
    let layout = Layout::from_size_align(1, 8).unwrap();
    unsafe { KernelAllocator.realloc(ptr, layout, size) }
}

/// Reallocate with a specific layout.
pub unsafe fn realloc_aligned(ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
    unsafe { KernelAllocator.realloc(ptr, layout, new_size) }
}