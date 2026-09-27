//! First hardware milestone: GOP and firmware memory-map diagnostics only.
use crate::{BootInfo, console, uefi::EFI_MEMORY_DESCRIPTOR};

pub fn run(info: &BootInfo) -> ! {
    unsafe { core::arch::asm!("cli", options(nomem, nostack)); }
    console::clear();
    crate::println!("kaguyaOS - HARDWARE TEST (storage writes disabled)");
    crate::println!("UEFI ExitBootServices: OK");
    crate::println!("GOP: {} x {}, stride {}, format {}",
        info.horizontal_resolution, info.vertical_resolution,
        info.pixels_per_scanline, info.pixel_format);
    crate::println!("Framebuffer: {:#x}, {} bytes", info.framebuffer_base, info.framebuffer_size);
    let mut usable_pages = 0u64;
    let count = info.memory_map_size / info.descriptor_size;
    for i in 0..count {
        let descriptor = unsafe {
            core::ptr::read_unaligned(info.memory_map.add(i * info.descriptor_size)
                as *const EFI_MEMORY_DESCRIPTOR)
        };
        if descriptor.Type == crate::uefi::EFI_CONVENTIONAL_MEMORY {
            usable_pages = usable_pages.saturating_add(descriptor.NumberOfPages);
        }
    }
    crate::println!("Memory map: {} entries, descriptor size {}", count, info.descriptor_size);
    crate::println!("Conventional memory: {} MiB", usable_pages / 256);
    crate::println!("ACPI RSDP: {:#x}", info.acpi_rsdp_phys);
    crate::println!("PCI/NVMe/USB/network/GPU drivers: NOT STARTED");
    crate::println!("Paging replacement, heap, timers, APs, userspace: NOT STARTED");
    crate::println!("HARDWARE TEST READY - reset or power off to exit");
    loop { unsafe { core::arch::asm!("cli; hlt", options(nomem, nostack)); } }
}
