mod driver;
mod e1000;
mod helper;
mod ipv4;

use crate::memory::{FrameAllocator, PageTable, PAGE_CACHE_DISABLE, PAGE_PRESENT, PAGE_WRITABLE};
use crate::pci::{self, PciDevice};
use crate::println;
use core::ptr::{addr_of_mut};
pub use driver::NetworkDriver;

pub mod arp;

static mut ACTIVE_NIC: Option<driver::Nic> = None;

/// Statically configured IPv4 address for this host (set via `set_ip_address`).
static mut HOST_IP: [u8; 4] = [0u8; 4];

pub fn is_ready() -> bool {
    unsafe { (*addr_of_mut!(ACTIVE_NIC)).is_some() }
}

/// Store the host IPv4 address.  Call this after `init` when you assign an IP
/// (e.g. hard-coded, DHCP result, or boot-parameter).
pub unsafe fn set_ip_address(ip: [u8; 4]) {
    *addr_of_mut!(HOST_IP) = ip;
}

/// Return the currently configured host IPv4 address, or `None` if unset
/// (all-zero means no address has been assigned yet).
pub fn get_ip_address() -> Option<[u8; 4]> {
    let ip = unsafe { *addr_of_mut!(HOST_IP) };
    if ip == [0u8; 4] { None } else { Some(ip) }
}

/// Read the MAC address from the active NIC's hardware registers.
/// Returns `None` if no NIC is initialised.
pub unsafe fn get_mac_address() -> Option<[u8; 6]> {
    match &*addr_of_mut!(ACTIVE_NIC) {
        Some(nic) => Some(nic.mac_address()),
        None => None,
    }
}

/// Probe PCI device, map MMIO/DMA, and bring up the NIC.
pub unsafe fn init(
    pml4: &mut PageTable,
    allocator: &mut FrameAllocator,
    device: PciDevice,
) {
    let Some(mut nic) = driver::Nic::probe(&device) else {
        println!(
            "network: unsupported NIC {:#04x}:{:#04x}",
            device.vendor_id, device.device_id
        );
        return;
    };

    let bar = pci::mmio_bar0(&device);
    let mmio_flags = PAGE_WRITABLE | PAGE_PRESENT | PAGE_CACHE_DISABLE;
    let pages = (nic.mmio_size() + 4095) / 4096;

    println!("network: probing {} at MMIO {:#x}", nic.name(), bar);

    for i in 0..pages {
        let phys = bar + i * 4096;
        crate::memory::map_page(pml4, phys, phys, mmio_flags, allocator);
    }

    nic.map_dma_buffers(pml4, allocator);
    nic.init(device);

    let name = nic.name();
    *addr_of_mut!(ACTIVE_NIC) = Some(nic);
    println!("network: {} ready", name);
}

/// Send a raw Ethernet frame through the active driver.
pub unsafe fn transmit(data: &[u8]) -> bool {
    match &mut *addr_of_mut!(ACTIVE_NIC) {
        Some(nic) => nic.transmit(data),
        None => false,
    }
}

/// Poll the active driver for one received frame.
pub unsafe fn poll_rx(out: &mut [u8]) -> usize {
    match &mut *addr_of_mut!(ACTIVE_NIC) {
        Some(nic) => nic.poll_rx(out),
        None => 0,
    }
}

//need implement ip/cmp 
// dhcp