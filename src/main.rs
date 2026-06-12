#![no_std]
#![no_main]

#[macro_use]
extern crate alloc;

mod uefi;
use core::ffi::c_void;
use uefi::*;

pub mod kef;
#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    let cs: u16;
    unsafe {
        core::arch::asm!("mov {0:x}, cs", out(reg) cs);
    }
    let cpl = cs & 0x03;

    if cpl == 3 {
        // User Mode Panic - Use Syscall to print
        // We can't format easily without alloc, so just print a static error string + address?
        // Or try to format into a small stack buffer.
        let msg = "PANIC in User Mode!\n";
        unsafe {
            core::arch::asm!(
                "syscall",
                in("rax") 1, // sys_print
                in("rdi") msg.as_ptr(),
                in("rsi") msg.len(),
                out("rcx") _,
                out("r11") _,
                options(nostack, preserves_flags)
            );
        }
    } else {
        // Kernel Mode Panic
        println!("{}", _info);
    }
    loop {}
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct BootInfo {
    pub framebuffer_base: u64,
    pub framebuffer_size: usize,
    pub horizontal_resolution: u32,
    pub vertical_resolution: u32,
    pub pixels_per_scanline: u32,
    pub pixel_format: u32, // Simplified Enum mapping
    pub memory_map: *mut u8,
    pub memory_map_size: usize,
    pub descriptor_size: usize,
    pub descriptor_version: u32,
    pub runtime_services: u64,
    /// Physical address of the ACPI RSDP, obtained from the EFI
    /// Configuration Table before ExitBootServices. Zero if not found.
    pub acpi_rsdp_phys: u64,
}

mod acpi;
mod allocator;
mod fs;
mod gdt;
mod interrupts;
mod io;
mod memory;
mod network;
mod nvme;
mod pci;
mod pic;
mod processor;
mod scheduler;
mod syscall;
mod writer;
mod xhci;

#[repr(align(16))]
struct KernelStack([u8; 16384]);
static mut KERNEL_STACK: KernelStack = KernelStack([0; 16384]);

