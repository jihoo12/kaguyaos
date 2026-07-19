# Chapter 2: UEFI Hello World

In this chapter we'll print "Hello World" using UEFI boot services, before the kernel even exists. This teaches the UEFI programming model and the transition from firmware to OS.

## The UEFI Boot Model

When a UEFI-capable machine powers on, the firmware:

1. Initializes hardware (memory, PCI, USB, storage)
2. Reads the ESP and loads `EFI/BOOT/BOOTX64.EFI`
3. Calls its entry point with pointers to **System Tables** (Boot Services, Runtime Services, Console I/O)
4. Your code runs in 64-bit long mode with paging enabled
5. You call `ExitBootServices()` to take ownership of hardware

Until you call `ExitBootServices()`, you're a guest of the firmware — you can use its services (text output, memory allocation, protocol discovery).

## Writing the UEFI Entry Point

Replace `src/main.rs` with:

```rust
#![no_std]
#![no_main]

use core::ffi::c_void;

// UEFI types (simplified)
type EFI_STATUS = usize;
type EFI_HANDLE = *mut c_void;

#[repr(C)]
struct EFI_SIMPLE_TEXT_OUTPUT_PROTOCOL {
    // Function pointers (simplified)
    _reset: *const c_void,
    output_string: unsafe extern "efiapi" fn(
        *mut EFI_SIMPLE_TEXT_OUTPUT_PROTOCOL,
        *const u16,
    ) -> EFI_STATUS,
}

#[repr(C)]
struct EFI_TABLE_HEADER {
    signature: u64,
    revision: u32,
    header_size: u32,
    crc32: u32,
    reserved: u32,
}

#[repr(C)]
struct EFI_RUNTIME_SERVICES {
    header: EFI_TABLE_HEADER,
    // ... more function pointers
    _pad: [u8; 256],
}

#[repr(C)]
struct EFI_BOOT_SERVICES {
    header: EFI_TABLE_HEADER,
    _pad: [u8; 512],
}

#[repr(C)]
struct EFI_SYSTEM_TABLE {
    header: EFI_TABLE_HEADER,
    firmware_vendor: *const u16,
    firmware_revision: u32,
    console_in_handle: EFI_HANDLE,
    _con_in: *const c_void,
    console_out_handle: EFI_HANDLE,
    con_out: *mut EFI_SIMPLE_TEXT_OUTPUT_PROTOCOL,
    _stderr_handle: EFI_HANDLE,
    _stderr: *const c_void,
    runtime_services: *mut EFI_RUNTIME_SERVICES,
    boot_services: *mut EFI_BOOT_SERVICES,
    _num_table_entries: usize,
    _table: *const *const c_void,
}

// Entry point — UEFI calls this with the image handle and system table
#[unsafe(no_mangle)]
pub extern "efiapi" fn efi_main(
    _image_handle: EFI_HANDLE,
    system_table: *mut EFI_SYSTEM_TABLE,
) -> EFI_STATUS {
    unsafe {
        let con_out = (*system_table).con_out;

        // Print "Hello from UEFI!\r\n"
        let msg = b"Hello from UEFI!\r\n\0";
        let mut buffer = [0u16; 32];
        for (i, &b) in msg.iter().enumerate() {
            if i >= 31 { break; }
            buffer[i] = b as u16;
        }
        ((*con_out).output_string)(con_out, buffer.as_ptr());
    }

    // Hang forever — we have nowhere to go yet
    loop {}
}

#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
```

Build and test:

```bash
cargo build --target x86_64-unknown-uefi
mkdir -p esp/EFI/BOOT
cp target/x86_64-unknown-uefi/debug/my_os.efi esp/EFI/BOOT/BOOTX64.EFI
qemu-system-x86_64 -bios $OVMF_BIOS -drive format=raw,file=fat:rw:esp -serial stdio -display none
```

You should see "Hello from UEFI!" on the QEMU serial console.

## Going graphical: The Graphics Output Protocol (GOP)

The serial console is nice, but let's get pixels on screen. UEFI provides the **Graphics Output Protocol (GOP)** to access the framebuffer.

