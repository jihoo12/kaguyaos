#![allow(dead_code)]

use crate::memory::{FrameAllocator, PAGE_PRESENT, PAGE_WRITABLE};
use crate::network::NetworkDriver;
use crate::pci::PciDevice;
use crate::println;
use core::ptr::{addr_of_mut, read_volatile, write_volatile};

const MMIO_SIZE: u64 = 128 * 1024;
const RING_SIZE: usize = 16;
const PACKET_BUF_SIZE: usize = 2048;

const E1000_VENDOR: u16 = 0x8086;
const E1000_DEVICE_82540EM: u16 = 0x100E;

const REG_CTRL: u32 = 0x0000;
const REG_STATUS: u32 = 0x0008;
const REG_RCTL: u32 = 0x0100;
const REG_TCTL: u32 = 0x0400;
const REG_TIPG: u32 = 0x0410;
const REG_RDBAL: u32 = 0x2800;
const REG_RDBAH: u32 = 0x2804;
const REG_RDLEN: u32 = 0x2808;
const REG_RDH: u32 = 0x2810;
const REG_RDT: u32 = 0x2818;
const REG_TDBAL: u32 = 0x3800;
const REG_TDBAH: u32 = 0x3804;
const REG_TDLEN: u32 = 0x3808;
const REG_TDH: u32 = 0x3810;
const REG_TDT: u32 = 0x3818;
const REG_RAL: u32 = 0x5400;
const REG_RAH: u32 = 0x5404;

const CTRL_SLU: u32 = 1 << 6;
const CTRL_RST: u32 = 1 << 26;

const RCTL_EN: u32 = 1 << 1;
const RCTL_SBP: u32 = 1 << 2;
const RCTL_UPE: u32 = 1 << 3;
const RCTL_MPE: u32 = 1 << 4;
const RCTL_BAM: u32 = 1 << 15;
const RCTL_BSIZE_2048: u32 = 0 << 16;
const RCTL_SECRC: u32 = 1 << 26;

const TCTL_EN: u32 = 1 << 1;
const TCTL_PSP: u32 = 1 << 3;
const TCTL_CT: u32 = 0x0F << 4;
const TCTL_COLD: u32 = 0x40 << 12;

const RX_STATUS_DD: u8 = 1 << 0;
const TX_CMD_EOP: u8 = 1 << 0;
const TX_CMD_IFCS: u8 = 1 << 1;
const TX_CMD_RS: u8 = 1 << 3;
const TX_STATUS_DD: u8 = 1 << 0;

#[repr(C, packed)]
#[derive(Clone, Copy, Default)]
struct RxDesc {
    addr: u64,
    length: u16,
    checksum: u16,
    status: u8,
    errors: u8,
    special: u16,
}

#[repr(C, packed)]
#[derive(Clone, Copy, Default)]
struct TxDesc {
    addr: u64,
    length: u16,
    cso: u8,
    cmd: u8,
    status: u8,
    css: u8,
    special: u16,
}

#[repr(align(16))]
struct RxDescRing([RxDesc; RING_SIZE]);

#[repr(align(16))]
struct TxDescRing([TxDesc; RING_SIZE]);

#[repr(align(4096))]
struct RxPacketBuffers([[u8; PACKET_BUF_SIZE]; RING_SIZE]);

#[repr(align(4096))]
struct TxPacketBuffers([[u8; PACKET_BUF_SIZE]; RING_SIZE]);

struct E1000Context {
    pci_dev: Option<PciDevice>,
    mmio: *mut u8,
    rx_next: usize,
    tx_next: usize,
}

static mut E1000_CTX: E1000Context = E1000Context {
    pci_dev: None,
    mmio: core::ptr::null_mut(),
    rx_next: 0,
    tx_next: 0,
};

static mut RX_DESC_RING: RxDescRing = RxDescRing([RxDesc {
    addr: 0,
    length: 0,
    checksum: 0,
    status: 0,
    errors: 0,
    special: 0,
}; RING_SIZE]);

static mut TX_DESC_RING: TxDescRing = TxDescRing([TxDesc {
    addr: 0,
    length: 0,
    cso: 0,
    cmd: 0,
    status: 0,
    css: 0,
    special: 0,
}; RING_SIZE]);

static mut RX_PACKET_BUFFERS: RxPacketBuffers =
    RxPacketBuffers([[0; PACKET_BUF_SIZE]; RING_SIZE]);

