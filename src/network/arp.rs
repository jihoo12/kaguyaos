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
    // SAFETY: `frame` lives until end of this function; the slice does not outlive it.
    let data = core::slice::from_raw_parts(
        &frame as *const ArpFrame as *const u8,
        core::mem::size_of::<ArpFrame>(),
    );
    transmit(data);
    // Prevent the compiler from dropping (and zeroing) `frame` before transmit returns.
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

        // If someone asks me for my IP address (Opcode 1 = Request)
        if opcode == 1 && arp_packet.target_ip == my_ip {
            println!("network: ARP Request received! Responding");

            // 3. Generate ARP Reply packet (send via 1:1 unicast to the person who visited me)
            let mut reply_frame = ArpFrame {
                eth: EthernetHeader {
                    dest_mac: arp_packet.sender_mac, // The MAC address of the person who asked me
                    src_mac: my_mac,
                    ethertype: 0x0806_u16.to_be(),
                },
                arp: ArpPacket {
                    hardware_type: 1u16.to_be(),
                    protocol_type: 0x0800u16.to_be(),
                    hw_addr_len: 6,
                    proto_addr_len: 4,
                    opcode: 2u16.to_be(), // 2 = Reply
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
        }
    } else if ethertype == 0x0800_u16 {
        let ip_offset = core::mem::size_of::<EthernetHeader>();
        if bytes_received < ip_offset + core::mem::size_of::<Ipv4Header>() {
            return;
        }

        // 1. IP Header Deserialization (Parsing)
        let ip_header_ptr = rx_buffer.as_mut_ptr().add(ip_offset) as *mut Ipv4Header;
        let ip_header = &mut *ip_header_ptr;

        // Check if the destination IP is mine and if the upper protocol is ICMP(1)
        if ip_header.dst_ip == my_ip && ip_header.protocol == 1 {
            // Calculate actual size based on IP header version (lower 4 bits of IHL field * 4)
            let ihl = (ip_header.ver_ihl & 0x0F) as usize * 4;
            let icmp_offset = ip_offset + ihl;

            if bytes_received < icmp_offset + core::mem::size_of::<IcmpPacket>() {
                return;
            }

            // 2. ICMP header parsing
            let icmp_packet_ptr = rx_buffer.as_mut_ptr().add(icmp_offset) as *mut IcmpPacket;
            let icmp_packet = &mut *icmp_packet_ptr;

            // If Type 8, it is a ping request (Echo Request).
            if icmp_packet.icmp_type == 8 {
                println!(
                    "network: Received a Ping request (Echo Request)! Preparing the response."
                );

                // --- 3. Convert to ICMP Echo Reply Packet ---
                icmp_packet.icmp_type = 0; // Type 0 = Echo Reply
                icmp_packet.checksum = 0; // Clear to 0 first for checksum recalculation

                // Calculate total ICMP length (total IP length - IP header length)
                let total_length = u16::from_be(ip_header.total_length) as usize;
                let icmp_len = total_length - ihl;

                // Recalculate the checksum of the ICMP area and substitute
                let icmp_bytes =
                    core::slice::from_raw_parts_mut(icmp_packet_ptr as *mut u8, icmp_len);
                icmp_packet.checksum = calculate_checksum(icmp_bytes).to_be();

                // --- 4. Modify IPv4 Header (Change Source/Destination) ---
                let temp_ip = ip_header.src_ip;
                ip_header.src_ip = my_ip;
                ip_header.dst_ip = temp_ip;
                ip_header.header_checksum = 0;

                let ip_bytes = core::slice::from_raw_parts_mut(ip_header_ptr as *mut u8, ihl);
                ip_header.header_checksum = calculate_checksum(ip_bytes).to_be();

                // --- 5. Modify Ethernet Header (Change Source/Destination) ---
                let eth_header_mut = rx_buffer.as_mut_ptr() as *mut EthernetHeader;
                (*eth_header_mut).dest_mac = eth_header.src_mac;
                (*eth_header_mut).src_mac = my_mac;

                // --- 6. Send the converted buffer as is ---
                let send_data = &rx_buffer[0..ip_offset + total_length];
                crate::network::transmit(send_data);
                println!("network: Ping response (Echo Reply) sent!");
            }
        }
    }
}
