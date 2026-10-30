use super::FrameAllocator;
use crate::sync::Spinlock;

unsafe extern "C" {
    fn jihoo_pool_commit(map: *const u8, size: usize, stride: usize, index: usize, offset: u64);
    fn jihoo_pool_alloc() -> u64;
    fn jihoo_pool_free(frame: u64);
    fn jihoo_pool_in_use() -> usize;
    fn jihoo_frame_zero(frame: u64);
}

static FRAMES: Spinlock<()> = Spinlock::new(());

/// Transfer the boot allocator's remaining frames. Do not use it afterwards.
pub fn commit_frame_allocator(alloc: &FrameAllocator) {
    let _guard = FRAMES.lock();
    unsafe {
        jihoo_pool_commit(
            alloc.memory_map,
            alloc.memory_map_size,
            alloc.descriptor_size,
            alloc.current_descriptor_index,
            alloc.current_page_offset,
        );
    }
}

pub fn alloc_frame() -> Option<u64> {
    let frame = {
        let _guard = FRAMES.lock();
        unsafe { jihoo_pool_alloc() }
    };
    if frame == 0 {
        return None;
    }
    unsafe { jihoo_frame_zero(frame) };
    Some(frame)
}

/// Return a live frame obtained from `alloc_frame` exactly once.
pub fn free_frame(frame: u64) {
    let _guard = FRAMES.lock();
    unsafe { jihoo_pool_free(frame) };
}

pub fn frames_in_use() -> usize {
    let _guard = FRAMES.lock();
    unsafe { jihoo_pool_in_use() }
}
