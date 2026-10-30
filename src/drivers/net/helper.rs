#[cfg(feature = "jihoo-kernel")]
pub use crate::jihoo::{calculate_checksum, transport_checksum};

#[cfg(not(feature = "jihoo-kernel"))]
pub fn calculate_checksum(data: &[u8]) -> u16 {
    let mut sum = 0u32;
    let mut i = 0;

    while i < data.len() - 1 {
        let word = ((data[i] as u32) << 8) | (data[i + 1] as u32);
        sum += word;
        i += 2;
    }

    if i < data.len() {
        sum += (data[i] as u32) << 8;
    }

    while (sum >> 16) > 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    
    !(sum as u16)
}

/// TCP/UDP checksum over the IPv4 pseudo-header and `segment`. Computing it
/// over a segment that already carries a valid checksum yields 0.
#[cfg(not(feature = "jihoo-kernel"))]
pub fn transport_checksum(src: [u8; 4], dst: [u8; 4], protocol: u8, segment: &[u8]) -> u16 {
    let mut sum = 0u32;
    for pair in src.chunks(2).chain(dst.chunks(2)) {
        sum += u16::from_be_bytes([pair[0], pair[1]]) as u32;
    }
    sum += protocol as u32 + segment.len() as u32;
    let mut chunks = segment.chunks_exact(2);
    for pair in &mut chunks {
        sum += u16::from_be_bytes([pair[0], pair[1]]) as u32;
    }
    if let [last] = chunks.remainder() {
        sum += (*last as u32) << 8;
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !(sum as u16)
}
