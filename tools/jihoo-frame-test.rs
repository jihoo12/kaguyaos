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
}
