#![allow(dead_code)]

const PAGE_PRESENT: u64 = 1;
const PAGE_WRITABLE: u64 = 2;
const PAGE_USER: u64 = 4;
const NX: u64 = 1 << 63;
const FRAME_MASK: u64 = 0x000f_ffff_ffff_f000;
const BASE: u64 = 0x4000_0000_0000;

#[repr(C, align(4096))]
struct PageTable {
    entries: [u64; 512],
}

impl PageTable {
    fn new(value: u64) -> Box<Self> {
        Box::new(Self {
            entries: [value; 512],
        })
    }
    fn zero(&mut self) {
        self.entries.fill(0);
    }
    fn pointer(&self) -> u64 {
        self.entries.as_ptr() as u64
    }
}

unsafe fn get_table_mut(address: u64) -> &'static mut PageTable {
    unsafe { &mut *(address as *mut PageTable) }
}

trait FrameSource {
    fn allocate_frame(&mut self) -> Option<u64>;
}

#[path = "../src/memory/paging.rs"]
mod reference;

unsafe extern "C" {
    fn jihoo_page_zero(table: *mut u64);
    fn jihoo_page_map(
        root: *mut u64,
        address: u64,
        physical: u64,
        flags: u64,
        context: *mut u8,
        allocate: extern "C" fn(*mut u8) -> u64,
    ) -> bool;
    fn jihoo_range_mapped(root: *const u64, start: u64, length: u64, user: bool) -> bool;
    fn jihoo_paging_init(
        map: *const u8,
        size: usize,
        stride: usize,
        framebuffer: u64,
        framebuffer_size: usize,
        context: *mut u8,
        allocate: extern "C" fn(*mut u8) -> u64,
    ) -> u64;
}

struct Source {
    frames: Vec<Box<PageTable>>,
    limit: usize,
}

extern "C" fn allocate(context: *mut u8) -> u64 {
    let source = unsafe { &mut *context.cast::<Source>() };
    if source.frames.len() == source.limit {
        return 0;
    }
    let frame = PageTable::new(u64::MAX);
    let address = frame.pointer();
    source.frames.push(frame);
    address
}

fn map(root: &mut PageTable, source: &mut Source, address: u64, physical: u64, flags: u64) -> bool {
    unsafe {
        jihoo_page_map(
            root.entries.as_mut_ptr(),
            address,
            physical,
            flags,
            (source as *mut Source).cast(),
            allocate,
        )
    }
}

fn check(root: &PageTable, start: u64, length: u64, present: bool, user: bool) {
    for require_user in [false, true] {
        let expected = if require_user { user } else { present };
        let actual =
            unsafe { jihoo_range_mapped(root.entries.as_ptr(), start, length, require_user) };
        assert_eq!(
            actual, expected,
            "jihoo: {start:#x} + {length:#x}, user={require_user}"
        );
        let actual = if require_user {
            reference::is_user_range_mapped(root, start, length)
        } else {
            reference::is_range_mapped(root, start, length)
        };
        assert_eq!(
            actual, expected,
            "Rust: {start:#x} + {length:#x}, user={require_user}"
        );
    }
}

fn mapping_and_permissions() {
    let mut root = PageTable::new(u64::MAX);
    unsafe { jihoo_page_zero(root.entries.as_mut_ptr()) };
    assert!(root.entries.iter().all(|&e| e == 0));
    let mut source = Source {
        frames: Vec::new(),
        limit: 32,
    };
    let address = BASE + 0x1234_5000;
    check(&root, address, 1, false, false);
    assert!(map(&mut root, &mut source, address, 0x123000, 6));
    assert_eq!(source.frames.len(), 3);
    assert_eq!(root.entries[128], source.frames[0].pointer() | 7);
    assert_eq!(source.frames[0].entries[0], source.frames[1].pointer() | 7);
    assert_eq!(
        source.frames[1].entries[0x91],
        source.frames[2].pointer() | 7
    );
    assert_eq!(source.frames[2].entries[0x145], 0x123007);
    for frame in &source.frames {
        assert_eq!(
            frame.entries.iter().filter(|&&e| e != 0).count(),
            1,
            "new tables must be zeroed"
        );
    }
    check(&root, address + 123, 4096 - 123, true, true);
    check(&root, address + 4095, 2, false, false);
    assert!(map(&mut root, &mut source, address + 4096, 0x124000, 6));
    assert_eq!(source.frames.len(), 3);
    check(&root, address + 4095, 2, true, true);

    // Permissions are required at every level, not just the final entry.
    for level in 0..4 {
        let slot = match level {
            0 => &mut root.entries[128] as *mut u64,
            1 => &mut source.frames[0].entries[0],
            2 => &mut source.frames[1].entries[0x91],
            _ => &mut source.frames[2].entries[0x145],
        };
        unsafe {
            let saved = *slot;
            *slot &= !PAGE_USER;
            check(&root, address, 1, true, false);
            *slot = saved & !PAGE_PRESENT;
            check(&root, address, 1, false, false);
            *slot = saved | NX;
            check(&root, address, 1, true, true);
            *slot = saved;
        }
    }
    assert!(map(&mut root, &mut source, address, 0x456000, NX | 2 | 128));
    check(&root, address, 1, true, false); // PTE bit 7 is PAT, not a huge-page flag.
    assert_eq!(source.frames[2].entries[0x145], 0x456083 | NX);
    assert!(map(&mut root, &mut source, address, 0x123000, 6));

    for (address, expected_tables) in [
        (BASE + 0x1240_0000, 4),
        (BASE + 0x4000_0000, 6),
        (BASE + (1 << 39), 9),
        (0xffff_9000_1234_5000, 12),
    ] {
        assert!(map(&mut root, &mut source, address, 0x800000, 2));
        assert_eq!(source.frames.len(), expected_tables);
        check(&root, address, 4096, true, false);
    }
    check(&root, address | (1 << 48), 1, false, false);
    check(&root, 0x0000_8000_0000_0000, 1, false, false);
    check(&root, 0x0000_7fff_ffff_f000, 8192, false, false);
    check(&root, u64::MAX - 3, 8, false, false);
    check(&root, u64::MAX, 0, true, true);
    check(&root, 123, 0, true, true);
    assert!(map(
        &mut root,
        &mut source,
        0xffff_ffff_ffff_f000,
        0x900000,
        6
    ));
    check(&root, u64::MAX - 100, 99, true, true); // Must not wrap the page cursor.
}