#[unsafe(no_mangle)]
pub extern "sysv64" fn kernel_main(boot_info: &BootInfo) -> ! {
    // Initialize Global Writer (for interrupts and syscalls)
    unsafe {
        writer::init_global_writer(*boot_info);
    }

    // We can now use println!
    println!("Hello World from Kernel!");
    println!(
        "Resolution: {}x{}",
        boot_info.horizontal_resolution, boot_info.vertical_resolution
    );
    println!("Framebuffer: {:#x}", boot_info.framebuffer_base);
    // Initialize Frame Allocator
    let mut allocator = unsafe { memory::FrameAllocator::new(boot_info) };

    // Initialize UEFI Runtime Services
    unsafe {
        uefi::init_runtime_services(boot_info.runtime_services as *mut uefi::EFI_RUNTIME_SERVICES);
    }

    // Initialize GDT
    unsafe {
        gdt::init();
        interrupts::init_idt();
        println!("GDT & IDT Initialized!");
    }

    let pml4_phys = unsafe { memory::init_paging(boot_info, &mut allocator) };
    unsafe {
        println!("Paging Initialized!");

        // Initialize PIC and Interrupts
        pic::init();
        // Enable interrupts
        core::arch::asm!("sti");
        println!("Interrupts Enabled!");
    }

    // Initialize Syscalls
    unsafe {
        syscall::init();
        println!("Syscalls Initialized!");
    }

    // ── ACPI ──────────────────────────────────────────────────────────────
    // The RSDP address was read from the EFI Configuration Table in efi_main
    // (before ExitBootServices) and stored in boot_info.acpi_rsdp_phys.
    // On UEFI systems the RSDP is never in the legacy BIOS ROM region, so we
    // skip the old BIOS scan entirely.
    let mut acpi_tables: Option<acpi::AcpiTables> = None;
    if boot_info.acpi_rsdp_phys != 0 {
        // Step 1: validate + wrap the RSDP pointer.
        let rsdp = unsafe { acpi::rsdp_from_address(boot_info.acpi_rsdp_phys) }
            .expect("ACPI RSDP checksum invalid");
        // Step 2: build the table index (stores pointers, no deref yet).
        let tables = unsafe { acpi::AcpiTables::from_rsdp(rsdp) };
        // Step 3: map every table page so we can safely dereference them.
        unsafe {
            let pml4 = memory::get_table_mut(pml4_phys);
            acpi::map_acpi_tables(pml4, &mut allocator, &tables);
        }
        // Step 4: use the tables.
        unsafe { acpi::dump_tables(&tables) };
        if let Some(info) = unsafe { tables.madt_info() } {
            println!(
                "Found {} CPUs, I/O APIC @ {:#x}",
                info.cpu_count, info.io_apic_address
            );
        }
        acpi_tables = Some(tables);
    } else {
        println!("ACPI: no RSDP found in EFI Configuration Table");
    }

    // Initialize PCI
    pci::init();

    // Initialize NVMe
    if let Some(device) = pci::get_nvme_device() {
        unsafe {
            // Identity map the BAR0 MMIO region
            let pml4 = memory::get_table_mut(pml4_phys);

            let mut bar = (device.bar0 as u64) & 0xFFFFFFF0;
            if device.bar1 != 0 {
                bar |= (device.bar1 as u64) << 32;
            }

            println!("Mapping NVMe BAR at {:#x}", bar);

            // Map 16KB (4 pages)
            let flags = memory::PAGE_WRITABLE | memory::PAGE_PRESENT; // Kernel only, or User? Let's keep it minimal.
            // Usually drivers run in kernel, so no PAGE_USER needed unless we access from user ring (which we shouldn't directly)
            // But map_page logic uses OR for user? No, flags argument is used.
            // memory.rs map_page signature: flags.

            for i in 0..4 {
                let offset = i * 4096;
                let addr = bar + offset;
                memory::map_page(pml4, addr, addr, flags, &mut allocator);
            }

            nvme::init(device);
            match fs::read_boot_sector() {
                Ok(bs) => {
                    let clusters = bs.total_clusters;
                    println!(
                        "FS: FAT volume mounted successfully. Total clusters: {}",
                        clusters
                    );
                }
                Err(_) => {
                    println!("FS: FAT volume is not formatted.");
                }
            }
        }
    } else {
        println!("No NVMe device found!");
    }
    if let Some(device) = pci::get_xhci_device() {
        println!("find xHCI device\n");
        unsafe {
            let pml4 = memory::get_table_mut(pml4_phys);
            let mut xhci_base_phys = (device.bar0 as u64) & 0xFFFFFFF0;
            let bar_type = (device.bar0 >> 1) & 0x3;

            if bar_type == 2 {
                xhci_base_phys |= (device.bar1 as u64) << 32;
            }

            // Map xHCI MMIO (with cache disable)
            let mmio_flags =
                memory::PAGE_WRITABLE | memory::PAGE_PRESENT | memory::PAGE_CACHE_DISABLE;
            for i in 0..16 {
                let phys = xhci_base_phys + i * 4096;
                memory::map_page(pml4, phys, phys, mmio_flags, &mut allocator);
            }

            // Map xHCI DMA/static buffers (no cache disable)
            let dma_flags = memory::PAGE_WRITABLE | memory::PAGE_PRESENT;

            // Single-page statics
            let single_page_statics: &[u64] = &[
                core::ptr::addr_of!(xhci::COMMAND_RING_BUFFER) as u64,
                core::ptr::addr_of!(xhci::DCBAA_BUFFER) as u64,
                core::ptr::addr_of!(xhci::EVENT_RING_SEGMENT_TABLE) as u64,
                core::ptr::addr_of!(xhci::EVENT_RING_BUFFER) as u64,
                core::ptr::addr_of!(xhci::INPUT_CONTEXT_BUFFER) as u64,
                core::ptr::addr_of!(xhci::USB_DATA_BUFFER) as u64,
            ];
            for &addr in single_page_statics {
                memory::map_page(pml4, addr, addr, dma_flags, &mut allocator);
            }

            // Multi-page statics
            let multi_page_statics: &[(u64, usize)] = &[
                (
                    core::ptr::addr_of!(xhci::DEVICE_CONTEXT_BUFFERS) as u64,
                    core::mem::size_of::<xhci::DeviceContextBuffer>(),
                ),
                (
                    core::ptr::addr_of!(xhci::EP0_TR_BUFFERS) as u64,
                    core::mem::size_of::<[xhci::TransferRingBuffer; 64]>(),
                ),
                (
                    core::ptr::addr_of!(xhci::KEYBOARD_TR_BUFFERS) as u64,
                    core::mem::size_of::<xhci::KeyboardTrBuffers>(),
                ),
            ];
            for &(base, size) in multi_page_statics {
                let pages = (size + 4095) / 4096;
                for i in 0..pages as u64 {
                    let addr = base + i * 4096;
                    memory::map_page(pml4, addr, addr, dma_flags, &mut allocator);
                }
            }
            xhci::init(device);
        }
    } else {
        println!("failed to find xHCI device\n")
    }
    if let Some(device) = pci::get_ethernet_device() {
        unsafe {
            let pml4 = memory::get_table_mut(pml4_phys);
            network::init(pml4, &mut allocator, device);
        }
        unsafe { network::set_ip_address([10, 0, 2, 15]) };
        let ip = network::get_ip_address();
        let mac = unsafe { network::get_mac_address() };
        println!("ip:{:?}", ip);
        println!("mac:{:?}", mac);
    } else {
        println!("No Ethernet device found!");
    }
    // Initialize Heap
    // Allocate 128 pages (512KB) for the heap
    let heap_pages = 128;
    let heap_start = allocator
        .allocate_frame()
        .expect("Failed to allocate heap start");
    let mut current_addr = heap_start;

    for _ in 1..heap_pages {
        let next_addr = allocator.allocate_frame().expect("Failed to allocate heap");
        if next_addr != current_addr + 4096 {
            panic!("Heap memory allocation failed: memory not contiguous!");
        }
        current_addr = next_addr;
    }

    // Map heap pages
    unsafe {
        let pml4 = memory::get_table_mut(pml4_phys);
        let flags = memory::PAGE_WRITABLE | memory::PAGE_PRESENT;
        for i in 0..heap_pages as u64 {
            let addr = heap_start + i * 4096;
            memory::map_page(pml4, addr, addr, flags, &mut allocator);
        }
    }

    unsafe {
        allocator::init(heap_start as usize, (heap_pages * 4096) as usize);
    }

    unsafe {
        scheduler::init();
    }
    if let Some(tables) = acpi_tables {
        if let Some(madt_ptr) = tables.madt {
            let madt = unsafe { acpi::parse_madt(madt_ptr) };
            unsafe {
                let pml4 = memory::get_table_mut(pml4_phys);
                let flags = memory::PAGE_WRITABLE | memory::PAGE_PRESENT | memory::PAGE_CACHE_DISABLE;
                let lapic_phys = madt.local_apic_address;
                println!("Mapping Local APIC MMIO at {:#x}", lapic_phys);
                memory::map_page(pml4, lapic_phys, lapic_phys, flags, &mut allocator);
                if madt.io_apic_address != 0 {
                    let io_apic_phys = madt.io_apic_address as u64;
                    println!("Mapping I/O APIC MMIO at {:#x}", io_apic_phys);
                    memory::map_page(pml4, io_apic_phys, io_apic_phys, flags, &mut allocator);
                }
            }
            let bsp_id = processor::current_apic_id();
            unsafe { processor::start_all_aps(&madt, bsp_id) };
            println!("Online APs: {}", processor::online_ap_count());
        } else {
            println!("ACPI: MADT table not found. Cannot start APs.");
        }
    } else {
        println!("ACPI: tables not initialized. Cannot start APs.");
    }

    // Initialize FAT filesystem & load init.kef
    let mut fs_ready = false;
    unsafe {
        if fs::is_ready() {
            if fs::read_boot_sector().is_err() {
                println!("FS: FAT volume not formatted. Formatting...");
                if fs::format().is_ok() {
                    fs_ready = true;
                }
            } else {
                fs_ready = true;
            }
        }
    }

    if fs_ready {
        let exists = fs::find_file("init.kef").ok().flatten().is_some();
        if !exists {
            println!("FS: init.kef not found. Assembling and creating default init.kef...");
            let default_kef = [0]; //todo need add 
            if let Err(e) = fs::create_file("init.kef", &default_kef) {
                println!("FS: Failed to create init.kef: {:?}", e);
            } else {
                println!("FS: Successfully created init.kef");
            }
        }
    }

    let mut loaded = false;
    if fs_ready {
        match fs::read_file("init.kef") {
            Ok(file_data) => {
                let pml4 = unsafe { memory::get_table_mut(pml4_phys) };
                match kef::load_kef(&file_data, &mut allocator, pml4) {
                    Ok((entry_point, user_rsp)) => {
                        println!("Loader: Successfully loaded init.kef. Entry={:#x}, RSP={:#x}", entry_point, user_rsp);
                        scheduler::add_new_user_task(entry_point, user_rsp, 16384);
                        loaded = true;
                    }
                    Err(e) => {
                        println!("Loader: Failed to load init.kef: {}", e);
                    }
                }
            }
            Err(e) => {
                println!("FS: Failed to read init.kef: {:?}", e);
            }
        }
    }

    if !loaded {
        panic!("Failed to load user-mode init process. System halted.");
    }

    unsafe {
        let stack_base = core::ptr::addr_of!(KERNEL_STACK) as u64;
        let stack_top = stack_base + 16384;

        // Map the kernel stack pages
        let pml4 = memory::get_table_mut(pml4_phys);
        let flags = memory::PAGE_WRITABLE | memory::PAGE_PRESENT;
        for i in 0..(16384 / 4096) as u64 {
            memory::map_page(
                pml4,
                stack_base + i * 4096,
                stack_base + i * 4096,
                flags,
                &mut allocator,
            );
        }

        println!("Kernel stack base={:#x} top={:#x}", stack_base, stack_top);
        println!("TSS rsp0={:#x}", gdt::get_tss_stack());

        println!("Starting scheduler loop on BSP...");
        core::arch::asm!("sti");
        loop {
            scheduler::switch_task();
            core::arch::asm!("hlt", options(nomem, nostack, preserves_flags));
        }
    }
}

