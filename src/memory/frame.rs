use super::{FrameAllocator, PAGE_SIZE};

/// Frames handed out after boot: a free list of released frames, refilled
/// from the boot bump allocator committed by `commit_frame_allocator`.
/// Free frames are identity-mapped, so each stores the next free address.
struct FrameState {
    bump: Option<FrameAllocator>,
    free_head: u64,
    free_count: usize,
    in_use: usize,
}

static FRAMES: crate::sync::Spinlock<FrameState> = crate::sync::Spinlock::new(FrameState {
    bump: None,
    free_head: 0,
    free_count: 0,
    in_use: 0,
});

/// Hand the boot allocator's remaining memory to the global frame pool.
/// The boot allocator must not be used afterwards.
pub fn commit_frame_allocator(alloc: &FrameAllocator) {
    FRAMES.lock().bump = Some(FrameAllocator {
        memory_map: alloc.memory_map,
        memory_map_size: alloc.memory_map_size,
        descriptor_size: alloc.descriptor_size,
        descriptor_version: alloc.descriptor_version,
        current_descriptor_index: alloc.current_descriptor_index,
        current_page_offset: alloc.current_page_offset,
    });
}

/// Allocate one zeroed frame from the global pool.
pub fn alloc_frame() -> Option<u64> {
    let frame = {
        let mut state = FRAMES.lock();
        let frame = if state.free_head != 0 {
            let frame = state.free_head;
            state.free_head = unsafe { *(frame as *const u64) };
            state.free_count -= 1;
            frame
        } else {
            state.bump.as_mut()?.allocate_frame()?
        };
        state.in_use += 1;
        frame
    };
    unsafe { core::ptr::write_bytes(frame as *mut u8, 0, PAGE_SIZE as usize) };
    Some(frame)
}

/// Return a frame obtained from `alloc_frame` to the global pool.
pub fn free_frame(frame: u64) {
    let mut state = FRAMES.lock();
    unsafe { *(frame as *mut u64) = state.free_head };
    state.free_head = frame;
    state.free_count += 1;
    state.in_use -= 1;
}

/// Frames currently allocated from the global pool.
pub fn frames_in_use() -> usize {
    FRAMES.lock().in_use
}
