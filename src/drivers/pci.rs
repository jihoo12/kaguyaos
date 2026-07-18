use crate::io::{inl, outl};
use crate::println;

pub const PCI_CONFIG_ADDRESS: u16 = 0xCF8;
pub const PCI_CONFIG_DATA: u16 = 0xCFC;

pub const PCI_CLASS_STORAGE: u8 = 0x01;
pub const PCI_SUBCLASS_NVME: u8 = 0x08;
pub const PCI_PROG_IF_NVME: u8 = 0x02;

pub const PCI_CLASS_SERIAL_BUS: u8 = 0x0C;
pub const PCI_SUBCLASS_USB: u8 = 0x03;
pub const PCI_PROG_IF_XHCI: u8 = 0x30;

pub const PCI_CLASS_NETWORK: u8 = 0x02;
pub const PCI_SUBCLASS_ETHERNET: u8 = 0x00;

#[derive(Debug, Clone, Copy)]
pub struct PciDevice {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
    pub vendor_id: u16,
    pub device_id: u16,
    pub bar0: u32,
    pub bar1: u32,
}

static mut NVME_DEVICE: Option<PciDevice> = None;
static mut XHCI_DEVICE: Option<PciDevice> = None;
static mut ETHERNET_DEVICE: Option<PciDevice> = None;

pub fn init() {
    unsafe {
        scan_bus();
    }
}

pub fn get_nvme_device() -> Option<PciDevice> {
    unsafe { NVME_DEVICE }
}

pub fn get_xhci_device() -> Option<PciDevice> {
    unsafe { XHCI_DEVICE }
}

pub fn get_ethernet_device() -> Option<PciDevice> {
    unsafe { ETHERNET_DEVICE }
}

/// Decode the physical MMIO base address for a device's BAR0.
pub fn mmio_bar0(device: &PciDevice) -> u64 {
    let mut base = (device.bar0 as u64) & 0xFFFF_FFF0;
    let bar_type = (device.bar0 >> 1) & 0x3;
    if bar_type == 2 {
        base |= (device.bar1 as u64) << 32;
    }
    base
}

unsafe fn scan_bus() {
    for bus in 0..256 {
        for dev in 0..32 {
            unsafe {
                check_device(bus as u8, dev as u8);
            }
        }
    }
}

unsafe fn check_device(bus: u8, dev: u8) {
    let vendor_id = unsafe { read_config_16(bus, dev, 0, 0x00) };
    if vendor_id == 0xFFFF {
        return;
    }

    unsafe {
        check_function(bus, dev, 0);
    }

    let header_type = unsafe { read_config_8(bus, dev, 0, 0x0E) };
    if (header_type & 0x80) != 0 {
        for func in 1..8 {
            if unsafe { read_config_16(bus, dev, func, 0x00) } != 0xFFFF {
                unsafe {
                    check_function(bus, dev, func);
                }
            }
        }
    }
}

