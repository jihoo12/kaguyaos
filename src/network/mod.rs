mod driver;
mod e1000;
mod helper;
pub mod ipv4;

use crate::memory::{FrameAllocator, PageTable, PAGE_CACHE_DISABLE, PAGE_PRESENT, PAGE_WRITABLE};
use crate::pci::{self, PciDevice};
use crate::println;
use core::ptr::addr_of_mut;
use core::sync::atomic::{AtomicBool, Ordering};
pub use driver::NetworkDriver;

pub mod arp;

// ── ARP cache ─────────────────────────────────────────────────────────────

const ARP_CACHE_SIZE: usize = 8;

#[derive(Clone, Copy)]
struct ArpEntry {
    ip: [u8; 4],
    mac: [u8; 6],
    valid: bool,
}

static mut ARP_CACHE: [ArpEntry; ARP_CACHE_SIZE] = [ArpEntry {
    ip: [0u8; 4],
    mac: [0u8; 6],
    valid: false,
}; ARP_CACHE_SIZE];

/// Record an ARP mapping from an incoming ARP reply or request.
pub unsafe fn arp_cache_insert(ip: [u8; 4], mac: [u8; 6]) {
    // Update existing entry or fill an empty slot
    for i in 0..ARP_CACHE_SIZE {
        if ARP_CACHE[i].valid && ARP_CACHE[i].ip == ip {
            ARP_CACHE[i].mac = mac;
            return;
        }
    }
    for i in 0..ARP_CACHE_SIZE {
        if !ARP_CACHE[i].valid {
            ARP_CACHE[i].ip = ip;
            ARP_CACHE[i].mac = mac;
            ARP_CACHE[i].valid = true;
            return;
        }
    }
}

/// Look up a MAC in the ARP cache. Returns None if not found.
pub unsafe fn arp_cache_lookup(ip: [u8; 4]) -> Option<[u8; 6]> {
    for i in 0..ARP_CACHE_SIZE {
        if ARP_CACHE[i].valid && ARP_CACHE[i].ip == ip {
            return Some(ARP_CACHE[i].mac);
        }
    }
    None
}

/// Send an ARP request and spin-wait for the reply.
/// Returns the resolved MAC, or None on timeout.
pub unsafe fn arp_resolve(target_ip: [u8; 4]) -> Option<[u8; 6]> {
    // Check cache first
    if let Some(mac) = arp_cache_lookup(target_ip) {
        return Some(mac);
    }

    let my_ip = get_ip_address()?;
    let my_mac = get_mac_address()?;

    arp::send_arp_request(target_ip, my_ip, my_mac);

    // Spin-wait: poll for ARP reply
    for _ in 0..200_000 {
        arp::handle_incoming_packets(my_ip, my_mac);
        if let Some(mac) = arp_cache_lookup(target_ip) {
            return Some(mac);
        }
    }
    None
}

static mut ACTIVE_NIC: Option<driver::Nic> = None;

/// Statically configured IPv4 address for this host (set via `set_ip_address`).
static mut HOST_IP: [u8; 4] = [0u8; 4];

// ── ICMP Echo Reply ring buffer ─────────────────────────────────────────────

const ICMP_RX_CAPACITY: usize = 16;

#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct IcmpEchoReply {
    pub src_ip: [u8; 4],
    pub identifier: u16,
    pub sequence: u16,
    pub payload_len: u16,
    pub payload: [u8; 64],
}

static mut ICMP_RX_RING: [IcmpEchoReply; ICMP_RX_CAPACITY] = [IcmpEchoReply {
    src_ip: [0u8; 4],
    identifier: 0,
    sequence: 0,
    payload_len: 0,
    payload: [0u8; 64],
}; ICMP_RX_CAPACITY];
static mut ICMP_RX_HEAD: usize = 0;
static mut ICMP_RX_TAIL: usize = 0;
static ICMP_RX_LOCK: crate::interrupts::InterruptSpinlock<()> =
    crate::interrupts::InterruptSpinlock::new(());

