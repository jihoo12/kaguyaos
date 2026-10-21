//! TCP client connections.
//!
//! A connection's control block lives in a global table, so it outlives the
//! descriptor: closing the last descriptor queues a FIN, and the connection
//! stays until that FIN is acknowledged, the peer resets it, or retransmission
//! gives up. Segments are processed by the RX path in `net::poll`, which also
//! runs the retransmission timer. Transmission never waits for ARP; `connect`
//! resolves the next hop before the first SYN.
//!
//! Deliberate simplifications: no out-of-order reassembly (such segments are
//! dropped and answered with a duplicate ACK, so the peer retransmits),
//! go-back-N retransmission with a doubling RTO instead of RTT estimation, no
//! congestion control beyond the peer's window, no listening sockets, and no
//! TIME_WAIT (a client uses a fresh ephemeral port for each connection).

use super::{get_ip_address, helper, ipv4, Endpoint};
use crate::fs::{FsError, FsResult};
use crate::process;
use crate::sync::Spinlock;
use alloc::collections::VecDeque;
use alloc::sync::Arc;
use alloc::vec::Vec;

pub const PROTOCOL: u8 = 6;

const FIN: u8 = 0x01;
const SYN: u8 = 0x02;
const RST: u8 = 0x04;
const PSH: u8 = 0x08;
const ACK: u8 = 0x10;

const HEADER_LEN: usize = 20;
const MSS: usize = 1460;
const DEFAULT_PEER_MSS: usize = 536;
const RECV_CAP: usize = 16 * 1024;
const SEND_CAP: usize = 8 * 1024;
const EPHEMERAL_FIRST: u16 = 50000;

// Scheduler ticks (100 Hz).
const INITIAL_RTO: u64 = 100;
const MAX_RTO: u64 = 1600;
const MAX_RETRIES: u32 = 6;
const FIN_WAIT2_TIMEOUT: u64 = 3000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// Created but never connected.
    Idle,
    SynSent,
    Established,
    FinWait1,
    FinWait2,
    CloseWait,
    Closing,
    LastAck,
    Closed,
}

struct Tcb {
    state: State,
    local_port: u16,
    remote: Endpoint,
    iss: u32,
    snd_una: u32,
    snd_nxt: u32,
    snd_wnd: usize,
    mss: usize,
    /// Unacknowledged and unsent data, starting at `snd_una`.
    send_buf: VecDeque<u8>,
    /// The user closed: send a FIN after the queued data.
    fin_queued: bool,
    fin_sent: bool,
    fin_acked: bool,
    rcv_nxt: u32,
    recv_buf: VecDeque<u8>,
    peer_fin: bool,
    user_closed: bool,
    /// Receive window in the last segment sent.
    advertised: usize,
    error: Option<FsError>,
    rto: u64,
    retries: u32,
    deadline: Option<u64>,
}

pub struct Conn {
    tcb: Spinlock<Tcb>,
}

struct Entry {
    local_port: u16,
    remote: Endpoint,
    conn: Arc<Conn>,
}

/// Connections that have sent a SYN and are not yet Closed. Lock order:
/// CONNS, then a connection's TCB.
static CONNS: Spinlock<Vec<Entry>> = Spinlock::new(Vec::new());
static NEXT_EPHEMERAL: Spinlock<u16> = Spinlock::new(EPHEMERAL_FIRST);

fn seq_lt(a: u32, b: u32) -> bool {
    (a.wrapping_sub(b) as i32) < 0
}

fn seq_le(a: u32, b: u32) -> bool {
    (a.wrapping_sub(b) as i32) <= 0
}

struct Segment<'a> {
    src_port: u16,
    dst_port: u16,
    seq: u32,
    ack: u32,
    flags: u8,
    window: usize,
    mss: Option<usize>,
    payload: &'a [u8],
}

fn parse(segment: &[u8]) -> Option<Segment<'_>> {
    if segment.len() < HEADER_LEN {
        return None;
    }
    let data_offset = (segment[12] >> 4) as usize * 4;
    if data_offset < HEADER_LEN || data_offset > segment.len() {
        return None;
    }
    let mut mss = None;
    let mut options = &segment[HEADER_LEN..data_offset];
    while let [kind, rest @ ..] = options {
        match kind {
            0 => break,
            1 => options = rest,
            _ => {
                let len = *rest.first()? as usize;
                if len < 2 || len > options.len() {
                    break;
                }
                if *kind == 2 && len == 4 {
                    mss = Some(u16::from_be_bytes([options[2], options[3]]) as usize);
                }
                options = &options[len..];
            }
        }
    }
    Some(Segment {
        src_port: u16::from_be_bytes([segment[0], segment[1]]),
        dst_port: u16::from_be_bytes([segment[2], segment[3]]),
        seq: u32::from_be_bytes([segment[4], segment[5], segment[6], segment[7]]),
        ack: u32::from_be_bytes([segment[8], segment[9], segment[10], segment[11]]),
        flags: segment[13],
        window: u16::from_be_bytes([segment[14], segment[15]]) as usize,
        mss,
        payload: &segment[data_offset..],
    })
}