static mut TX_PACKET_BUFFERS: TxPacketBuffers =
    TxPacketBuffers([[0; PACKET_BUF_SIZE]; RING_SIZE]);

pub struct E1000;

impl E1000 {
    pub fn matches(device: &PciDevice) -> bool {
        device.vendor_id == E1000_VENDOR && device.device_id == E1000_DEVICE_82540EM
    }
}

impl NetworkDriver for E1000 {
    fn name(&self) -> &'static str {
        "e1000"
    }

    fn mmio_size(&self) -> u64 {
        MMIO_SIZE
    }

    unsafe fn map_dma_buffers(
        &self,
        pml4: &mut crate::memory::PageTable,
        allocator: &mut FrameAllocator,
    ) {
        let flags = PAGE_WRITABLE | PAGE_PRESENT;
        let regions: &[(u64, usize)] = &[
            (
                addr_of_mut!(RX_DESC_RING) as u64,
                core::mem::size_of::<RxDescRing>(),
            ),
            (
                addr_of_mut!(TX_DESC_RING) as u64,
                core::mem::size_of::<TxDescRing>(),
            ),
            (
                addr_of_mut!(RX_PACKET_BUFFERS) as u64,
                core::mem::size_of::<RxPacketBuffers>(),
            ),
            (
                addr_of_mut!(TX_PACKET_BUFFERS) as u64,
                core::mem::size_of::<TxPacketBuffers>(),
            ),
        ];

        for &(base, size) in regions {
            let pages = (size + 4095) / 4096;
            for i in 0..pages as u64 {
                let addr = base + i * 4096;
                crate::memory::map_page(pml4, addr, addr, flags, allocator);
            }
        }
    }

    unsafe fn init(&mut self, device: PciDevice) {
        let ctx = &mut *addr_of_mut!(E1000_CTX);

        let bar = crate::pci::mmio_bar0(&device);

        ctx.pci_dev = Some(device);
        ctx.mmio = bar as *mut u8;
        ctx.rx_next = 0;
        ctx.tx_next = 0;

        println!("e1000: MMIO at {:#x}", bar);

        reset(ctx.mmio);
        setup_mac(ctx.mmio);
        setup_rx_ring(ctx.mmio);
        setup_tx_ring(ctx.mmio);

        write_reg(
            ctx.mmio,
            REG_CTRL,
            read_reg(ctx.mmio, REG_CTRL) | CTRL_SLU,
        );

        let status = read_reg(ctx.mmio, REG_STATUS);
        println!(
            "e1000: link up, rx/tx rings ready (status={:#x})",
            status
        );
    }

    unsafe fn transmit(&mut self, data: &[u8]) -> bool {
        let ctx = &mut *addr_of_mut!(E1000_CTX);
        if ctx.mmio.is_null() {
            return false;
        }

        if data.len() > PACKET_BUF_SIZE {
            return false;
        }

        let idx = ctx.tx_next;
        let tx_ring = &mut *addr_of_mut!(TX_DESC_RING);
        let desc = &mut tx_ring.0[idx];

        if (desc.status & TX_STATUS_DD) == 0 && desc.cmd != 0 {
            return false;
        }

        let buf = &mut (*addr_of_mut!(TX_PACKET_BUFFERS)).0[idx];
        buf[..data.len()].copy_from_slice(data);

        desc.addr = buf.as_ptr() as u64;
        desc.length = data.len() as u16;
        desc.cmd = TX_CMD_EOP | TX_CMD_IFCS | TX_CMD_RS;
        desc.status = 0;

        ctx.tx_next = (idx + 1) % RING_SIZE;
        write_reg(ctx.mmio, REG_TDT, ctx.tx_next as u32);

        true
    }

    unsafe fn poll_rx(&mut self, out: &mut [u8]) -> usize {
        let ctx = &mut *addr_of_mut!(E1000_CTX);
        if ctx.mmio.is_null() {
            return 0;
        }

        let idx = ctx.rx_next;
        let rx_ring = &*addr_of_mut!(RX_DESC_RING);
        let desc = &rx_ring.0[idx];

        if (desc.status & RX_STATUS_DD) == 0 {
            return 0;
        }

        let len = desc.length as usize;
        let copy_len = len.min(out.len()).min(PACKET_BUF_SIZE);
        let buf = &(*addr_of_mut!(RX_PACKET_BUFFERS)).0[idx];
        out[..copy_len].copy_from_slice(&buf[..copy_len]);

        let rx_ring_mut = &mut *addr_of_mut!(RX_DESC_RING);
        rx_ring_mut.0[idx].status = 0;
        write_reg(ctx.mmio, REG_RDT, idx as u32);

        ctx.rx_next = (idx + 1) % RING_SIZE;
        copy_len
    }

    unsafe fn mac_address(&self) -> [u8; 6] {
        let ctx = &*addr_of_mut!(E1000_CTX);
        if ctx.mmio.is_null() {
            return [0u8; 6];
        }
        read_mac(ctx.mmio)
    }
}

