//! Diskless, single-CPU ring-3 shell bring-up. No device or timer initialization.
use crate::{BootInfo, console, gdt, interrupts, loader, memory, process, syscall};

static INIT: &[u8] = include_bytes!(env!("KAGUYA_INIT_KEF"));

pub fn run(info: &BootInfo) -> ! {
    unsafe {
        core::arch::asm!("cli", options(nomem, nostack));
        console::clear();
        crate::println!("kaguyaOS - HARDWARE SHELL (diskless, storage writes disabled)");
        crate::println!("[1/6] Installing GDT and IDT");
        gdt::init();
        interrupts::init_idt();
        let mut allocator = memory::FrameAllocator::new(info);
        memory::store_boot_info(
            info.memory_map,
            info.memory_map_size,
            info.descriptor_size,
            info.descriptor_version,
        );
        crate::println!("[2/6] Building page tables");
        let pml4 = memory::init_paging(info, &mut allocator);
        syscall::init();
        crate::println!("[3/6] Allocating kernel and user heaps");
        const KERNEL_HEAP: u64 = 0xffff_9000_0000_0000;
        const USER_HEAP: u64 = 0x0000_7000_0000_0000;
        map_buffer(
            pml4,
            &mut allocator,
            KERNEL_HEAP,
            128,
            memory::PAGE_NO_EXECUTE,
        );
        memory::heap::init(KERNEL_HEAP as usize, 128 * 4096);
        map_buffer(
            pml4,
            &mut allocator,
            USER_HEAP,
            128,
            memory::PAGE_USER | memory::PAGE_NO_EXECUTE,
        );
        memory::heap::init_user_heap(USER_HEAP as usize, 128 * 4096);
        crate::println!(
            "[4/6] Allocating GOP surface {}x{}",
            info.horizontal_resolution,
            info.vertical_resolution
        );
        let surface = console::display::init_gop(info, &mut allocator)
            .expect("GOP surface allocation failed");
        console::use_surface(surface);
        console::term::init();
        crate::println!("[5/6] Loading embedded init.kef ({} bytes)", INIT.len());
        process::init();
        let (entry, stack) = loader::load_kef(INIT, &mut allocator, memory::get_table_mut(pml4))
            .expect("embedded init.kef is invalid");
        process::add_new_user_task(entry, stack, 16384, 0, 0);
        memory::commit_frame_allocator(&allocator);
        crate::println!("[6/6] Entering ring 3 on BSP");
        crate::println!("HARDWARE SHELL: no PCI, disks, USB, network, APs or timer interrupts");
        crate::println!("Keyboard input is not enabled in this milestone. Reset to exit.");
        process::enter_bsp_scheduler_idle();
        process::switch_task();
        loop {
            core::arch::asm!("cli; hlt", options(nomem, nostack));
        }
    }
}

unsafe fn map_buffer(
    pml4: u64,
    allocator: &mut memory::FrameAllocator,
    base: u64,
    pages: usize,
    flags: u64,
) {
    unsafe {
        for i in 0..pages {
            let frame = allocator
                .allocate_frame()
                .expect("out of memory allocating heap");
            memory::map_page(
                memory::get_table_mut(pml4),
                base + i as u64 * 4096,
                frame,
                memory::PAGE_WRITABLE | flags,
                allocator,
            );
        }
    }
}