/// Build and send one segment without consulting a TCB.
fn transmit(local_port: u16, remote: Endpoint, seq: u32, ack: u32, flags: u8, window: usize, payload: &[u8]) -> bool {
    let Some(local_ip) = get_ip_address() else {
        return false;
    };
    let header_len = if flags & SYN != 0 { HEADER_LEN + 4 } else { HEADER_LEN };
    let len = header_len + payload.len();
    let mut segment = [0u8; HEADER_LEN + 4 + MSS];
    segment[0..2].copy_from_slice(&local_port.to_be_bytes());
    segment[2..4].copy_from_slice(&remote.port.to_be_bytes());
    segment[4..8].copy_from_slice(&seq.to_be_bytes());
    segment[8..12].copy_from_slice(&ack.to_be_bytes());
    segment[12] = ((header_len / 4) as u8) << 4;
    segment[13] = flags;
    segment[14..16].copy_from_slice(&(window.min(u16::MAX as usize) as u16).to_be_bytes());
    if flags & SYN != 0 {
        segment[20..24].copy_from_slice(&[2, 4, (MSS >> 8) as u8, MSS as u8]);
    }
    segment[header_len..len].copy_from_slice(payload);
    let checksum = helper::transport_checksum(local_ip, remote.ip, PROTOCOL, &segment[..len]);
    segment[16..18].copy_from_slice(&checksum.to_be_bytes());
    unsafe { ipv4::transmit_ipv4_cached(remote.ip, PROTOCOL, &segment[..len]) }
}

impl Tcb {
    fn new() -> Self {
        Self {
            state: State::Idle,
            local_port: 0,
            remote: Endpoint { ip: [0; 4], port: 0 },
            iss: 0,
            snd_una: 0,
            snd_nxt: 0,
            snd_wnd: 0,
            mss: DEFAULT_PEER_MSS,
            send_buf: VecDeque::new(),
            fin_queued: false,
            fin_sent: false,
            fin_acked: false,
            rcv_nxt: 0,
            recv_buf: VecDeque::new(),
            peer_fin: false,
            user_closed: false,
            advertised: 0,
            error: None,
            rto: INITIAL_RTO,
            retries: 0,
            deadline: None,
        }
    }

    fn send(&mut self, flags: u8, seq: u32, payload: &[u8]) {
        let window = RECV_CAP - self.recv_buf.len();
        self.advertised = window;
        let ack = if flags & ACK != 0 { self.rcv_nxt } else { 0 };
        transmit(self.local_port, self.remote, seq, ack, flags, window, payload);
    }

    fn send_ack(&mut self) {
        self.send(ACK, self.snd_nxt, &[]);
    }

    fn arm(&mut self, now: u64) {
        if self.deadline.is_none() {
            self.deadline = Some(now + self.rto);
        }
    }

    fn fail(&mut self, error: FsError) {
        self.error = Some(error);
        self.state = State::Closed;
        self.deadline = None;
        self.send_buf.clear();
    }

    /// Bytes of `send_buf` already transmitted.
    fn data_in_flight(&self) -> usize {
        let in_flight = self.snd_nxt.wrapping_sub(self.snd_una) as usize;
        in_flight - (self.fin_sent && !self.fin_acked) as usize
    }

