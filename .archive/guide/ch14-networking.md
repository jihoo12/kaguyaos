# Chapter 14: Networking

This chapter adds network connectivity: an E1000 Ethernet driver, ARP for address resolution, and ICMP for ping. Your OS can now talk to the outside world.

## The Network Stack

```
User program (ping.kef)
    │ syscalls
    ▼
ICMP Echo Request/Reply
    │
    ▼
ARP (IP → MAC resolution)
    │
    ▼
Ethernet frames
    │
    ▼
E1000 NIC driver (DMA rings)
    │
    ▼
Physical network (QEMU user-mode net)
```

## E1000 Initialization

The E1000 NIC uses MMIO registers for control and DMA descriptor rings for data:

```rust
// src/drivers/net/e1000.rs (simplified)
const RING_SIZE: usize = 16;

pub struct E1000 {
    mmio_base: u64,
    mac: [u8; 6],
    rx_ring: *mut RxDesc,
    tx_ring: *mut TxDesc,
    rx_index: usize,
    tx_index: usize,
}
```

**Receive descriptors** describe buffers where the NIC writes incoming packets.
**Transmit descriptors** describe buffers the NIC reads to send outgoing packets.

Each descriptor contains a physical address and length. The NIC DMA-transfers data directly to/from these buffers.

## ARP: IP to MAC Resolution

Ethernet needs MAC addresses, but applications use IP addresses. ARP (Address Resolution Protocol) maps IP → MAC:

```rust
// src/drivers/net/mod.rs
pub struct ArpEntry {
    pub ip: [u8; 4],
    pub mac: [u8; 6],
    pub valid: bool,
}

static ARP_CACHE: Spinlock<[ArpEntry; 8]> = Spinlock::new([ArpEntry {
    ip: [0; 4], mac: [0; 6], valid: false,
}; 8]);
```

The ARP cache is populated from both ARP requests and replies it sees on the network:

```rust
// src/drivers/net/arp.rs (simplified)
pub unsafe fn handle_incoming_packets() {
    let mut buffer = [0u8; 2048];
    let len = crate::drivers::net::poll_rx(&mut buffer);

    if len > 0 && is_arp(&buffer) {
        let arp = parse_arp(&buffer);
        match arp.opcode {
            1 => {  // ARP Request
                // Cache sender's IP→MAC
                arp_cache_insert(arp.sender_ip, arp.sender_mac);
                // Send ARP reply if they're asking about us
                if arp.target_ip == MY_IP {
                    send_arp_reply(arp);
                }
            }
            2 => {  // ARP Reply
                arp_cache_insert(arp.sender_ip, arp.sender_mac);
            }
        }
    }
}
```

## Sending a Ping

The ping utility:
1. Resolves the target IP via ARP (spin-wait until we get a MAC)
2. Constructs an ICMP Echo Request packet
3. Wraps it in an IPv4 + Ethernet frame
4. Sends via the E1000 TX ring

```rust
// src/drivers/net/mod.rs (simplified)
pub unsafe fn send_icmp_echo_request(target_ip: [u8; 4]) {
    // 1. Resolve MAC via ARP
    let target_mac = arp_resolve(target_ip);  // Spin-wait

    // 2. Build ICMP Echo Request
    let icmp = IcmpPacket {
        type_code: 0x0800,  // Echo Request
        identifier: 0x1234,
        sequence: 1,
    };

    // 3. Build IPv4 header
    let ipv4 = Ipv4Header {
        src_ip: MY_IP,
        dst_ip: target_ip,
        protocol: 1,  // ICMP
        // ...
    };

    // 4. Build Ethernet frame
    let frame = build_ethernet_frame(target_mac, ETHERTYPE_IPV4, &ipv4, &icmp);

    // 5. Transmit
    transmit(&frame);
}
```

## Receiving ICMP Replies

Replies come from the AP's (Application Processor) network poller, which runs on the second CPU core:

```rust
// ICMP reply ring buffer
static ICMP_RX_LOCK: Spinlock<usize> = Spinlock::new(0);
static ICMP_RX_RING: Spinlock<[IcmpEchoReply; 16]> = /* ... */;

pub fn push_icmp_reply(reply: IcmpEchoReply) {
    let mut lock = ICMP_RX_RING.lock();
    let idx = ICMP_RX_LOCK.lock();
    lock[*idx % 16] = reply;
    *idx += 1;
}

pub fn pop_icmp_reply_raw(buf: &mut [u8]) -> usize {
    // Read from ring buffer, copy raw bytes for userland
}
```

The user program polls via syscall 24 (`net_recv_ping`) and yields between attempts.

## QEMU User-Mode Networking

QEMU's `-netdev user` provides NAT networking. By default:
- Guest IP: 10.0.2.15
- Host gateway: 10.0.2.2
- UDP port 5555 → forwarded to host

The ARP cache handles the IP→MAC resolution for the QEMU virtual network.

## kaguyaOS Reference

| File | Lines | What it does |
|------|-------|-------------|
| `src/drivers/net/mod.rs` | 1-338 | Network init, ARP cache, ICMP ring |
| `src/drivers/net/arp.rs` | 1-185 | ARP request/reply handling |
| `src/drivers/net/e1000.rs` | 1-361 | E1000 NIC driver |
| `src/drivers/net/ipv4.rs` | 1-40 | IPv4 + ICMP packet types |
| `user/src/ping.rs` | 1-267 | User-space ping utility |

---

**Next:** [Chapter 15 — Symmetric Multi-Processing](ch15-smp.md) — Running code on multiple CPU cores.