unsafe fn check_function(bus: u8, dev: u8, func: u8) {
    let vendor_id = unsafe { read_config_16(bus, dev, func, 0x00) };
    if vendor_id == 0xFFFF {
        return;
    }
    let device_id = unsafe { read_config_16(bus, dev, func, 0x02) };
    let class_code = unsafe { read_config_8(bus, dev, func, 0x0B) };
    let sub_class = unsafe { read_config_8(bus, dev, func, 0x0A) };
    let prog_if = unsafe { read_config_8(bus, dev, func, 0x09) };

    println!(
        "PCI: Checking {}:{}:{} Vendor={:#04x} Device={:#04x} Class={:02x}:{:02x}:{:02x}",
        bus, dev, func, vendor_id, device_id, class_code, sub_class, prog_if
    );

    if class_code == PCI_CLASS_STORAGE
        && sub_class == PCI_SUBCLASS_NVME
        && prog_if == PCI_PROG_IF_NVME
    {
        let bar0 = unsafe { read_config_32(bus, dev, func, 0x10) };
        let bar1 = unsafe { read_config_32(bus, dev, func, 0x14) };

        let device = PciDevice {
            bus,
            device: dev,
            function: func,
            vendor_id,
            device_id,
            bar0,
            bar1,
        };

        unsafe {
            NVME_DEVICE = Some(device);
        }
        println!("PCI: Found NVMe Controller at {}:{}:{}", bus, dev, func);
        println!(
            "Vendor ID: {:#04x}, Device ID: {:#04x}",
            vendor_id, device_id
        );

        // Enable Bus Master and Memory Space in Command Register (Offset 0x04)
        let mut cmd = unsafe { read_config_16(bus, dev, func, 0x04) };
        cmd |= 0x0006; // Bit 1: Memory Space, Bit 2: Bus Master
        unsafe {
            write_config_32(bus, dev, func, 0x04, cmd as u32);
        }
    } else if class_code == PCI_CLASS_SERIAL_BUS
        && sub_class == PCI_SUBCLASS_USB
        && prog_if == PCI_PROG_IF_XHCI
    {
        let bar0 = unsafe { read_config_32(bus, dev, func, 0x10) };
        let bar1 = unsafe { read_config_32(bus, dev, func, 0x14) };

        let device = PciDevice {
            bus,
            device: dev,
            function: func,
            vendor_id,
            device_id,
            bar0,
            bar1,
        };

        unsafe {
            XHCI_DEVICE = Some(device);
        }
        println!("PCI: Found xHCI Controller at {}:{}:{}", bus, dev, func);
        println!(
            "Vendor ID: {:#04x}, Device ID: {:#04x}",
            vendor_id, device_id
        );

        // Enable Bus Master and Memory Space in Command Register (Offset 0x04)
        let mut cmd = unsafe { read_config_16(bus, dev, func, 0x04) };
        cmd |= 0x0006; // Bit 1: Memory Space, Bit 2: Bus Master
        unsafe {
            write_config_32(bus, dev, func, 0x04, cmd as u32);
        }
    } else if class_code == PCI_CLASS_NETWORK && sub_class == PCI_SUBCLASS_ETHERNET {
        let bar0 = unsafe { read_config_32(bus, dev, func, 0x10) };
        let bar1 = unsafe { read_config_32(bus, dev, func, 0x14) };

        let device = PciDevice {
            bus,
            device: dev,
            function: func,
            vendor_id,
            device_id,
            bar0,
            bar1,
        };

        unsafe {
            ETHERNET_DEVICE = Some(device);
        }
        println!(
            "PCI: Found Ethernet controller at {}:{}:{}",
            bus, dev, func
        );
        println!(
            "Vendor ID: {:#04x}, Device ID: {:#04x}",
            vendor_id, device_id
        );

        let mut cmd = unsafe { read_config_16(bus, dev, func, 0x04) };
        cmd |= 0x0006;
        unsafe {
            write_config_32(bus, dev, func, 0x04, cmd as u32);
        }
    }
}

pub unsafe fn read_config_32(bus: u8, dev: u8, func: u8, offset: u8) -> u32 {
    let address = ((bus as u32) << 16)
        | ((dev as u32) << 11)
        | ((func as u32) << 8)
        | (offset as u32 & 0xFC)
        | 0x80000000;
    unsafe {
        outl(PCI_CONFIG_ADDRESS, address);
        inl(PCI_CONFIG_DATA)
    }
}

pub unsafe fn read_config_16(bus: u8, dev: u8, func: u8, offset: u8) -> u16 {
    let val = unsafe { read_config_32(bus, dev, func, offset) };
    (val >> ((offset & 2) * 8)) as u16
}

pub unsafe fn read_config_8(bus: u8, dev: u8, func: u8, offset: u8) -> u8 {
    let val = unsafe { read_config_32(bus, dev, func, offset) };
    (val >> ((offset & 3) * 8)) as u8
}

pub unsafe fn write_config_32(bus: u8, dev: u8, func: u8, offset: u8, val: u32) {
    let address = ((bus as u32) << 16)
        | ((dev as u32) << 11)
        | ((func as u32) << 8)
        | (offset as u32 & 0xFC)
        | 0x80000000;
    unsafe {
        outl(PCI_CONFIG_ADDRESS, address);
        outl(PCI_CONFIG_DATA, val);
    }
}