    /// Send whatever the peer's window allows, then a queued FIN.
    fn output(&mut self, now: u64, probe: bool) {
        match self.state {
            State::SynSent => {
                if self.snd_nxt == self.iss {
                    self.send(SYN, self.iss, &[]);
                    self.snd_nxt = self.iss.wrapping_add(1);
                    self.arm(now);
                }
                return;
            }
            State::Established | State::CloseWait | State::FinWait1 | State::Closing | State::LastAck => {}
            _ => return,
        }

        let mut payload = [0u8; MSS];
        loop {
            let in_flight = self.data_in_flight();
            let unsent = self.send_buf.len() - in_flight;
            // A zero window is probed with one byte when the timer expires.
            let window = if probe && self.snd_wnd == 0 && in_flight == 0 { 1 } else { self.snd_wnd };
            let n = unsent.min(window.saturating_sub(in_flight)).min(self.mss);
            if n == 0 {
                break;
            }
            for (dst, src) in payload.iter_mut().zip(self.send_buf.range(in_flight..in_flight + n)) {
                *dst = *src;
            }
            let seq = self.snd_nxt;
            self.send(ACK | PSH, seq, &payload[..n]);
            self.snd_nxt = seq.wrapping_add(n as u32);
            self.arm(now);
        }

        if self.fin_queued && !self.fin_sent && self.data_in_flight() == self.send_buf.len() {
            self.send(FIN | ACK, self.snd_nxt, &[]);
            self.snd_nxt = self.snd_nxt.wrapping_add(1);
            self.fin_sent = true;
            self.state = match self.state {
                State::Established => State::FinWait1,
                State::CloseWait => State::LastAck,
                state => state,
            };
            self.arm(now);
        }
        if self.send_buf.len() > self.data_in_flight() {
            self.arm(now);
        }
    }

    fn on_segment(&mut self, seg: &Segment, now: u64) {
        match self.state {
            State::Idle | State::Closed => return,
            State::SynSent => {
                if seg.flags & ACK != 0 && seg.ack != self.iss.wrapping_add(1) {
                    if seg.flags & RST == 0 {
                        transmit(self.local_port, self.remote, seg.ack, 0, RST, 0, &[]);
                    }
                    return;
                }
                if seg.flags & RST != 0 {
                    if seg.flags & ACK != 0 {
                        self.fail(FsError::ConnectionRefused);
                    }
                    return;
                }
                if seg.flags & (SYN | ACK) == SYN | ACK {
                    self.rcv_nxt = seg.seq.wrapping_add(1);
                    self.snd_una = seg.ack;
                    self.snd_wnd = seg.window;
                    self.mss = seg.mss.unwrap_or(DEFAULT_PEER_MSS).min(MSS);
                    self.state = State::Established;
                    self.retries = 0;
                    self.rto = INITIAL_RTO;
                    self.deadline = None;
                    self.send_ack();
                    self.output(now, false);
                }
                return;
            }
            _ => {}
        }

        if seg.flags & RST != 0 {
            let window = (RECV_CAP - self.recv_buf.len()).max(1) as u32;
            if seq_le(self.rcv_nxt, seg.seq) && seq_lt(seg.seq, self.rcv_nxt.wrapping_add(window)) {
                self.fail(FsError::ConnectionReset);
            }
            return;
        }
        if seg.flags & SYN != 0 {
            // A retransmitted SYN-ACK means our ACK was lost.
            self.send_ack();
            return;
        }
        if seg.flags & ACK == 0 {
            return;
        }

        if seq_lt(self.snd_una, seg.ack) && seq_le(seg.ack, self.snd_nxt) {
            let acked = seg.ack.wrapping_sub(self.snd_una) as usize;
            let data = acked.min(self.send_buf.len());
            self.send_buf.drain(..data);
            if acked > data && self.fin_sent {
                self.fin_acked = true;
            }
            self.snd_una = seg.ack;
            self.retries = 0;
            self.rto = INITIAL_RTO;
            self.deadline = (self.snd_una != self.snd_nxt).then(|| now + self.rto);
            if self.fin_acked {
                match self.state {
                    State::FinWait1 => {
                        self.state = State::FinWait2;
                        self.deadline = Some(now + FIN_WAIT2_TIMEOUT);
                    }
                    State::Closing | State::LastAck => self.state = State::Closed,
                    _ => {}
                }
            }
        } else if seq_lt(self.snd_nxt, seg.ack) {
            self.send_ack();
            return;
        } else if seg.ack == self.snd_una && seg.window == 0 {
            // The peer is alive but full; keep probing instead of giving up.
            self.retries = 0;
        }
        self.snd_wnd = seg.window;

        let fin = seg.flags & FIN != 0;
        if !seg.payload.is_empty() || fin {
            if seg.seq == self.rcv_nxt {
                let accepts = matches!(self.state, State::Established | State::FinWait1 | State::FinWait2);
                let mut taken = 0;
                if accepts {
                    taken = if self.user_closed {
                        seg.payload.len()
                    } else {
                        seg.payload.len().min(RECV_CAP - self.recv_buf.len())
                    };
                    if !self.user_closed {
                        self.recv_buf.extend(&seg.payload[..taken]);
                    }
                    self.rcv_nxt = self.rcv_nxt.wrapping_add(taken as u32);
                }
                if fin && accepts && taken == seg.payload.len() {
                    self.rcv_nxt = self.rcv_nxt.wrapping_add(1);
                    self.peer_fin = true;
                    self.state = match self.state {
                        State::Established => State::CloseWait,
                        State::FinWait1 => State::Closing,
                        // No TIME_WAIT; see the module comment.
                        State::FinWait2 => State::Closed,
                        state => state,
                    };
                    if self.state == State::Closed {
                        self.deadline = None;
                    }
                }
            }
            // Acknowledge in-order data, and re-acknowledge anything else so
            // the peer retransmits what we dropped.
            self.send_ack();
        }
        self.output(now, false);
    }

