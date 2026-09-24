#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

mod std;

fn sbPrint(s: &[u8]) {
    unsafe { std::print(core::str::from_utf8_unchecked(s)); }
}

fn put_char(ch: u8) {
    let buf = [ch];
    unsafe {
        std::print(core::str::from_utf8_unchecked(&buf));
    }
}

fn print_u32(mut val: u32) {
    if val == 0 {
        put_char(b'0');
        return;
    }
    let mut buf = [0u8; 10];
    let mut i = 10;
    while val > 0 {
        i -= 1;
        buf[i] = b'0' + (val % 10) as u8;
        val /= 10;
    }
    unsafe {
        std::print(core::str::from_utf8_unchecked(
            core::slice::from_raw_parts(buf.as_ptr().add(i), 10 - i),
        ));
    }
}

fn print_u64(mut val: u64) {
    if val == 0 {
        put_char(b'0');
        return;
    }
    let mut buf = [0u8; 20];
    let mut i = 20;
    while val > 0 {
        i -= 1;
        buf[i] = b'0' + (val % 10) as u8;
        val /= 10;
    }
    unsafe {
        std::print(core::str::from_utf8_unchecked(
            core::slice::from_raw_parts(buf.as_ptr().add(i), 20 - i),
        ));
    }
}

#[repr(C, packed)]
#[derive(Clone, Copy)]
struct IcmpEchoReply {
    src_ip: [u8; 4],
    identifier: u16,
    sequence: u16,
    payload_len: u16,
    payload: [u8; 64],
}

fn parse_ip(s: &str) -> Option<[u8; 4]> {
    let mut ip = [0u8; 4];
    let mut part = 0u16;
    let mut idx = 0;
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'0'..=b'9' => {
                part = part * 10 + (bytes[i] - b'0') as u16;
                if part > 255 {
                    return None;
                }
            }
            b'.' => {
                if idx >= 4 || part > 255 {
                    return None;
                }
                ip[idx] = part as u8;
                part = 0;
                idx += 1;
            }
            _ => return None,
        }
        i += 1;
    }
    if idx != 3 {
        return None;
    }
    ip[3] = part as u8;
    Some(ip)
}

fn print_ip(ip: [u8; 4]) {
    print_u32(ip[0] as u32);
    put_char(b'.');
    print_u32(ip[1] as u32);
    put_char(b'.');
    print_u32(ip[2] as u32);
    put_char(b'.');
    print_u32(ip[3] as u32);
}

fn newline() {
    let buf = [b'\n'];
    unsafe { std::print(core::str::from_utf8_unchecked(&buf)); }
}

#[unsafe(no_mangle)]
pub extern "C" fn _start(args_ptr: *const u8, args_len: usize) -> ! {
    let mut target_ip: [u8; 4] = [10, 0, 2, 2];

    if args_len > 0 {
        unsafe {
            let mut start = 0;
            while start < args_len && (*args_ptr.add(start) == b' ' || *args_ptr.add(start) == b'\t')
            {
                start += 1;
            }
            if start < args_len {
                let arg_str =
                    core::str::from_utf8_unchecked(core::slice::from_raw_parts(
                        args_ptr.add(start),
                        args_len - start,
                    ));
                if let Some(ip) = parse_ip(arg_str) {
                    target_ip = ip;
                } else if arg_str == "-h" || arg_str == "--help" {
                    sbPrint(b"Usage: ping [ip-address|hostname]\n");
                    sbPrint(b"  Default target: 10.0.2.2 (QEMU gateway)\n");
                    std::terminate_task(0);
                } else {
                    sbPrint(b"Resolving "); sbPrint(arg_str.as_bytes()); sbPrint(b"...\n");
                    match std::dns_resolve(arg_str) {
                        Some(ip) => target_ip = ip,
                        None => { sbPrint(b"ping: DNS lookup failed\n"); std::terminate_task(1); }
                    }
                }
            }
        }
    }

    // "PING <ip>: 56 data bytes\n"
    sbPrint(b"PING ");
    print_ip(target_ip);
    sbPrint(b": 56 data bytes\n");

    let mut sent: u32 = 0;
    let mut received: u32 = 0;
    let mut total_time_ms: u32 = 0;
    let mut min_ms: u32 = 999999;
    let mut max_ms: u32 = 0;

    let count = 10u32;
    let mut seq: u32 = 0;
    while seq < count {
        seq += 1;
        sent += 1;

        let send_time = rdtsc_ms();

        let ret = std::net_send_ping(target_ip);
        if ret == 0 {
            sbPrint(b"  send failed\n");
            std::yield_task();
            continue;
        }

        // Wait up to one second for the reply. RX processing is IRQ-driven, so
        // yielding in a tight iteration-count loop can expire before an
        // external reply (typically tens of milliseconds) reaches the guest.
        let mut found = false;
        let deadline = send_time.wrapping_add(1000);
        loop {
            std::yield_task();

            let mut reply_buf = [0u8; core::mem::size_of::<IcmpEchoReply>()];
            let n = std::net_recv_ping(&mut reply_buf);
            if n >= core::mem::size_of::<IcmpEchoReply>() {
                let reply: IcmpEchoReply =
                    unsafe { core::ptr::read(reply_buf.as_ptr() as *const IcmpEchoReply) };

                if reply.identifier == 0xBEEF && reply.sequence == ret as u16 {
                    let recv_time = rdtsc_ms();
                    let rtt = recv_time.wrapping_sub(send_time);

                    received += 1;
                    total_time_ms += rtt;
                    if rtt < min_ms {
                        min_ms = rtt;
                    }
                    if rtt > max_ms {
                        max_ms = rtt;
                    }

                    // "<seq> bytes from <ip>: icmp_seq=<seq> time=<rtt> ms\n"
                    print_u64(seq as u64);
                    sbPrint(b" bytes from ");
                    print_ip(reply.src_ip);
                    sbPrint(b": icmp_seq=");
                    print_u64(seq as u64);
                    sbPrint(b" time=");
                    print_u32(rtt);
                    sbPrint(b" ms\n");
                    found = true;
                    break;
                }
            }
            let now = rdtsc_ms();
            if now.wrapping_sub(deadline) < 0x8000_0000 {
                break;
            }
        }

        if !found {
            print_u64(seq as u64);
            sbPrint(b" bytes from timeout\n");
        }

        // Keep the traditional one-second interval without busy-yielding.
        std::sleep(1000);
    }

    newline();
    sbPrint(b"--- ");
    print_ip(target_ip);
    sbPrint(b" ping statistics ---\n");
    print_u64(sent as u64);
    sbPrint(b" packets transmitted, ");
    print_u64(received as u64);
    sbPrint(b" received, ");
    if sent > 0 {
        let loss = ((sent - received) * 100) / sent;
        print_u64(loss as u64);
        sbPrint(b"% packet loss\n");
    } else {
        sbPrint(b"0% packet loss\n");
    }

    if received > 0 {
        let avg = total_time_ms / received;
        sbPrint(b"rtt min/avg/max = ");
        print_u32(min_ms);
        put_char(b'/');
        print_u32(avg);
        put_char(b'/');
        print_u32(max_ms);
        sbPrint(b" ms\n");
    }

    std::terminate_task(0);
    loop {}
}

fn rdtsc_ms() -> u32 {
    unsafe {
        let lo: u32;
        let hi: u32;
        core::arch::asm!("rdtsc", out("eax") lo, out("edx") hi);
        let cycles = ((hi as u64) << 32) | (lo as u64);
        (cycles / 2_400_000) as u32
    }
}
