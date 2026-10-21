#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct Ipv4Header {
    pub ver_ihl: u8,         // Version (4-bit) + Header Length (4-bit) -> Usually 0x45
    pub tos: u8,             // Service type (usually 0)
    pub total_length: u16,    // Total size combining IP header + higher-level protocol data
    pub identification: u16, // Packet ID (usually 0 or a sequentially increasing value)
    pub flags_fragment: u16, // Fragmentation flag (usually 0x4000 - Don't Fragment)
    pub ttl: u8,             // Time To Live (usally 64 or 128)
    pub protocol: u8,        // Higher-level protocol (ICMP is 1, TCP is 6, UDP is 17)
    pub header_checksum: u16,// IP header checksum (calculation required for verification)
    pub src_ip: [u8; 4],     // Source IP (My IP)
    pub dst_ip: [u8; 4],     // Destination IP (Recipient IP)
}

#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct IcmpPacket {
    pub icmp_type: u8,       // 8 = Echo Request, 0 = Echo Reply
    pub icmp_code: u8,       // Usually 0
    pub checksum: u16,       // ICMP packet checksum
    pub identifier: u16,     // ping process id
    pub sequence_number: u16,// packet sequence number
    // Variable data (payload) can be appended after this.
}

/// The address whose MAC an IPv4 packet for `dst_ip` is sent to.
///
/// Routing is intentionally kept here so ICMP, UDP, TCP and future IPv4
/// protocols share the same next-hop decision.
pub fn next_hop(dst_ip: [u8; 4]) -> Option<[u8; 4]> {
    const NETMASK: [u8; 4] = [255, 255, 255, 0];
    const DEFAULT_GATEWAY: [u8; 4] = [10, 0, 2, 2];
    let my_ip = super::get_ip_address()?;
    let same_subnet = (0..4).all(|i| (my_ip[i] & NETMASK[i]) == (dst_ip[i] & NETMASK[i]));
    Some(if same_subnet { dst_ip } else { DEFAULT_GATEWAY })
}

/// Build and transmit one Ethernet + IPv4 packet, blocking for up to a
/// second while ARP resolves the next hop.
pub unsafe fn transmit_ipv4(dst_ip: [u8; 4], protocol: u8, payload: &[u8]) -> bool { unsafe {
    match next_hop(dst_ip).and_then(|hop| super::arp_resolve(hop)) {
        Some(dst_mac) => transmit_to(dst_mac, dst_ip, protocol, payload),
        None => false,
    }
}}

/// Like transmit_ipv4, but never blocks: when the next hop is not in the ARP
/// cache, request it and drop the packet. Safe from the RX and timer paths.
pub unsafe fn transmit_ipv4_cached(dst_ip: [u8; 4], protocol: u8, payload: &[u8]) -> bool { unsafe {
    let Some(hop) = next_hop(dst_ip) else {
        return false;
    };
    match super::arp_cache_lookup(hop) {
        Some(dst_mac) => transmit_to(dst_mac, dst_ip, protocol, payload),
        None => {
            if let (Some(ip), Some(mac)) = (super::get_ip_address(), super::get_mac_address()) {
                super::arp::send_arp_request(hop, ip, mac);
            }
            false
        }
    }
}}

unsafe fn transmit_to(dst_mac: [u8; 6], dst_ip: [u8; 4], protocol: u8, payload: &[u8]) -> bool { unsafe {
    if payload.len() > 1500 - 20 {
        return false;
    }
    let Some(my_ip) = super::get_ip_address() else {
        return false;
    };
    let Some(my_mac) = super::get_mac_address() else {
        return false;
    };

    let total_len = 20 + payload.len();
    let mut frame = [0u8; 1514];
    frame[0..6].copy_from_slice(&dst_mac);
    frame[6..12].copy_from_slice(&my_mac);
    frame[12..14].copy_from_slice(&0x0800u16.to_be_bytes());

    let header = &mut frame[14..34];
    header[0] = 0x45;
    header[2..4].copy_from_slice(&(total_len as u16).to_be_bytes());
    header[6] = 0x40; // Don't Fragment
    header[8] = 64;
    header[9] = protocol;
    header[12..16].copy_from_slice(&my_ip);
    header[16..20].copy_from_slice(&dst_ip);
    let checksum = super::helper::calculate_checksum(header);
    header[10..12].copy_from_slice(&checksum.to_be_bytes());

    frame[34..34 + payload.len()].copy_from_slice(payload);
    super::transmit(&frame[..14 + total_len])
}}