    fn on_timer(&mut self, now: u64) {
        let Some(deadline) = self.deadline else {
            return;
        };
        if now < deadline {
            return;
        }
        self.deadline = None;
        if self.state == State::FinWait2 {
            self.state = State::Closed;
            return;
        }
        self.retries += 1;
        if self.retries > MAX_RETRIES {
            self.fail(FsError::TimedOut);
            return;
        }
        self.rto = (self.rto * 2).min(MAX_RTO);
        // Go back to the oldest unacknowledged byte (or the SYN) and resend.
        self.snd_nxt = if self.state == State::SynSent { self.iss } else { self.snd_una };
        if !self.fin_acked {
            self.fin_sent = false;
        }
        self.output(now, true);
        self.arm(now);
    }
}

impl Conn {
    fn key(&self) -> usize {
        self as *const Self as usize
    }
}

impl Drop for Conn {
    fn drop(&mut self) {
        process::forget_wait_key(self.key());
    }
}

fn remove(conn: &Arc<Conn>) {
    CONNS.lock().retain(|entry| !Arc::ptr_eq(&entry.conn, conn));
}

fn iss() -> u32 {
    let low: u32;
    unsafe {
        core::arch::asm!("rdtsc", out("eax") low, out("edx") _, options(nomem, nostack));
    }
    low
}

/// The descriptor-side handle. Dropping the last one closes the connection.
pub struct TcpSocket {
    conn: Arc<Conn>,
}

impl TcpSocket {
    pub fn new() -> Arc<Self> {
        Arc::new(Self { conn: Arc::new(Conn { tcb: Spinlock::new(Tcb::new()) }) })
    }

    /// Open a connection and block until it is established or fails.
    pub fn connect(&self, remote: Endpoint) -> FsResult<()> {
        if remote.port == 0 || remote.ip == [0; 4] {
            return Err(FsError::InvalidArgument);
        }
        // Resolve the next hop now: segment transmission never waits for ARP.
        let hop = ipv4::next_hop(remote.ip).ok_or(FsError::NetworkUnreachable)?;
        if unsafe { super::arp_resolve(hop) }.is_none() {
            return Err(FsError::NetworkUnreachable);
        }

        {
            let mut conns = CONNS.lock();
            let mut tcb = self.conn.tcb.lock();
            if tcb.state != State::Idle {
                return Err(FsError::InvalidArgument);
            }
            let mut next = NEXT_EPHEMERAL.lock();
            let span = (u16::MAX - EPHEMERAL_FIRST) as usize + 1;
            let mut port = None;
            for _ in 0..span {
                let candidate = *next;
                *next = if candidate == u16::MAX { EPHEMERAL_FIRST } else { candidate + 1 };
                if !conns.iter().any(|entry| entry.local_port == candidate) {
                    port = Some(candidate);
                    break;
                }
            }
            drop(next);
            let local_port = port.ok_or(FsError::AddressInUse)?;
            let iss = iss();
            tcb.local_port = local_port;
            tcb.remote = remote;
            tcb.iss = iss;
            tcb.snd_una = iss;
            tcb.snd_nxt = iss;
            tcb.state = State::SynSent;
            tcb.output(process::scheduler_clock_now(), false);
            conns.push(Entry { local_port, remote, conn: self.conn.clone() });
        }

        loop {
            let generation = process::event_generation(self.conn.key());
            {
                let tcb = self.conn.tcb.lock();
                match tcb.state {
                    State::SynSent => {}
                    State::Closed => return Err(tcb.error.unwrap_or(FsError::ConnectionReset)),
                    _ => return Ok(()),
                }
            }
            process::wait_current_until(self.conn.key(), 0, generation);
        }
    }

