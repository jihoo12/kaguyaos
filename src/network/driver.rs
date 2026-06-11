use crate::memory::{FrameAllocator, PageTable};
use crate::pci::PciDevice;

/// Hardware-specific Ethernet driver interface (L2 only — no TCP/IP).
pub trait NetworkDriver {
    fn name(&self) -> &'static str;

    /// MMIO region size in bytes (used for page-table mapping).
    fn mmio_size(&self) -> u64;

    unsafe fn map_dma_buffers(
        &self,
        pml4: &mut PageTable,
        allocator: &mut FrameAllocator,
    );

    unsafe fn init(&mut self, device: PciDevice);

    /// Enqueue a raw Ethernet frame. Returns false if the TX ring is full.
    unsafe fn transmit(&mut self, data: &[u8]) -> bool;

    /// Poll the RX ring once. Returns bytes copied into `out`, or 0 if empty.
    unsafe fn poll_rx(&mut self, out: &mut [u8]) -> usize;

    /// Read the hardware MAC address programmed into the NIC.
    unsafe fn mac_address(&self) -> [u8; 6];
}

/// Supported NIC backends. Add a variant here when adding new hardware.
pub enum Nic {
    E1000(super::e1000::E1000),
}

impl Nic {
    /// Pick a driver implementation for the given PCI Ethernet device.
    pub fn probe(device: &PciDevice) -> Option<Self> {
        if super::e1000::E1000::matches(device) {
            return Some(Nic::E1000(super::e1000::E1000));
        }
        None
    }
}

impl NetworkDriver for Nic {
    fn name(&self) -> &'static str {
        match self {
            Nic::E1000(d) => d.name(),
        }
    }

    fn mmio_size(&self) -> u64 {
        match self {
            Nic::E1000(d) => d.mmio_size(),
        }
    }

    unsafe fn map_dma_buffers(
        &self,
        pml4: &mut PageTable,
        allocator: &mut FrameAllocator,
    ) {
        match self {
            Nic::E1000(d) => d.map_dma_buffers(pml4, allocator),
        }
    }

    unsafe fn init(&mut self, device: PciDevice) {
        match self {
            Nic::E1000(d) => d.init(device),
        }
    }

    unsafe fn transmit(&mut self, data: &[u8]) -> bool {
        match self {
            Nic::E1000(d) => d.transmit(data),
        }
    }

    unsafe fn poll_rx(&mut self, out: &mut [u8]) -> usize {
        match self {
            Nic::E1000(d) => d.poll_rx(out),
        }
    }

    unsafe fn mac_address(&self) -> [u8; 6] {
        match self {
            Nic::E1000(d) => d.mac_address(),
        }
    }
}