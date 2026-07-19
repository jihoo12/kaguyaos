# Chapter 12: Device Drivers

Your OS needs to talk to hardware: storage (for the filesystem), USB (for the keyboard), and network (for ping). This chapter covers PCI enumeration and three drivers.

## PCI Enumeration

PCI (Peripheral Component Interconnect) is the bus that connects devices to the CPU. Every PCI device has configuration registers accessible via I/O ports 0xCF8 (address) and 0xCFC (data):

```rust
// src/drivers/pci.rs:4-5
const PCI_CONFIG_ADDRESS: u16 = 0xCF8;
const PCI_CONFIG_DATA: u16 = 0xCFC;
```

The address encodes bus/device/function/register:

```
Bit 31:    Enable
Bits 23-16: Bus number
Bits 15-11: Device number
Bits 10-8:  Function number
Bits 7-2:   Register number
Bits 1-0:   Always 0
```

kaguyaOS scans all 256 buses, 32 devices, 8 functions:

```rust
// src/drivers/pci.rs (simplified)
pub fn init() {
    for bus in 0..256u16 {
        for device in 0..32u8 {
            for function in 0..8u8 {
                let vendor_id = pci_read_u16(bus, device, function, 0);
                if vendor_id == 0xFFFF { continue; }  // No device

                let class = pci_read_u8(bus, device, function, 10);
                let subclass = pci_read_u8(bus, device, function, 11);
                let bar0 = pci_read_u32(bus, device, function, 16);

                match (class, subclass) {
                    (0x01, 0x08) => { /* NVMe */ }
                    (0x0C, 0x03) => { /* xHCI USB */ }
                    (0x02, 0x00) => { /* Ethernet */ }
                    _ => {}
                }
            }
        }
    }
}
```

## MMIO Mapping

Device registers are accessed via Memory-Mapped I/O (MMIO). The BAR0 (Base Address Register 0) tells us where the device's registers are in physical memory.

Before accessing MMIO, you must identity-map the physical pages:

```rust
// src/main.rs:193-197 (NVMe example)
let bar = (device.bar0 as u64) & 0xFFFFFFF0;
let flags = PAGE_WRITABLE | PAGE_PRESENT;
for i in 0..4 {  // 4 pages = 16 KiB
    memory::map_page(pml4, bar + i * 4096, bar + i * 4096, flags, &mut allocator);
}
```

**Important:** MMIO pages should use `PAGE_CACHE_DISABLE` (UC memory type) to prevent the CPU from caching stale device register reads.

## NVMe Driver

NVMe (Non-Volatile Memory Express) is the modern interface for SSDs. The driver:

1. **Disables the controller** (write 0 to CC register)
2. **Configures admin queues** (submission + completion, 4 KiB each)
3. **Enables the controller** (write 1 to CC, wait for RDY)
4. **Identifies the controller** (admin command 0x06)
5. **Creates I/O queues** (for read/write operations)

```rust
// src/drivers/nvme.rs (simplified flow)
pub unsafe fn init(device: &PciDevice) {
    let base = (device.bar0 as u64) & 0xFFFFFFF0;

    // Disable controller
    write_volatile(base + 0x14, 0u32);  // CC.EN = 0
    while read_volatile(base + 0x1C) & 1 != 0 {}  // Wait CSTS.RDY = 0

    // Set up admin queues (CSS=0, IOSQES=6, IOCQES=4)
    write_volatile(base + 0x24, aqa);   // AQA
    write_volatile(base + 0x28, asq);   // ASQ
    write_volatile(base + 0x30, acq);   // ACQ

    // Enable controller
    write_volatile(base + 0x14, 1u32);  // CC.EN = 1
    while read_volatile(base + 0x1C) & 1 == 0 {}  // Wait CSTS.RDY = 1

    // ... identify controller, create I/O queues ...
}
```

## xHCI USB Driver

xHCI is the USB 3.0 host controller interface. The driver handles:

1. **Controller initialization** (reset, capability registers, operational registers)
2. **Device detection** (polling ports for connections)
3. **Device enumeration** (GET_DESCRIPTOR, SET_CONFIGURATION)
4. **Endpoint setup** (configuring interrupt IN for keyboard)

```rust
// src/drivers/xhci.rs (simplified flow)
pub unsafe fn init(device: &PciDevice) {
    let base = (device.bar0 as u64) & 0xFFFFFFF0;

    // Read capabilities
    let caplength = read_volatile(base) as u16;
    let operational = base + caplength as u64;

    // Reset controller
    write_volatile(operational, 2u32);  // USBCMD.HCRST = 1
    while read_volatile(operational) & 2 != 0 {}

    // Set up command ring, event ring, device contexts
    // ...

    // Start controller
    write_volatile(operational, 0x3u32);  // RUN + ENABLE

    // Poll ports for devices
    for port in 0..max_ports {
        if port_has_device(port) {
            reset_port(port);
            enable_slot(port);
            address_device(slot);
            setup_endpoint(slot, keyboard_endpoint);
        }
    }
}
```

## E1000 Ethernet Driver

The Intel e1000 (82540EM) is QEMU's default emulated NIC. The driver:

1. **Resets the device** (write to CTRL register)
2. **Reads the MAC address** from EEPROM (RAL/RAH registers)
3. **Sets up RX/TX descriptor rings** (16 entries each, DMA buffers)
4. **Enables receive/transmit** (RCTL/TCTL registers)

```rust
// src/drivers/net/e1000.rs (simplified)
pub unsafe fn init(device: &PciDevice) {
    let bar = crate::drivers::pci::mmio_bar0(device);

    // Reset
    write_reg(bar, 0x0000, 0x04000000);  // CTRL.RST
    // Wait for reset to complete ...

    // Read MAC address
    let mac = read_mac(bar);

    // Set up RX ring (16 buffers, 2048 bytes each)
    write_reg(bar, 0x2800, rx_ring_phys);  // RDBAL
    write_reg(bar, 0x2804, 0);             // RDBAH
    write_reg(bar, 0x2808, 16 * 16);      // RDLEN
    // ... initialize descriptors ...

    // Enable receive
    write_reg(bar, 0x0100, 0x0040002A);   // RCTL: EN, BAM, BSIZE=2048

    // Similar for TX ring ...
}
```

## DMA Buffers

Device drivers need DMA (Direct Memory Access) buffers — physical memory that the device can write to. These must be:

1. **Physically contiguous** (devices don't understand page tables)
2. **Identity-mapped** (the device uses physical addresses, and our kernel uses identity mapping)
3. **Cache-aligned** (for performance)

kaguyaOS allocates DMA buffers as part of the driver statics and maps them in `kernel_main()`:

```rust
// src/main.rs:239-249 (xHCI DMA buffers)
let single_page_statics: &[u64] = &[
    core::ptr::addr_of!(xhci::COMMAND_RING_BUFFER) as u64,
    core::ptr::addr_of!(xhci::DCBAA_BUFFER) as u64,
    core::ptr::addr_of!(xhci::EVENT_RING_BUFFER) as u64,
    // ...
];
for &addr in single_page_statics {
    memory::map_page(pml4, addr, addr, dma_flags, &mut allocator);
}
```

## kaguyaOS Reference

| File | Lines | What it does |
|------|-------|-------------|
| `src/drivers/pci.rs` | 1-238 | PCI enumeration |
| `src/drivers/nvme.rs` | 1-504 | NVMe block device |
| `src/drivers/xhci.rs` | 1-1302 | USB 3.0 controller + keyboard |
| `src/drivers/net/e1000.rs` | 1-361 | E1000 NIC driver |
| `src/drivers/net/mod.rs` | 1-338 | Network stack init |

---

**Next:** [Chapter 13 — FAT16 Filesystem](ch13-filesystem.md) — Storing files on the NVMe disk.
