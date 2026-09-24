use crate::drivers::net::{arp_resolve, get_ip_address, get_mac_address, helper, transmit};

const DNS_SERVER: [u8; 4] = [10, 0, 2, 3];
const DNS_PORT: u16 = 53;
const SRC_PORT: u16 = 49152;
const TXID: u16 = 0x4b47;
/// Reserved scheduler wait key for the single outstanding DNS request.
pub const WAIT_KEY: usize = usize::MAX - 1;

static DNS_RESULT: crate::sync::Spinlock<Option<[u8; 4]>> = crate::sync::Spinlock::new(None);

pub fn take_result() -> Option<[u8; 4]> { DNS_RESULT.lock().take() }

pub unsafe fn send_query(name: &str) -> bool { unsafe {
    if name.is_empty() || name.len() > 253 { return false; }
    let my_ip = match get_ip_address() { Some(v) => v, None => return false };
    let my_mac = match get_mac_address() { Some(v) => v, None => return false };
    let dns_mac = match arp_resolve(DNS_SERVER) { Some(v) => v, None => return false };

    let mut q = [0u8; 256];
    q[0..2].copy_from_slice(&TXID.to_be_bytes());
    q[2..4].copy_from_slice(&0x0100u16.to_be_bytes());
    q[4..6].copy_from_slice(&1u16.to_be_bytes());
    let mut p = 12usize;
    for label in name.split('.') {
        if label.is_empty() || label.len() > 63 || p + 1 + label.len() + 5 > q.len() { return false; }
        q[p] = label.len() as u8; p += 1;
        q[p..p+label.len()].copy_from_slice(label.as_bytes()); p += label.len();
    }
    q[p] = 0; p += 1;
    q[p..p+2].copy_from_slice(&1u16.to_be_bytes()); p += 2; // A
    q[p..p+2].copy_from_slice(&1u16.to_be_bytes()); p += 2; // IN

    let udp_len = 8 + p;
    let ip_len = 20 + udp_len;
    let mut frame = [0u8; 14 + 20 + 8 + 256];
    frame[0..6].copy_from_slice(&dns_mac);
    frame[6..12].copy_from_slice(&my_mac);
    frame[12..14].copy_from_slice(&0x0800u16.to_be_bytes());
    let ip = &mut frame[14..34];
    ip[0]=0x45; ip[2..4].copy_from_slice(&(ip_len as u16).to_be_bytes()); ip[6]=0x40; ip[8]=64; ip[9]=17;
    ip[12..16].copy_from_slice(&my_ip); ip[16..20].copy_from_slice(&DNS_SERVER);
    let sum=helper::calculate_checksum(ip); ip[10..12].copy_from_slice(&sum.to_be_bytes());
    let u=34usize;
    frame[u..u+2].copy_from_slice(&SRC_PORT.to_be_bytes());
    frame[u+2..u+4].copy_from_slice(&DNS_PORT.to_be_bytes());
    frame[u+4..u+6].copy_from_slice(&(udp_len as u16).to_be_bytes());
    frame[u+8..u+8+p].copy_from_slice(&q[..p]);
    *DNS_RESULT.lock() = None;
    transmit(&frame[..14+ip_len])
}}

fn skip_name(data: &[u8], mut p: usize) -> Option<usize> {
    loop {
        let n=*data.get(p)?;
        if n & 0xc0 == 0xc0 { return if p+1 < data.len() { Some(p+2) } else { None }; }
        p+=1; if n==0 { return Some(p); }
        p=p.checked_add(n as usize)?; if p>data.len(){return None;}
    }
}

pub fn handle_udp(src_ip:[u8;4], data:&[u8]) {
    if src_ip != DNS_SERVER || data.len() < 20 { return; }
    let src=u16::from_be_bytes([data[0],data[1]]);
    let dst=u16::from_be_bytes([data[2],data[3]]);
    if src != DNS_PORT || dst != SRC_PORT { return; }
    let d=&data[8..];
    if d.len()<12 || u16::from_be_bytes([d[0],d[1]]) != TXID || (d[3]&0x0f)!=0 { return; }
    let qd=u16::from_be_bytes([d[4],d[5]]) as usize;
    let an=u16::from_be_bytes([d[6],d[7]]) as usize;
    let mut p=12usize;
    for _ in 0..qd { p=match skip_name(d,p){Some(v)=>v,None=>return}; if p+4>d.len(){return} p+=4; }
    for _ in 0..an {
        p=match skip_name(d,p){Some(v)=>v,None=>return}; if p+10>d.len(){return}
        let typ=u16::from_be_bytes([d[p],d[p+1]]); let class=u16::from_be_bytes([d[p+2],d[p+3]]);
        let len=u16::from_be_bytes([d[p+8],d[p+9]]) as usize; p+=10;
        if p+len>d.len(){return}
        if typ==1 && class==1 && len==4 {
            *DNS_RESULT.lock()=Some([d[p],d[p+1],d[p+2],d[p+3]]);
            crate::process::wake_waiters(WAIT_KEY);
            return;
        }
        p+=len;
    }
}
