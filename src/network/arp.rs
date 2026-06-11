use crate::{network::transmit, println, std::stdio::print};

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
    pub opcode: u16,         // 1 = Request, 2 = Reply
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

    if ethertype == 0x0806 && bytes_received >= core::mem::size_of::<ArpFrame>() {
        let arp_packet = &*(rx_buffer.as_ptr().add(core::mem::size_of::<EthernetHeader>()) as *const ArpPacket);
        let opcode = u16::from_be(arp_packet.opcode);

        //  (Opcode 1 = Request)
        if opcode == 1 && arp_packet.target_ip == my_ip {
            println!("network: ARP Request received! Responding");
            
            // 3. create ARP Reply packet
            let reply_frame = ArpFrame {
                eth: EthernetHeader {
                    dest_mac: arp_packet.sender_mac, 
                    src_mac: my_mac,
                    ethertype: 0x0806_u16.to_be(),
                },
                arp: ArpPacket {
                    hardware_type: 1_u16.to_be(),
                    protocol_type: 0x0800_u16.to_be(),
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
            core::mem::forget(reply_frame);
        } 
        else if opcode == 2 {
            println!(
                "network: ARP notification - The MAC address of IP {:?} is {:?}.",
                arp_packet.sender_ip, arp_packet.sender_mac
            );
        }
    }
}