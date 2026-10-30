use crate::drivers::pci::PciDevice;
use core::mem::MaybeUninit;

unsafe extern "C" {
    #[link_name = "jihoo_pci_read32"]
    pub fn read_config_32(bus: u8, device: u8, function: u8, offset: u8) -> u32;
    #[link_name = "jihoo_pci_read16"]
    pub fn read_config_16(bus: u8, device: u8, function: u8, offset: u8) -> u16;
    #[link_name = "jihoo_pci_read8"]
    pub fn read_config_8(bus: u8, device: u8, function: u8, offset: u8) -> u8;
    #[link_name = "jihoo_pci_write32"]
    pub fn write_config_32(bus: u8, device: u8, function: u8, offset: u8, value: u32);
    pub fn jihoo_pci_scan(visit: extern "C" fn(u8, u8, u8));
    fn jihoo_pci_bar0(bar0: u32, bar1: u32) -> u64;
    fn jihoo_pci_find_xhci(index: usize, output: *mut PciDevice) -> bool;
}

pub fn mmio_bar0(device: &PciDevice) -> u64 {
    unsafe { jihoo_pci_bar0(device.bar0, device.bar1) }
}

pub fn find_xhci(index: usize) -> Option<PciDevice> {
    let mut output = MaybeUninit::uninit();
    if unsafe { jihoo_pci_find_xhci(index, output.as_mut_ptr()) } {
        Some(unsafe { output.assume_init() })
    } else {
        None
    }
}