    /// Block until data arrives; returns 0 once the peer has closed.
    pub fn read(&self, buf: &mut [u8]) -> FsResult<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        loop {
            let generation = process::event_generation(self.conn.key());
            {
                let mut tcb = self.conn.tcb.lock();
                if !tcb.recv_buf.is_empty() {
                    let n = buf.len().min(tcb.recv_buf.len());
                    for (dst, src) in buf.iter_mut().zip(tcb.recv_buf.drain(..n)) {
                        *dst = src;
                    }
                    // Reopen a window that had shrunk below half.
                    if tcb.advertised < RECV_CAP / 2
                        && RECV_CAP - tcb.recv_buf.len() >= RECV_CAP / 2
                        && !matches!(tcb.state, State::Idle | State::SynSent | State::Closed)
                    {
                        tcb.send_ack();
                    }
                    return Ok(n);
                }
                if tcb.peer_fin {
                    return Ok(0);
                }
                if let Some(error) = tcb.error {
                    return Err(error);
                }
                match tcb.state {
                    State::Idle => return Err(FsError::NotConnected),
                    State::Closed => return Ok(0),
                    _ => {}
                }
            }
            process::wait_current_until(self.conn.key(), 0, generation);
        }
    }

    /// Block until all of `data` is queued for sending.
    pub fn write(&self, data: &[u8]) -> FsResult<usize> {
        let mut written = 0;
        while written < data.len() {
            let generation = process::event_generation(self.conn.key());
            {
                let mut tcb = self.conn.tcb.lock();
                let error = match (tcb.error, tcb.state) {
                    (Some(error), _) => Some(error),
                    (None, State::Idle | State::SynSent) => Some(FsError::NotConnected),
                    (None, State::Established | State::CloseWait) if !tcb.fin_queued => None,
                    _ => Some(FsError::BrokenPipe),
                };
                if let Some(error) = error {
                    return if written > 0 { Ok(written) } else { Err(error) };
                }
                let n = (SEND_CAP - tcb.send_buf.len()).min(data.len() - written);
                if n > 0 {
                    tcb.send_buf.extend(&data[written..written + n]);
                    written += n;
                    tcb.output(process::scheduler_clock_now(), false);
                    continue;
                }
            }
            process::wait_current_until(self.conn.key(), 0, generation);
        }
        Ok(written)
    }
}

impl Drop for TcpSocket {
    fn drop(&mut self) {
        let closed = {
            let mut tcb = self.conn.tcb.lock();
            tcb.user_closed = true;
            tcb.recv_buf.clear();
            match tcb.state {
                State::SynSent => tcb.state = State::Closed,
                State::Established | State::CloseWait => {
                    tcb.fin_queued = true;
                    tcb.output(process::scheduler_clock_now(), false);
                }
                _ => {}
            }
            tcb.state == State::Closed
        };
        if closed {
            remove(&self.conn);
        }
    }
}

/// Deliver a TCP segment addressed to this host.
pub fn handle_incoming(src_ip: [u8; 4], dst_ip: [u8; 4], segment: &[u8]) {
    if helper::transport_checksum(src_ip, dst_ip, PROTOCOL, segment) != 0 {
        return;
    }
    let Some(seg) = parse(segment) else {
        return;
    };
    let remote = Endpoint { ip: src_ip, port: seg.src_port };
    let conn = CONNS
        .lock()
        .iter()
        .find(|entry| entry.local_port == seg.dst_port && entry.remote == remote)
        .map(|entry| entry.conn.clone());
    let Some(conn) = conn else {
        // No connection: answer with a reset so the peer gives up.
        if seg.flags & RST == 0 {
            let fin_syn = (seg.flags & (SYN | FIN) != 0) as u32;
            if seg.flags & ACK != 0 {
                transmit(seg.dst_port, remote, seg.ack, 0, RST, 0, &[]);
            } else {
                let ack = seg.seq.wrapping_add(seg.payload.len() as u32 + fin_syn);
                transmit(seg.dst_port, remote, 0, ack, RST | ACK, 0, &[]);
            }
        }
        return;
    };
    let closed = {
        let mut tcb = conn.tcb.lock();
        tcb.on_segment(&seg, process::scheduler_clock_now());
        tcb.state == State::Closed
    };
    process::wake_waiters(conn.key());
    if closed {
        remove(&conn);
    }
}

/// Run retransmission and FIN_WAIT_2 timers. Called on every `net::poll`.
pub fn on_timer() {
    let now = process::scheduler_clock_now();
    let conns: Vec<Arc<Conn>> = CONNS.lock().iter().map(|entry| entry.conn.clone()).collect();
    for conn in conns {
        let (fired, closed) = {
            let mut tcb = conn.tcb.lock();
            let fired = tcb.deadline.is_some_and(|deadline| now >= deadline);
            if fired {
                tcb.on_timer(now);
            }
            (fired, tcb.state == State::Closed)
        };
        if fired {
            process::wake_waiters(conn.key());
        }
        if closed {
            remove(&conn);
        }
    }
}
