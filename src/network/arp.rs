use crate::{
    network::{
        helper::calculate_checksum,
        ipv4::{IcmpPacket, Ipv4Header},
        transmit,
    },
    println,
};

#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct EthernetHeader {
    pub dest_mac: [u8; 6],
    pub src_mac: [u8; 6],
    pub ethertype: u16, // ARP 0x0806, IPv4 0x0800
}

#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct ArpPacket {
    pub hardware_type: u16, // ethernet 1 (0x0001)
    pub protocol_type: u16, // IPv4 0x0800
    pub hw_addr_len: u8,    // MAC address length = 6
    pub proto_addr_len: u8, // IP address length = 4
    pub opcode: u16,        // 1 = Request, 2 = Reply
    pub sender_mac: [u8; 6],
    pub sender_ip: [u8; 4],
    pub target_mac: [u8; 6],
    pub target_ip: [u8; 4],
}

#[repr(C, packed)]
pub struct ArpFrame {
    pub eth: EthernetHeader,
    pub arp: ArpPacket,
}

pub unsafe fn send_arp_request(target_ip: [u8; 4], my_ip: [u8; 4], my_mac: [u8; 6]) {
    let frame = ArpFrame {
        eth: EthernetHeader {
            dest_mac: [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF], // broadcast
            src_mac: my_mac,
            ethertype: 0x0806_u16.to_be(), // network byteorder translation
        },
        arp: ArpPacket {
            hardware_type: 1_u16.to_be(),
            protocol_type: 0x0800_u16.to_be(),
            hw_addr_len: 6,
            proto_addr_len: 4,
            opcode: 1_u16.to_be(), // 1 = Request
            sender_mac: my_mac,
            sender_ip: my_ip,
            target_mac: [0x00; 6],
            target_ip: target_ip,
        },
    };
    let data = core::slice::from_raw_parts(
        &frame as *const ArpFrame as *const u8,
        core::mem::size_of::<ArpFrame>(),
    );
    transmit(data);
    core::mem::forget(frame);
}

pub unsafe fn handle_incoming_packets(my_ip: [u8; 4], my_mac: [u8; 6]) {
    let mut rx_buffer = [0u8; 1514];
    let bytes_received = crate::network::poll_rx(&mut rx_buffer);

    if bytes_received < core::mem::size_of::<EthernetHeader>() {
        return;
    }

    let eth_header = &*(rx_buffer.as_ptr() as *const EthernetHeader);
    let ethertype = u16::from_be(eth_header.ethertype);

    if ethertype == 0x0806_u16 {
        let arp_packet = &*(rx_buffer
            .as_ptr()
            .add(core::mem::size_of::<EthernetHeader>())
            as *const ArpPacket);
        let opcode = u16::from_be(arp_packet.opcode);

        if opcode == 1 && arp_packet.target_ip == my_ip {
            // Cache the sender's mapping from any incoming ARP request
            crate::network::arp_cache_insert(arp_packet.sender_ip, arp_packet.sender_mac);

            let mut reply_frame = ArpFrame {
                eth: EthernetHeader {
                    dest_mac: arp_packet.sender_mac,
                    src_mac: my_mac,
                    ethertype: 0x0806_u16.to_be(),
                },
                arp: ArpPacket {
                    hardware_type: 1u16.to_be(),
                    protocol_type: 0x0800u16.to_be(),
                    hw_addr_len: 6,
                    proto_addr_len: 4,
                    opcode: 2u16.to_be(), // Reply
                    sender_mac: my_mac,
                    sender_ip: my_ip,
                    target_mac: arp_packet.sender_mac,
                    target_ip: arp_packet.sender_ip,
                },
            };

            let reply_data = core::slice::from_raw_parts(
                &reply_frame as *const ArpFrame as *const u8,
                core::mem::size_of::<ArpFrame>(),
            );

            crate::network::transmit(reply_data);
        } else if opcode == 2 {
            // ARP Reply → cache the sender's mapping
            crate::network::arp_cache_insert(arp_packet.sender_ip, arp_packet.sender_mac);
        }
    } else if ethertype == 0x0800_u16 {
        let ip_offset = core::mem::size_of::<EthernetHeader>();
        if bytes_received < ip_offset + core::mem::size_of::<Ipv4Header>() {
            return;
        }

        let ip_header_ptr = rx_buffer.as_mut_ptr().add(ip_offset) as *mut Ipv4Header;
        let ip_header = &mut *ip_header_ptr;

        if ip_header.dst_ip == my_ip && ip_header.protocol == 1 {
            let ihl = (ip_header.ver_ihl & 0x0F) as usize * 4;
            let icmp_offset = ip_offset + ihl;

            if bytes_received < icmp_offset + core::mem::size_of::<IcmpPacket>() {
                return;
            }

            let icmp_packet_ptr = rx_buffer.as_mut_ptr().add(icmp_offset) as *mut IcmpPacket;
            let icmp_packet = &mut *icmp_packet_ptr;

            if icmp_packet.icmp_type == 8 {
                // Echo Request → auto-reply
                icmp_packet.icmp_type = 0;
                icmp_packet.checksum = 0;

                let total_length = u16::from_be(ip_header.total_length) as usize;
                let icmp_len = total_length - ihl;

                let icmp_bytes =
                    core::slice::from_raw_parts_mut(icmp_packet_ptr as *mut u8, icmp_len);
                icmp_packet.checksum = calculate_checksum(icmp_bytes).to_be();

                let temp_ip = ip_header.src_ip;
                ip_header.src_ip = my_ip;
                ip_header.dst_ip = temp_ip;
                ip_header.header_checksum = 0;

                let ip_bytes = core::slice::from_raw_parts_mut(ip_header_ptr as *mut u8, ihl);
                ip_header.header_checksum = calculate_checksum(ip_bytes).to_be();

                let eth_header_mut = rx_buffer.as_mut_ptr() as *mut EthernetHeader;
                (*eth_header_mut).dest_mac = eth_header.src_mac;
                (*eth_header_mut).src_mac = my_mac;

                let send_data = &rx_buffer[0..ip_offset + total_length];
                crate::network::transmit(send_data);
            } else if icmp_packet.icmp_type == 0 {
                // Echo Reply → buffer for userland
                let total_length = u16::from_be(ip_header.total_length) as usize;
                let icmp_len = total_length - ihl;
                let payload_len = if icmp_len > 8 { (icmp_len - 8).min(64) } else { 0 };

                let mut payload = [0u8; 64];
                if payload_len > 0 {
                    core::ptr::copy_nonoverlapping(
                        (icmp_packet_ptr as *const u8).add(8),
                        payload.as_mut_ptr(),
                        payload_len,
                    );
                }

                let reply = crate::network::IcmpEchoReply {
                    src_ip: ip_header.src_ip,
                    identifier: u16::from_be(icmp_packet.identifier),
                    sequence: u16::from_be(icmp_packet.sequence_number),
                    payload_len: payload_len as u16,
                    payload,
                };
                crate::network::push_icmp_reply(reply);
            }
        }
    }
}
