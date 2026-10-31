use std::alloc::{Layout, alloc, dealloc, realloc};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

fn allocations() -> std::sync::MutexGuard<'static, HashMap<usize, Layout>> {
    static ALLOCATIONS: OnceLock<Mutex<HashMap<usize, Layout>>> = OnceLock::new();
    ALLOCATIONS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap()
}

#[unsafe(no_mangle)]
unsafe extern "C" fn kaguya_runtime_alloc(size: usize, alignment: usize) -> *mut u8 {
    let layout = Layout::from_size_align(size.max(1), alignment).unwrap();
    let pointer = unsafe { alloc(layout) };
    assert!(!pointer.is_null());
    assert_eq!(pointer as usize % 16, 0);
    assert!(allocations().insert(pointer as usize, layout).is_none());
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
    let old = allocations().remove(&(pointer as usize)).unwrap();
    assert_eq!(old.align(), alignment);
    let next = unsafe { realloc(pointer, old, size.max(1)) };
    assert!(!next.is_null());
    assert!(
        allocations()
            .insert(
                next as usize,
                Layout::from_size_align(size.max(1), alignment).unwrap()
            )
            .is_none()
    );
    next
}

#[unsafe(no_mangle)]
unsafe extern "C" fn kaguya_runtime_free(pointer: *mut u8) {
    let layout = allocations().remove(&(pointer as usize)).unwrap();
    unsafe { dealloc(pointer, layout) };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn kaguya_runtime_write(fd: i32, bytes: *const u8, length: usize) -> i64 {
    assert!(fd == 1 || fd == 2);
    let text = unsafe { std::slice::from_raw_parts(bytes, length) };
    eprint!("{}", String::from_utf8_lossy(text));
    length as i64
}

#[unsafe(no_mangle)]
unsafe extern "C" fn kaguya_kernel_panic(bytes: *const u8, length: usize) -> ! {
    let text = unsafe { std::slice::from_raw_parts(bytes, length) };
    eprintln!("{}", String::from_utf8_lossy(text));
    std::process::abort()
}

unsafe extern "C" {
    fn jihoo_runtime_check() -> u64;
}

fn main() {
    for _ in 0..20 {
        assert_eq!(unsafe { jihoo_runtime_check() }, 8128);
        assert!(
            allocations().is_empty(),
            "jihoo destructors leaked allocations"
        );
    }
    println!(
        "kernel std.sys: Vec growth/clone/sort, Map<String, Box>, String reallocation and complete cleanup passed"
    );
}