/// Push an ICMP Echo Reply into the ring buffer (called from AP poll).
unsafe fn push_icmp_reply(reply: IcmpEchoReply) {
    let _guard = ICMP_RX_LOCK.lock();
    let next = (ICMP_RX_HEAD + 1) % ICMP_RX_CAPACITY;
    if next == ICMP_RX_TAIL {
        return; // full — drop
    }
    *addr_of_mut!(ICMP_RX_RING[ICMP_RX_HEAD]) = reply;
    ICMP_RX_HEAD = next;
}

/// Pop an ICMP Echo Reply from the ring buffer (called from userland syscall).
pub unsafe fn pop_icmp_reply(out: &mut IcmpEchoReply) -> bool {
    let _guard = ICMP_RX_LOCK.lock();
    if ICMP_RX_TAIL == ICMP_RX_HEAD {
        return false;
    }
    *out = ICMP_RX_RING[ICMP_RX_TAIL];
    ICMP_RX_TAIL = (ICMP_RX_TAIL + 1) % ICMP_RX_CAPACITY;
    true
}

/// Pop an ICMP Echo Reply from the ring buffer, copying raw bytes into `buf`.
/// Returns bytes copied, or 0 if buffer is empty.
pub unsafe fn pop_icmp_reply_raw(buf: &mut [u8]) -> usize {
    let _guard = ICMP_RX_LOCK.lock();
    if ICMP_RX_TAIL == ICMP_RX_HEAD {
        return 0;
    }
    let reply = ICMP_RX_RING[ICMP_RX_TAIL];
    ICMP_RX_TAIL = (ICMP_RX_TAIL + 1) % ICMP_RX_CAPACITY;
    let copy_len = core::mem::size_of::<IcmpEchoReply>().min(buf.len());
    core::ptr::copy_nonoverlapping(
        &reply as *const IcmpEchoReply as *const u8,
        buf.as_mut_ptr(),
        copy_len,
    );
    copy_len
}

// ── ICMP Echo Request sender ────────────────────────────────────────────────

static PING_SEQ: core::sync::atomic::AtomicU16 = core::sync::atomic::AtomicU16::new(1);

