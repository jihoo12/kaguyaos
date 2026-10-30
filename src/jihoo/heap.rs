use crate::sync::Spinlock;
use core::alloc::{GlobalAlloc, Layout};

unsafe extern "C" {
    fn jihoo_heap_init(start: usize, size: usize);
    fn jihoo_heap_alloc(size: usize, align: usize) -> *mut u8;
    fn jihoo_heap_free(ptr: *mut u8);
    fn jihoo_heap_realloc(ptr: *mut u8, size: usize, align: usize) -> *mut u8;
}

static HEAP_LOCK: Spinlock<()> = Spinlock::new(());

pub struct KernelAllocator;

#[global_allocator]
static ALLOCATOR: KernelAllocator = KernelAllocator;

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

unsafe impl GlobalAlloc for KernelAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if get_cpl() == 3 {
            return unsafe { user_alloc(layout) };
        }
        let _guard = HEAP_LOCK.lock();
        unsafe { jihoo_heap_alloc(layout.size(), layout.align()) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        if ptr.is_null() {
            return;
        }
        if get_cpl() == 3 {
            unsafe { user_free(ptr) };
            return;
        }
        let _guard = HEAP_LOCK.lock();
        unsafe { jihoo_heap_free(ptr) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if get_cpl() == 3 {
            return unsafe { user_realloc(ptr, layout, new_size) };
        }
        let _guard = HEAP_LOCK.lock();
        unsafe { jihoo_heap_realloc(ptr, new_size, layout.align()) }
    }
}

pub unsafe fn init(start: usize, size: usize) {
    {
        let _guard = HEAP_LOCK.lock();
        unsafe { jihoo_heap_init(start, size) };
    }
    crate::println!("Kernel heap initialised at {:#x}, size {}", start, size);
}

pub unsafe fn alloc(size: usize) -> *mut u8 {
    unsafe { KernelAllocator.alloc(Layout::from_size_align(size, 8).unwrap()) }
}

pub unsafe fn alloc_aligned(layout: Layout) -> *mut u8 {
    unsafe { KernelAllocator.alloc(layout) }
}

pub unsafe fn free(ptr: *mut u8) {
    unsafe { KernelAllocator.dealloc(ptr, Layout::from_size_align(1, 1).unwrap()) }
}

pub unsafe fn realloc(ptr: *mut u8, size: usize) -> *mut u8 {
    unsafe { KernelAllocator.realloc(ptr, Layout::from_size_align(1, 8).unwrap(), size) }
}

pub unsafe fn realloc_aligned(ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
    unsafe { KernelAllocator.realloc(ptr, layout, new_size) }
}