#[unsafe(no_mangle)]
pub extern "efiapi" fn efi_main(
    _image_handle: EFI_HANDLE,
    system_table: *mut EFI_SYSTEM_TABLE,
) -> EFI_STATUS {
    // 1. Initialize formatted output (minimal)
    let msg = "Getting ready to jump to kernel...\r\n\0";
    let mut buffer: [u16; 64] = [0; 64];
    for (i, b) in msg.bytes().enumerate() {
        if i >= 63 {
            break;
        }
        buffer[i] = b as u16;
    }

    unsafe {
        let con_out = (*system_table).ConOut;
        ((*con_out).OutputString)(con_out, buffer.as_ptr());
    }

    let boot_services = unsafe { (*system_table).BootServices };
    let runtime_services = unsafe { (*system_table).RuntimeServices };

    // 2. Locate GOP
    let mut gop: *mut EFI_GRAPHICS_OUTPUT_PROTOCOL = core::ptr::null_mut();
    let gop_guid = EFI_GRAPHICS_OUTPUT_PROTOCOL_GUID;

    let status = unsafe {
        ((*boot_services).LocateProtocol)(
            &gop_guid as *const EFI_GUID,
            core::ptr::null_mut(),
            &mut gop as *mut *mut EFI_GRAPHICS_OUTPUT_PROTOCOL as *mut *mut c_void,
        )
    };

    if status != 0 {
        // Failed to locate GOP
        return status;
    }

    // 3. Prepare BootInfo
    let mode = unsafe { *(*gop).Mode };
    let info = unsafe { *mode.Info };

    // Framebuffer might need mapping? In UEFI it is identity mapped or IO mapped.
    // We assume we can access it directly for now (x86_64 UEFI usually maps it).

    let framebuffer_base = mode.FrameBufferBase;
    let framebuffer_size = mode.FrameBufferSize;
    let horizontal_resolution = info.HorizontalResolution;
    let vertical_resolution = info.VerticalResolution;
    let pixels_per_scanline = info.PixelsPerScanLine;
    let pixel_format = info.PixelFormat as u32;

    // 4. Get Memory Map
    // We need a larger buffer for real hardware.
    // Using static buffer to avoid stack overflow or allocation issues.
    // But since no global allocator, we put it on stack or use raw bytes.
    // 16KB should be enough.
    let mut memory_map_buffer = [0u8; 16384];
    let mut memory_map_size = memory_map_buffer.len();
    let mut map_key: usize = 0;
    let mut descriptor_size: usize = 0;
    let mut descriptor_version: u32 = 0;

    let memory_map_ptr = memory_map_buffer.as_mut_ptr() as *mut EFI_MEMORY_DESCRIPTOR;

    let status = unsafe {
        ((*boot_services).GetMemoryMap)(
            &mut memory_map_size,
            memory_map_ptr,
            &mut map_key,
            &mut descriptor_size,
            &mut descriptor_version,
        )
    };

    if status != 0 {
        return status;
    }

    // 5. Exit Boot Services
    let mut status = unsafe { ((*boot_services).ExitBootServices)(_image_handle, map_key) };

    if status != 0 {
        // The memory map changed between GetMemoryMap and ExitBootServices.
        // We must get the memory map again and retry once.
        memory_map_size = memory_map_buffer.len();
        status = unsafe {
            ((*boot_services).GetMemoryMap)(
                &mut memory_map_size,
                memory_map_ptr,
                &mut map_key,
                &mut descriptor_size,
                &mut descriptor_version,
            )
        };

        if status != 0 {
            return status;
        }

        status = unsafe { ((*boot_services).ExitBootServices)(_image_handle, map_key) };

        if status != 0 {
            return status;
        }
    }

    // 5b. Locate the ACPI RSDP from EFI Configuration Table
    //     (must be done BEFORE ExitBootServices, while config table is valid).
    let acpi_rsdp_phys = unsafe { uefi::find_rsdp_in_system_table(system_table) };

    // 6. Jump to Kernel
    let boot_info = BootInfo {
        framebuffer_base,
        framebuffer_size,
        horizontal_resolution,
        vertical_resolution,
        pixels_per_scanline,
        pixel_format,
        memory_map: memory_map_ptr as *mut u8,
        memory_map_size,
        descriptor_size,
        descriptor_version,
        runtime_services: runtime_services as u64,
        acpi_rsdp_phys,
    };

    kernel_main(&boot_info);
}
