# Chapter 3: Booting Into Your Kernel

This is the pivotal moment: calling `ExitBootServices()` and taking full control of the machine. After this call, UEFI firmware is gone — no more text output, no more memory allocation. Your kernel owns everything.

## The Contract

`ExitBootServices()` is a one-way door. Before you call it:

- Collect everything you need: memory map, framebuffer address, ACPI tables
- You can still use UEFI Boot Services (memory allocation, protocol access)

After you call it:

- All UEFI Boot Services are **invalid** — calling them causes a crash
- UEFI Runtime Services (RTC, variable access, reset) still work
- You have raw hardware: no drivers, no allocator, no `println!`

## Packing Data: The BootInfo Struct

kaguyaOS defines a `BootInfo` struct to carry UEFI's data into the kernel:

```rust
// src/main.rs:44-61
#[repr(C)]
#[derive(Copy, Clone)]
pub struct BootInfo {
    pub framebuffer_base: u64,
    pub framebuffer_size: usize,
    pub horizontal_resolution: u32,
    pub vertical_resolution: u32,
    pub pixels_per_scanline: u32,
    pub pixel_format: u32,
    pub memory_map: *mut u8,
    pub memory_map_size: usize,
    pub descriptor_size: usize,
    pub descriptor_version: u32,
    pub runtime_services: u64,
    pub acpi_rsdp_phys: u64,
}
```

Why `#[repr(C)]`? Because this struct crosses the UEFI-to-kernel boundary — the compiler must lay it out predictably with no Rust-specific reordering.

## The Full efi_main Flow

Here's what kaguyaOS's `efi_main()` does (simplified from `src/main.rs:490`):

```rust
pub extern "efiapi" fn efi_main(
    _image_handle: EFI_HANDLE,
    system_table: *mut EFI_SYSTEM_TABLE,
) -> EFI_STATUS {
    // 1. Print "Getting ready to jump to kernel..." via UEFI console
    //    (this is our last chance to use firmware text output)

    let boot_services = unsafe { (*system_table).BootServices };
    let runtime_services = unsafe { (*system_table).RuntimeServices };

    // 2. Locate GOP → get framebuffer info
    let mut gop: *mut EFI_GRAPHICS_OUTPUT_PROTOCOL = core::ptr::null_mut();
    // ... LocateProtocol(GOP_GUID, &mut gop) ...

    let mode = unsafe { *(*gop).Mode };
    let info = unsafe { *mode.Info };

    let framebuffer_base = mode.FrameBufferBase;
    let framebuffer_size = mode.FrameBufferSize;
    let horizontal_resolution = info.HorizontalResolution;
    let vertical_resolution = info.VerticalResolution;
    let pixels_per_scanline = info.PixelsPerScanLine;
    let pixel_format = info.PixelFormat as u32;

    // 3. Get the UEFI memory map
    let mut memory_map_buffer = [0u8; 16384];
    let mut memory_map_size = memory_map_buffer.len();
    let mut map_key: usize = 0;
    let mut descriptor_size: usize = 0;
    let mut descriptor_version: u32 = 0;

    let status = unsafe {
        ((*boot_services).GetMemoryMap)(
            &mut memory_map_size,
            memory_map_buffer.as_mut_ptr() as *mut EFI_MEMORY_DESCRIPTOR,
            &mut map_key,
            &mut descriptor_size,
            &mut descriptor_version,
        )
    };

    // 4. Find the ACPI RSDP from the EFI Configuration Table
    //    (must be done before ExitBootServices — config table may be gone)
    let acpi_rsdp_phys = unsafe {
        uefi::find_rsdp_in_system_table(system_table)
    };

    // 5. ExitBootServices — the point of no return
    let mut status = unsafe {
        ((*boot_services).ExitBootServices)(_image_handle, map_key)
    };

    // The memory map may have changed between GetMemoryMap and
    // ExitBootServices. If ExitBootServices fails, we re-fetch
    // and try once more.
    if status != 0 {
        memory_map_size = memory_map_buffer.len();
        // ... re-fetch memory map, retry ExitBootServices ...
    }

    // 6. Build BootInfo and jump to kernel
    let boot_info = BootInfo {
        framebuffer_base,
        framebuffer_size,
        horizontal_resolution,
        vertical_resolution,
        pixels_per_scanline,
        pixel_format,
        memory_map: memory_map_buffer.as_mut_ptr(),
        memory_map_size,
        descriptor_size,
        descriptor_version,
        runtime_services: runtime_services as u64,
        acpi_rsdp_phys,
    };

    kernel_main(&boot_info);  // Never returns
}
```

## The ExitBootServices Retry Problem

Notice the retry logic. Between `GetMemoryMap()` and `ExitBootServices()`, the firmware might allocate or free memory internally, invalidating the `map_key`. If `ExitBootServices` fails with that key, we must re-fetch the memory map and try again.

This is a real-world gotcha that many OS tutorials skip.

## What Happens After ExitBootServices

```
  ┌─────────────────────────┐
  │  UEFI Firmware (alive)  │  Runtime Services still work
  └─────────────┬───────────┘
                │ ExitBootServices()
                ▼
  ┌─────────────────────────┐
  │     YOUR KERNEL         │  No firmware services
  │                         │  Raw hardware access
  │  • Framebuffer (still   │  (mapped by firmware before exit)
  │    accessible at same   │
  │    physical address)    │
  │                         │
  │  • Memory map (saved)   │  Your only knowledge of RAM layout
  │                         │
  │  • ACPI RSDP (saved)    │  Your only path to LAPIC/APIC info
  │                         │
  │  • Nothing else         │  No serial output, no allocator,
  │                         │  no drivers — yet
  └─────────────────────────┘
```

## Writing to Serial After ExitBootServices

The serial port (COM1 at I/O port 0x3F8) doesn't need firmware — it's hardware. kaguyaOS writes to serial in its `Writer` (see Chapter 4), but you can do it directly:

```rust
unsafe fn serial_write(s: &str) {
    for byte in s.bytes() {
        // Wait for transmit buffer empty
        while (inb(0x3FD) & 0x20) == 0 {}
        outb(0x3F8, byte);
    }
}

unsafe fn outb(port: u16, val: u8) {
    core::arch::asm!("out dx, al", in(dx) port, in(al) val);
}

unsafe fn inb(port: u16) -> u8 {
    let val: u8;
    core::arch::asm!("in al, dx", out(al) val, in(dx) port);
    val
}
```

This gives you a "println" before you've set up a framebuffer console.

## The Two-Stage Architecture

kaguyaOS uses a clean two-stage boot:

| Stage | Runs in | Has firmware? | Purpose |
|-------|---------|---------------|---------|
| `efi_main()` | UEFI context | Yes | Collect data, call ExitBootServices |
| `kernel_main()` | Bare metal | No | Initialize all subsystems, start scheduler |

This separation keeps UEFI-specific code isolated. Everything from Chapter 4 onward lives in `kernel_main()` and never touches UEFI Boot Services.

## kaguyaOS Reference

| File | Lines | What it does |
|------|-------|-------------|
| `src/main.rs` | 490-619 | `efi_main()` — full UEFI exit sequence |
| `src/main.rs` | 44-61 | `BootInfo` struct definition |
| `src/main.rs` | 82-487 | `kernel_main()` — everything after boot |
| `src/uefi.rs` | 1-100 | UEFI type definitions (GOP, Boot Services, memory descriptors) |

---

**Next:** [Chapter 4 — Kernel Hello World](ch4-kernel-hello.md) — Setting up a framebuffer console from scratch.
