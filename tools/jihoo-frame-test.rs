unsafe extern "C" {
    fn jihoo_boot_frame_alloc(
        map: *const u8,
        size: usize,
        stride: usize,
        index: *mut usize,
        offset: *mut u64,
    ) -> u64;
    fn jihoo_boot_frames_alloc(
        map: *const u8,
        size: usize,
        stride: usize,
        index: *mut usize,
        offset: *mut u64,
        count: usize,
    ) -> u64;
    fn jihoo_pool_commit(map: *const u8, size: usize, stride: usize, index: usize, offset: u64);
    fn jihoo_pool_alloc() -> u64;
    fn jihoo_pool_free(frame: u64);
    fn jihoo_pool_in_use() -> usize;
    fn jihoo_frame_zero(frame: u64);
}

#[repr(C, align(4096))]
#[derive(Clone)]
struct Page([u8; 4096]);

fn pool_test() {
    let mut backing = vec![Page([0xd1; 4096]); 11];
    let base = backing[1].0.as_mut_ptr() as u64;
    let mut map = [0u8; 80];
    map[40..44].copy_from_slice(&7u32.to_le_bytes());
    map[48..56].copy_from_slice(&base.to_le_bytes());
    map[64..72].copy_from_slice(&9u64.to_le_bytes());
    unsafe {
        assert_eq!(jihoo_pool_alloc(), 0, "pool is unavailable before commit");
        assert_eq!(jihoo_pool_in_use(), 0);
        jihoo_pool_commit(map.as_ptr(), map.len(), 40, 1, 2);
        let mut live = Vec::new();
        for i in 2..9 {
            let frame = jihoo_pool_alloc();
            assert_eq!(frame, base + i * 4096);
            jihoo_frame_zero(frame);
            let page = std::slice::from_raw_parts_mut(frame as *mut u8, 4096);
            assert!(page.iter().all(|&b| b == 0));
            page.fill(i as u8);
            live.push(frame);
            assert_eq!(jihoo_pool_in_use(), live.len());
        }
        for _ in 0..3 {
            assert_eq!(jihoo_pool_alloc(), 0);
            assert_eq!(jihoo_pool_in_use(), 7, "OOM must not change accounting");
        }
        for &i in &[1, 5, 3] {
            jihoo_pool_free(live[i]);
        }
        assert_eq!(jihoo_pool_in_use(), 4);
        for &i in &[3, 5, 1] {
            let frame = jihoo_pool_alloc();
            assert_eq!(
                frame, live[i],
                "free frames must be reused before the bump source"
            );
            jihoo_frame_zero(frame);
            let page = std::slice::from_raw_parts_mut(frame as *mut u8, 4096);
            assert!(
                page.iter().all(|&b| b == 0),
                "reused page and next pointer must be cleared"
            );
            page.fill((i + 2) as u8);
        }
        assert_eq!(jihoo_pool_in_use(), 7);
        for (i, &frame) in live.iter().enumerate() {
            let page = std::slice::from_raw_parts(frame as *const u8, 4096);
            assert!(
                page.iter().all(|&b| b == (i + 2) as u8),
                "live frame corrupted"
            );
        }
        for &frame in &live {
            jihoo_pool_free(frame);
        }
        assert_eq!(jihoo_pool_in_use(), 0);
        for &expected in live.iter().rev() {
            let frame = jihoo_pool_alloc();
            assert_eq!(frame, expected);
            jihoo_frame_zero(frame);
            assert!(
                std::slice::from_raw_parts(frame as *const u8, 4096)
                    .iter()
                    .all(|&b| b == 0)
            );
        }
        for &frame in &live {
            jihoo_pool_free(frame);
        }
        assert_eq!(jihoo_pool_in_use(), 0);
    }
    for i in [0, 1, 2, 10] {
        assert!(
            backing[i].0.iter().all(|&b| b == 0xd1),
            "guard or boot-owned frame overwritten"
        );
    }
    println!("jihoo frame pool: boot handoff, zeroing, LIFO reuse, OOM and accounting passed");
}

fn main() {
    // UEFI permits descriptors larger than the current 40-byte structure.
    for stride in [40, 48, 64] {
        let entries = [
            (7u32, 0u64, 1u64),
            (0, 0x1000, 4),
            (7, 0x5000, 2),
            (7, 0x7000, 0),
            (3, 0x7000, 1),
            (7, 0x8000, 5),
            (7, 0xd000, 3),
        ];
        let mut map = vec![0xccu8; entries.len() * stride + 16];
        for (i, (kind, start, pages)) in entries.into_iter().enumerate() {
            let desc = &mut map[i * stride..];
            desc[0..4].copy_from_slice(&kind.to_le_bytes());
            desc[8..16].copy_from_slice(&start.to_le_bytes());
            desc[24..32].copy_from_slice(&pages.to_le_bytes());
        }
        let original = map.clone();
        let mut index = 0;
        let mut offset = 0;
        unsafe {
            for expected in [
                0x5000, 0x6000, 0x8000, 0x9000, 0xa000, 0xb000, 0xc000, 0xd000, 0xe000, 0xf000, 0,
                0,
            ] {
                assert_eq!(
                    jihoo_boot_frame_alloc(
                        map.as_ptr(),
                        map.len(),
                        stride,
                        &mut index,
                        &mut offset
                    ),
                    expected
                );
            }
            assert_eq!((index, offset), (entries.len(), 0));
            index = 0;
            offset = 0;
            assert_eq!(
                jihoo_boot_frames_alloc(
                    map.as_ptr(),
                    map.len(),
                    stride,
                    &mut index,
                    &mut offset,
                    0
                ),
                0
            );
            assert_eq!((index, offset), (0, 0));
            // Skip a two-frame fragment; the eight frames across the last
            // two descriptors are physically contiguous.
            assert_eq!(
                jihoo_boot_frames_alloc(
                    map.as_ptr(),
                    map.len(),
                    stride,
                    &mut index,
                    &mut offset,
                    8
                ),
                0x8000
            );
            assert_eq!(
                jihoo_boot_frame_alloc(map.as_ptr(), map.len(), stride, &mut index, &mut offset),
                0
            );
            index = 0;
            offset = 0;
            assert_eq!(
                jihoo_boot_frames_alloc(
                    map.as_ptr(),
                    map.len(),
                    stride,
                    &mut index,
                    &mut offset,
                    9
                ),
                0
            );
            assert_eq!(
                jihoo_boot_frames_alloc(map.as_ptr(), 0, stride, &mut index, &mut offset, 1),
                0
            );
        }
        assert_eq!(map, original, "firmware map must remain read-only");
    }
    println!(
        "jihoo boot frames: descriptor strides, reserved/empty/zero pages, exhaustion and contiguous runs passed"
    );
    pool_test();
}
