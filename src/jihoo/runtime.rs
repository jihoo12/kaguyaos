use core::alloc::Layout;
use core::sync::atomic::{AtomicUsize, Ordering};

static LIVE: AtomicUsize = AtomicUsize::new(0);

unsafe extern "C" {
    fn jihoo_runtime_check() -> u64;
}

#[unsafe(no_mangle)]
unsafe extern "C" fn kaguya_runtime_alloc(size: usize, alignment: usize) -> *mut u8 {
    let Ok(layout) = Layout::from_size_align(size.max(1), alignment) else {
        return core::ptr::null_mut();
    };
    let pointer = unsafe { crate::memory::heap::alloc_aligned(layout) };
    if !pointer.is_null() {
        LIVE.fetch_add(1, Ordering::Relaxed);
    }
    pointer
}

#[unsafe(no_mangle)]
unsafe extern "C" fn kaguya_runtime_realloc(
    pointer: *mut u8,
    size: usize,
    alignment: usize,
) -> *mut u8 {
    if pointer.is_null() {
        return unsafe { kaguya_runtime_alloc(size, alignment) };
    }
    let Ok(layout) = Layout::from_size_align(1, alignment) else {
        return core::ptr::null_mut();
    };
    unsafe { crate::memory::heap::realloc_aligned(pointer, layout, size.max(1)) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn kaguya_runtime_free(pointer: *mut u8) {
    if pointer.is_null() {
        return;
    }
    unsafe { crate::memory::heap::free(pointer) };
    LIVE.fetch_sub(1, Ordering::Relaxed);
}

#[unsafe(no_mangle)]
unsafe extern "C" fn kaguya_runtime_write(fd: i32, bytes: *const u8, length: usize) -> i64 {
    if fd != 1 && fd != 2 {
        return -1;
    }
    let data = unsafe { core::slice::from_raw_parts(bytes, length) };
    let text = core::str::from_utf8(data).unwrap_or("<invalid UTF-8>");
    crate::print!("{}", text);
    length as i64
}

pub fn check() {
    let baseline = LIVE.load(Ordering::Relaxed);
    assert_eq!(unsafe { jihoo_runtime_check() }, 8128);
    assert_eq!(
        LIVE.load(Ordering::Relaxed),
        baseline,
        "jihoo collection allocation leak"
    );
    crate::println!("jihoo kernel runtime: collections and cleanup passed");
}