The key idea:
1. Call `BootServices->LocateProtocol()` with the GOP GUID
2. Get the current mode (resolution, framebuffer base address, pixel format)
3. Write pixels directly to the framebuffer memory

```rust
// Simplified GOP access in efi_main:
#[repr(C)]
struct EFI_GRAPHICS_OUTPUT_PROTOCOL_MODE {
    max_mode: u32,
    mode: u32,
    info: *const EFI_GRAPHICS_OUTPUT_MODE_INFORMATION,
    info_size: usize,
    frame_buffer_base: u64,
    frame_buffer_size: usize,
}

#[repr(C)]
struct EFI_GRAPHICS_OUTPUT_PROTOCOL {
    query_mode: *const c_void,
    set_mode: *const c_void,
    blt: *const c_void,
    mode: *mut EFI_GRAPHICS_OUTPUT_PROTOCOL_MODE,
}

// The GOP GUID: 9042a9de-2339-4bf3-97f9-714c4e576000
const GOP_GUID: EFI_GUID = EFI_GUID {
    data1: 0x9042a9de,
    data2: 0x2339,
    data3: 0x4bf3,
    data4: [0x97, 0xf9, 0x71, 0x4c, 0x4e, 0x57, 0x60, 0x00],
};

// After locating the GOP protocol:
let gop = /* ... located via LocateProtocol ... */;
let mode = unsafe { *(*gop).mode };
let fb_base = mode.frame_buffer_base as *mut u32;
let stride = (*mode.info).pixels_per_scanline as usize;
let width = (*mode.info).horizontal_resolution as usize;
let height = (*mode.info).vertical_resolution as usize;

// Draw a red rectangle
for y in 100..200 {
    for x in 100..300 {
        let offset = y * stride + x;
        unsafe { *fb_base.add(offset) = 0x00FF0000; } // Red (BGR format)
    }
}
```

This is exactly what kaguyaOS does in `src/main.rs` (`efi_main` at line 490):

```rust
// src/main.rs:517-526
let mut gop: *mut EFI_GRAPHICS_OUTPUT_PROTOCOL = core::ptr::null_mut();
let gop_guid = EFI_GRAPHICS_OUTPUT_PROTOCOL_GUID;
let status = unsafe {
    ((*boot_services).LocateProtocol)(
        &gop_guid as *const EFI_GUID,
        core::ptr::null_mut(),
        &mut gop as *mut *mut EFI_GRAPHICS_OUTPUT_PROTOCOL as *mut *mut c_void,
    )
};
```

## Getting the Memory Map

Before you can take over the machine, you need the **UEFI memory map** — a list of all physical memory regions and their types (conventional, ACPI, MMIO, reserved, etc.).

```rust
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
```

The memory map is critical: after `ExitBootServices()`, this is the **only** way to know which physical pages are available for your kernel to use.

## What's Happening Under the Hood

```
Power On
  │
  ▼
UEFI Firmware initializes hardware
  │
  ▼
Loads EFI/BOOT/BOOTX64.EFI
  │
  ▼
Calls efi_main(image_handle, system_table)
  │
  ├──> LocateProtocol(GOP) ──> framebuffer access
  ├──> GetMemoryMap() ──> physical memory regions
  ├──> LocateProtocol(ACPI) ──> RSDP pointer
  │
  ▼
Your code runs in 64-bit long mode
  with paging enabled, MMIO mapped
```

## Key Takeaways

1. UEFI runs your code in 64-bit long mode with paging — no real-mode hacks needed
2. The System Table gives you access to Boot Services (memory, protocols) and Runtime Services (RTC, reset)
3. The GOP gives you direct framebuffer access for graphics
4. The memory map tells you which physical pages are free
5. You must call `ExitBootServices()` before doing any hardware initialization yourself

## kaguyaOS Reference

| File | What it does |
|------|-------------|
| `src/main.rs:490-619` | `efi_main()` — GOP, memory map, ExitBootServices |
| `src/uefi.rs` | All UEFI type definitions and constants |
| `src/main.rs:43-60` | `BootInfo` struct — data passed from UEFI to kernel |

---

**Next:** [Chapter 3 — Booting Into Your Kernel](ch3-boot.md) — Taking over the machine from UEFI.