unsafe fn read_reg(mmio: *mut u8, offset: u32) -> u32 {
    read_volatile(mmio.add(offset as usize) as *const u32)
}

unsafe fn write_reg(mmio: *mut u8, offset: u32, val: u32) {
    write_volatile(mmio.add(offset as usize) as *mut u32, val);
}

unsafe fn reset(mmio: *mut u8) {
    write_reg(mmio, REG_CTRL, read_reg(mmio, REG_CTRL) | CTRL_RST);
    while (read_reg(mmio, REG_CTRL) & CTRL_RST) != 0 {}
}

unsafe fn setup_mac(mmio: *mut u8) {
    // QEMU e1000 default MAC 52:54:00:12:34:56, packed as the hardware expects:
    //   RAL[31:0]  = bytes 0-3 of MAC => 52:54:00:12 => 0x1200_5452 (little-endian)
    //   RAH[15:0]  = bytes 4-5 of MAC => 34:56       => 0x5634      (little-endian)
    //   RAH[31]    = Address Valid bit
    write_reg(mmio, REG_RAL, 0x1200_5452_u32);
    write_reg(mmio, REG_RAH, 0x8000_5634_u32);
}

/// Read the MAC address that was programmed into the Receive Address registers.
pub unsafe fn read_mac(mmio: *mut u8) -> [u8; 6] {
    let ral = read_reg(mmio, REG_RAL);
    let rah = read_reg(mmio, REG_RAH);
    [
        (ral & 0xFF) as u8,
        ((ral >> 8) & 0xFF) as u8,
        ((ral >> 16) & 0xFF) as u8,
        ((ral >> 24) & 0xFF) as u8,
        (rah & 0xFF) as u8,
        ((rah >> 8) & 0xFF) as u8,
    ]
}

unsafe fn setup_rx_ring(mmio: *mut u8) {
    let rx_ring = addr_of_mut!(RX_DESC_RING);
    let rx_bufs = addr_of_mut!(RX_PACKET_BUFFERS);

    for i in 0..RING_SIZE {
        let buf_addr = unsafe { (*rx_bufs).0[i].as_ptr() as u64 };
        unsafe {
            (*rx_ring).0[i].addr = buf_addr;
            (*rx_ring).0[i].status = 0;
        }
    }

    let ring_addr = rx_ring as u64;
    write_reg(mmio, REG_RDBAL, ring_addr as u32);
    write_reg(mmio, REG_RDBAH, (ring_addr >> 32) as u32);
    write_reg(
        mmio,
        REG_RDLEN,
        (RING_SIZE * core::mem::size_of::<RxDesc>()) as u32,
    );
    write_reg(mmio, REG_RDH, 0);
    write_reg(mmio, REG_RDT, (RING_SIZE - 1) as u32);

    write_reg(
        mmio,
        REG_RCTL,
        RCTL_EN | RCTL_SBP | RCTL_UPE | RCTL_MPE | RCTL_BAM | RCTL_BSIZE_2048 | RCTL_SECRC,
    );
}

unsafe fn setup_tx_ring(mmio: *mut u8) {
    let tx_ring = addr_of_mut!(TX_DESC_RING);
    let ring_addr = tx_ring as u64;

    write_reg(mmio, REG_TDBAL, ring_addr as u32);
    write_reg(mmio, REG_TDBAH, (ring_addr >> 32) as u32);
    write_reg(
        mmio,
        REG_TDLEN,
        (RING_SIZE * core::mem::size_of::<TxDesc>()) as u32,
    );
    write_reg(mmio, REG_TDH, 0);
    write_reg(mmio, REG_TDT, 0);

    write_reg(mmio, REG_TIPG, 0x0060_080C);
    write_reg(mmio, REG_TCTL, TCTL_EN | TCTL_PSP | TCTL_CT | TCTL_COLD);
}