fn huge_pages() {
    let mut root = PageTable::new(0);
    let mut source = Source {
        frames: Vec::new(),
        limit: 10,
    };
    assert!(map(&mut root, &mut source, BASE, 0x100000, 6));
    source.frames[0].entries[1] = 0x8000_0000 | 0x85; // 1 GiB, no backing table.
    check(&root, BASE + (1 << 30), 12345, true, true);
    let before = source.frames.len();
    assert!(!map(&mut root, &mut source, BASE + (1 << 30), 0x100000, 6));
    assert_eq!(source.frames.len(), before);
    source.frames[1].entries[16] = 0x200_0000 | 0x85; // 2 MiB.
    check(&root, BASE + (16 << 21), 2 << 20, true, true);
    check(&root, BASE + (16 << 21), (2 << 20) + 1, false, false);
    assert!(!map(&mut root, &mut source, BASE + (16 << 21), 0x100000, 6));
    source.frames[1].entries[16] &= !PAGE_USER;
    check(&root, BASE + (16 << 21), 1, true, false);
    root.entries[128] |= 128; // A huge-page flag is invalid in a PML4 entry.
    check(&root, BASE, 1, false, false);
    assert!(!map(&mut root, &mut source, BASE, 0x100000, 6));
}

fn allocation_failure() {
    for limit in 0..3 {
        let mut root = PageTable::new(0);
        let mut source = Source {
            frames: Vec::new(),
            limit,
        };
        assert!(!map(&mut root, &mut source, BASE, 0x100000, 6));
        assert_eq!(source.frames.len(), limit);
        check(&root, BASE, 1, false, false);
        source.limit = 3;
        assert!(map(&mut root, &mut source, BASE, 0x100000, 6));
        assert_eq!(
            source.frames.len(),
            3,
            "retry must reuse the reachable prefix"
        );
        check(&root, BASE, 1, true, true);
    }
}

fn boot_identity_map() {
    let stride = 48;
    let mut descriptors = vec![0u8; 16 * stride];
    for kind in 0u32..16 {
        let descriptor = &mut descriptors[kind as usize * stride..];
        descriptor[..4].copy_from_slice(&kind.to_le_bytes());
        descriptor[8..16].copy_from_slice(&(kind as u64 * 0x20000).to_le_bytes());
        let pages: u64 = if kind == 6 { 0 } else { 1 };
        descriptor[24..32].copy_from_slice(&pages.to_le_bytes());
    }
    let saved = descriptors.clone();
    let mut source = Source {
        frames: Vec::new(),
        limit: 64,
    };
    let root = unsafe {
        jihoo_paging_init(
            descriptors.as_ptr(),
            descriptors.len(),
            stride,
            0xfe000000,
            4097,
            (&mut source as *mut Source).cast(),
            allocate,
        )
    };
    assert_ne!(root, 0);
    assert_eq!(root, source.frames[0].pointer());
    let root = unsafe { &*(root as *const PageTable) };
    for kind in 0u32..16 {
        let expected = (1..=7).contains(&kind) || (9..=12).contains(&kind);
        check(
            root,
            kind as u64 * 0x20000,
            4096,
            expected && kind != 6,
            false,
        );
    }
    check(root, 0xfe000000, 8192, true, false);
    check(root, 0xfe002000, 1, false, false);
    assert_eq!(descriptors, saved);
    for limit in 0..4 {
        let mut source = Source {
            frames: Vec::new(),
            limit,
        };
        let root = unsafe {
            jihoo_paging_init(
                descriptors.as_ptr(),
                descriptors.len(),
                stride,
                0xfe000000,
                4097,
                (&mut source as *mut Source).cast(),
                allocate,
            )
        };
        assert_eq!(root, 0);
        assert_eq!(source.frames.len(), limit);
    }
}

fn main() {
    mapping_and_permissions();
    huge_pages();
    allocation_failure();
    boot_identity_map();
    println!(
        "jihoo paging: boot identity map, mapping, callbacks, permissions, huge pages, canonical/overflow boundaries and OOM passed; range checks match Rust"
    );
}