/// Build and send an ICMP Echo Request to `target_ip`.
/// Returns the sequence number used, or 0 on failure.
pub unsafe fn send_icmp_echo_request(target_ip: [u8; 4]) -> u16 {
    let my_ip = match get_ip_address() {
        Some(ip) => ip,
        None => return 0,
    };
    let my_mac = match get_mac_address() {
        Some(mac) => mac,
        None => return 0,
    };

    // Resolve target MAC via ARP
    let target_mac = match arp_resolve(target_ip) {
        Some(mac) => mac,
        None => {
            return 0;
        }
    };

    let seq = PING_SEQ.fetch_add(1, Ordering::Relaxed);
    let ident: u16 = 0xBEEF;

    // ICMP Echo Request payload: 48 bytes of timestamp-like data
    let mut payload = [0u8; 48];
    let mut ts: u32;
    unsafe {
        let lo: u32;
        core::arch::asm!("rdtsc", out("eax") lo, out("edx") _);
        ts = lo;
    }
    payload[0] = (ts >> 24) as u8;
    payload[1] = (ts >> 16) as u8;
    payload[2] = (ts >> 8) as u8;
    payload[3] = ts as u8;
    for i in 4..48 {
        payload[i] = (i & 0xFF) as u8;
    }

    // Build ICMP header + payload
    let mut icmp_buf = [0u8; 8 + 48]; // ICMP header (8 bytes) + payload (48 bytes)
    icmp_buf[0] = 8; // type 8 = Echo Request
    icmp_buf[1] = 0; // code 0
    icmp_buf[2] = 0; // checksum high
    icmp_buf[3] = 0; // checksum low
    icmp_buf[4] = (ident >> 8) as u8;
    icmp_buf[5] = ident as u8;
    icmp_buf[6] = (seq >> 8) as u8;
    icmp_buf[7] = seq as u8;
    icmp_buf[8..56].copy_from_slice(&payload);

    // ICMP checksum
    let cksum = helper::calculate_checksum(&icmp_buf);
    icmp_buf[2] = (cksum >> 8) as u8;
    icmp_buf[3] = cksum as u8;

    // IP header
    let total_len = 20 + icmp_buf.len();
    let mut ip_buf = [0u8; 20];
    ip_buf[0] = 0x45; // ver=4, ihl=5
    ip_buf[1] = 0;    // tos
    ip_buf[2] = (total_len >> 8) as u8;
    ip_buf[3] = total_len as u8;
    ip_buf[4] = 0; // id high
    ip_buf[5] = 0; // id low
    ip_buf[6] = 0x40; // flags: Don't Fragment
    ip_buf[7] = 0x00;
    ip_buf[8] = 64;  // TTL
    ip_buf[9] = 1;   // protocol: ICMP
    ip_buf[10] = 0;  // header checksum (filled later)
    ip_buf[11] = 0;
    ip_buf[12..16].copy_from_slice(&my_ip);
    ip_buf[16..20].copy_from_slice(&target_ip);

    let ip_cksum = helper::calculate_checksum(&ip_buf);
    ip_buf[10] = (ip_cksum >> 8) as u8;
    ip_buf[11] = ip_cksum as u8;

    // Ethernet frame
    let mut eth_buf = [0u8; 14 + 20 + 56];
    // Use ARP-resolved target MAC
    eth_buf[0..6].copy_from_slice(&target_mac);
    eth_buf[6..12].copy_from_slice(&my_mac);
    eth_buf[12] = 0x08; // ethertype: IPv4
    eth_buf[13] = 0x00;
    eth_buf[14..34].copy_from_slice(&ip_buf);
    eth_buf[34..90].copy_from_slice(&icmp_buf);

    transmit(&eth_buf);
    seq
}

// ── Network poll (called by AP) ─────────────────────────────────────────────

/// Poll the NIC for one frame and dispatch it.
/// - ARP Request -> auto-reply
/// - ICMP Echo Request (type 8) -> auto-reply
/// - ICMP Echo Reply (type 0) -> buffer for userland
pub unsafe fn poll() {
    if !is_ready() {
        return;
    }
    let my_ip = match get_ip_address() {
        Some(ip) => ip,
        None => return,
    };
    let my_mac = match get_mac_address() {
        Some(mac) => mac,
        None => return,
    };
    arp::handle_incoming_packets(my_ip, my_mac);
}

// ── Existing accessors ──────────────────────────────────────────────────────

pub fn is_ready() -> bool {
    unsafe { (*addr_of_mut!(ACTIVE_NIC)).is_some() }
}

pub unsafe fn set_ip_address(ip: [u8; 4]) {
    *addr_of_mut!(HOST_IP) = ip;
}

pub fn get_ip_address() -> Option<[u8; 4]> {
    let ip = unsafe { *addr_of_mut!(HOST_IP) };
    if ip == [0u8; 4] { None } else { Some(ip) }
}

pub unsafe fn get_mac_address() -> Option<[u8; 6]> {
    match &*addr_of_mut!(ACTIVE_NIC) {
        Some(nic) => Some(nic.mac_address()),
        None => None,
    }
}

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

pub unsafe fn transmit(data: &[u8]) -> bool {
    match &mut *addr_of_mut!(ACTIVE_NIC) {
        Some(nic) => nic.transmit(data),
        None => false,
    }
}

pub unsafe fn poll_rx(out: &mut [u8]) -> usize {
    match &mut *addr_of_mut!(ACTIVE_NIC) {
        Some(nic) => nic.poll_rx(out),
        None => 0,
    }
}