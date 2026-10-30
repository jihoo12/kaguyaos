use super::{FrameSource, PAGE_PRESENT, PAGE_USER, PAGE_WRITABLE, PageTable, get_table_mut};

/// Maps a virtual address to a physical address.
pub unsafe fn map_page(
    pml4: &mut PageTable,
    virt_addr: u64,
    phys_addr: u64,
    flags: u64,
    allocator: &mut impl FrameSource,
) {
    let pml4_idx = ((virt_addr >> 39) & 0x1FF) as usize;
    let pdp_idx = ((virt_addr >> 30) & 0x1FF) as usize;
    let pd_idx = ((virt_addr >> 21) & 0x1FF) as usize;
    let pt_idx = ((virt_addr >> 12) & 0x1FF) as usize;

    // 1. Get PDPT
    if (pml4.entries[pml4_idx] & PAGE_PRESENT) == 0 {
        let frame = allocator.allocate_frame().expect("OOM allocating PDPT");
        let table = unsafe { get_table_mut(frame) };
        table.zero();
        pml4.entries[pml4_idx] = frame | PAGE_PRESENT | PAGE_WRITABLE | PAGE_USER;
    }
    assert_eq!(pml4.entries[pml4_idx] & 128, 0, "invalid PML4 entry");
    let pdpt_phys = pml4.entries[pml4_idx] & 0x000f_ffff_ffff_f000;
    let pdpt = unsafe { get_table_mut(pdpt_phys) };

    // 2. Get PD
    if (pdpt.entries[pdp_idx] & PAGE_PRESENT) == 0 {
        let frame = allocator.allocate_frame().expect("OOM allocating PD");
        let table = unsafe { get_table_mut(frame) };
        table.zero();
        pdpt.entries[pdp_idx] = frame | PAGE_PRESENT | PAGE_WRITABLE | PAGE_USER;
    }
    assert_eq!(
        pdpt.entries[pdp_idx] & 128,
        0,
        "cannot replace a 1 GiB page"
    );
    let pd_phys = pdpt.entries[pdp_idx] & 0x000f_ffff_ffff_f000;
    let pd = unsafe { get_table_mut(pd_phys) };

    // 3. Get PT
    if (pd.entries[pd_idx] & PAGE_PRESENT) == 0 {
        let frame = allocator.allocate_frame().expect("OOM allocating PT");
        let table = unsafe { get_table_mut(frame) };
        table.zero();
        pd.entries[pd_idx] = frame | PAGE_PRESENT | PAGE_WRITABLE | PAGE_USER;
    }
    assert_eq!(pd.entries[pd_idx] & 128, 0, "cannot replace a 2 MiB page");
    let pt_phys = pd.entries[pd_idx] & 0x000f_ffff_ffff_f000;
    let pt = unsafe { get_table_mut(pt_phys) };

    // 4. Map Page
    pt.entries[pt_idx] = phys_addr | flags | PAGE_PRESENT;

    // 5. Flush TLB entry for this virtual address
    unsafe {
        core::arch::asm!("invlpg [{}]", in(reg) virt_addr, options(nostack, preserves_flags));
    }
}

pub fn is_range_mapped(pml4: &PageTable, start: u64, len: u64) -> bool {
    range_mapped(pml4, start, len, PAGE_PRESENT)
}

pub fn is_user_range_mapped(pml4: &PageTable, start: u64, len: u64) -> bool {
    range_mapped(pml4, start, len, PAGE_PRESENT | PAGE_USER)
}

fn range_mapped(pml4: &PageTable, start: u64, len: u64, required: u64) -> bool {
    if len == 0 {
        return true;
    }
    let Some(end) = start.checked_add(len) else {
        return false;
    };
    let region = start >> 47;
    if region != 0 && region != 0x1ffff {
        return false;
    }
    if (end - 1) >> 47 != region {
        return false;
    }
    let mut address = start & !0xfff;
    let last = (end - 1) & !0xfff;
    loop {
        if !page_present(pml4, address, required) {
            return false;
        }
        if address == last {
            return true;
        }
        address += 4096;
    }
}

fn page_present(pml4: &PageTable, address: u64, required: u64) -> bool {
    let mut table = pml4;
    for shift in [39, 30, 21, 12] {
        let entry = table.entries[((address >> shift) & 511) as usize];
        if entry & required != required {
            return false;
        }
        if shift == 12 {
            return true;
        }
        if entry & (1 << 7) != 0 {
            return shift != 39;
        }
        table = unsafe { get_table_mut(entry & 0x000f_ffff_ffff_f000) };
    }
    false
}
