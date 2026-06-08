use core::mem::size_of;

// Constants for GDT selectors
pub const KERNEL_CODE_SEL: u16 = 0x08;
pub const KERNEL_DATA_SEL: u16 = 0x10;
pub const USER_DATA_SEL: u16 = 0x18 | 3;
pub const USER_CODE_SEL: u16 = 0x20 | 3;
pub const TSS_SEL: u16 = 0x28;

pub const DOUBLE_FAULT_IST_INDEX: u16 = 1;

#[repr(align(16))]
struct AlignedStack([u8; 4096]);

static mut DOUBLE_FAULT_STACK: AlignedStack = AlignedStack([0; 4096]);

#[derive(Copy, Clone)]
#[repr(C, packed)]
struct GdtEntry {
    limit_low: u16,
    base_low: u16,
    base_middle: u8,
    access: u8,
    granularity: u8,
    base_high: u8,
}

#[derive(Copy, Clone)]
#[repr(C, packed)]
struct GdtSystemEntry {
    limit_low: u16,
    base_low: u16,
    base_middle: u8,
    access: u8,
    granularity: u8,
    base_high: u8,
    base_upper: u32,
    reserved: u32,
}

#[derive(Copy, Clone)]
#[repr(C, packed)]
struct GdtPointer {
    limit: u16,
    base: u64,
}

#[derive(Copy, Clone)]
#[repr(C, packed)]
pub struct Tss {
    reserved1: u32,
    rsp0: u64,
    rsp1: u64,
    rsp2: u64,
    reserved2: u64,
    ist1: u64,
    ist2: u64,
    ist3: u64,
    ist4: u64,
    ist5: u64,
    ist6: u64,
    ist7: u64,
    reserved3: u64,
    reserved4: u16,
    iomap_base: u16,
}

static mut TSS: Tss = Tss {
    reserved1: 0,
    rsp0: 0,
    rsp1: 0,
    rsp2: 0,
    reserved2: 0,
    ist1: 0,
    ist2: 0,
    ist3: 0,
    ist4: 0,
    ist5: 0,
    ist6: 0,
    ist7: 0,
    reserved3: 0,
    reserved4: 0,
    iomap_base: 0,
};

static mut GDT: [GdtEntry; 7] = [GdtEntry {
    limit_low: 0,
    base_low: 0,
    base_middle: 0,
    access: 0,
    granularity: 0,
    base_high: 0,
}; 7];

static mut GDT_PTR: GdtPointer = GdtPointer { limit: 0, base: 0 };

unsafe fn set_gdt_entry(index: usize, base: u32, limit: u32, access: u8, gran: u8) {
    unsafe {
        GDT[index].base_low = (base & 0xFFFF) as u16;
        GDT[index].base_middle = ((base >> 16) & 0xFF) as u8;
        GDT[index].base_high = ((base >> 24) & 0xFF) as u8;

        GDT[index].limit_low = (limit & 0xFFFF) as u16;
        GDT[index].granularity = ((limit >> 16) & 0x0F) as u8;

        GDT[index].granularity |= gran & 0xF0;
        GDT[index].access = access;
    }
}

unsafe fn set_gdt_system_entry(index: usize, base: u64, limit: u32, access: u8, gran: u8) {
    unsafe {
        set_gdt_entry(index, base as u32, limit, access, gran);
    }

    let high_base_offset = (core::ptr::addr_of!(GDT) as u64) + ((index + 1) * 8) as u64;
    let high_base_ptr = high_base_offset as *mut u32;

    unsafe {
        *high_base_ptr = (base >> 32) as u32;
        *high_base_ptr.add(1) = 0;
    }
}

pub unsafe fn init() {
    unsafe {
        // Clear TSS
        // Rust static initialization already zeroes it, but we set iomap_base
        TSS.iomap_base = size_of::<Tss>() as u16;

        // Set up IST1 for Double Fault
        let stack_ptr = core::ptr::addr_of_mut!(DOUBLE_FAULT_STACK) as *const u8;
        let stack_top = stack_ptr.add(4096) as u64;
        TSS.ist1 = stack_top;

        // Null descriptor
        set_gdt_entry(0, 0, 0, 0, 0);

        // Kernel Code Segment: Access 0x9A, Granularity 0xAF (64-bit)
        set_gdt_entry(1, 0, 0xFFFFFFFF, 0x9A, 0xAF);

        // Kernel Data Segment: Access 0x92, Granularity 0xCF
        set_gdt_entry(2, 0, 0xFFFFFFFF, 0x92, 0xCF);

        // User Data Segment: Access 0xF2 (Present, Ring 3, Data, Writable)
        set_gdt_entry(3, 0, 0xFFFFFFFF, 0xF2, 0xCF);

        // User Code Segment: Access 0xFA (Present, Ring 3, Code, Readable)
        set_gdt_entry(4, 0, 0xFFFFFFFF, 0xFA, 0xAF);

        // TSS Segment: Access 0x89 (Present, Ring 0, Available TSS)
        set_gdt_system_entry(
            5,
            &raw const TSS as *const _ as u64,
            (size_of::<Tss>() - 1) as u32,
            0x89,
            0x00,
        );

        GDT_PTR.limit = (size_of::<[GdtEntry; 7]>() - 1) as u16;
        GDT_PTR.base = &raw const GDT as *const _ as u64;

        // Load GDT
        core::arch::asm!(
            "lgdt [{}]",
            in(reg) &raw const GDT_PTR,
            options(readonly, nostack, preserves_flags)
        );

        // Reload segments
        core::arch::asm!(
            "push {sel}",
            "lea {tmp}, [2f + rip]",
            "push {tmp}",
            "retfq",
            "2:",
            "mov ds, {data_sel:e}",
            "mov es, {data_sel:e}",
            "mov fs, {data_sel:e}",
            "mov gs, {data_sel:e}",
            "mov ss, {data_sel:e}",
            sel = in(reg) KERNEL_CODE_SEL as u64,
            data_sel = in(reg) KERNEL_DATA_SEL,
            tmp = out(reg) _,
        );

        // Load Task Register
        core::arch::asm!(
            "ltr {0:x}",
            in(reg) TSS_SEL,
            options(nostack, preserves_flags)
        );
    }
}

pub unsafe fn set_tss_stack(stack: u64) {
    unsafe {
        TSS.rsp0 = stack;
    }
}

pub unsafe fn get_tss_stack() -> u64 {
    unsafe { TSS.rsp0 }
}