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