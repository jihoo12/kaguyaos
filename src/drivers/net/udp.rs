//! UDP sockets.
//!
//! Each socket owns a local port and a bounded queue of received datagrams.
//! The RX path runs in scheduler context (`net::poll`), finds the socket bound
//! to the destination port through a table of weak references, and wakes
//! readers waiting on the socket's address. Receiving blocks, optionally with
//! a timeout; sending may block briefly while ARP resolves the next hop.

use super::{dns, get_ip_address, helper, ipv4};
pub use super::Endpoint;
use crate::fs::{FsError, FsResult};
use crate::process;
use crate::sync::Spinlock;
use alloc::collections::VecDeque;
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;

pub const PROTOCOL: u8 = 17;
const HEADER_LEN: usize = 8;
pub const MAX_PAYLOAD: usize = 1500 - 20 - HEADER_LEN;
const QUEUE_DATAGRAMS: usize = 16;
/// Ephemeral ports start above the kernel DNS client's fixed source port.
const EPHEMERAL_FIRST: u16 = 50000;

struct Datagram {
    from: Endpoint,
    data: Vec<u8>,
}

struct State {
    local_port: u16,
    peer: Option<Endpoint>,
    queue: VecDeque<Datagram>,
}

pub struct UdpSocket {
    state: Spinlock<State>,
}

/// Bound sockets by local port. Lock order: PORTS, then a socket's state.
static PORTS: Spinlock<Vec<(u16, Weak<UdpSocket>)>> = Spinlock::new(Vec::new());
static NEXT_EPHEMERAL: Spinlock<u16> = Spinlock::new(EPHEMERAL_FIRST);

impl UdpSocket {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Spinlock::new(State {
                local_port: 0,
                peer: None,
                queue: VecDeque::new(),
            }),
        })
    }

    fn key(&self) -> usize {
        self as *const Self as usize
    }

    /// Bind to `port`, or to a free ephemeral port when `port` is 0.
    pub fn bind(self: &Arc<Self>, port: u16) -> FsResult<u16> {
        self.bind_port(port, false)
    }

    /// With `implicit`, a socket that is already bound (perhaps by another
    /// thread racing on the same socket) keeps its port instead of failing.
    fn bind_port(self: &Arc<Self>, port: u16, implicit: bool) -> FsResult<u16> {
        let mut ports = PORTS.lock();
        ports.retain(|(_, socket)| socket.strong_count() > 0);
        let mut state = self.state.lock();
        if state.local_port != 0 {
            return if implicit { Ok(state.local_port) } else { Err(FsError::InvalidArgument) };
        }
        let in_use = |port: u16| port == dns::SRC_PORT || ports.iter().any(|(bound, _)| *bound == port);
        let port = if port != 0 {
            if in_use(port) {
                return Err(FsError::AddressInUse);
            }
            port
        } else {
            let mut next = NEXT_EPHEMERAL.lock();
            let span = (u16::MAX - EPHEMERAL_FIRST) as usize + 1;
            let mut chosen = None;
            for _ in 0..span {
                let candidate = *next;
                *next = if candidate == u16::MAX { EPHEMERAL_FIRST } else { candidate + 1 };
                if !in_use(candidate) {
                    chosen = Some(candidate);
                    break;
                }
            }
            chosen.ok_or(FsError::AddressInUse)?
        };
        ports.push((port, Arc::downgrade(self)));
        state.local_port = port;
        Ok(port)
    }

    fn ensure_bound(self: &Arc<Self>) -> FsResult<u16> {
        let port = self.state.lock().local_port;
        if port != 0 { Ok(port) } else { self.bind_port(0, true) }
    }

    /// Set the default destination and accept datagrams only from it.
    pub fn connect(self: &Arc<Self>, peer: Endpoint) -> FsResult<()> {
        if peer.port == 0 {
            return Err(FsError::InvalidArgument);
        }
        self.ensure_bound()?;
        let mut state = self.state.lock();
        state.peer = Some(peer);
        state.queue.retain(|datagram| datagram.from == peer);
        Ok(())
    }

    /// Send to `dest`, or to the connected peer when `dest` is None.
    pub fn send_to(self: &Arc<Self>, dest: Option<Endpoint>, data: &[u8]) -> FsResult<usize> {
        if data.len() > MAX_PAYLOAD {
            return Err(FsError::InvalidArgument);
        }
        let dest = match dest {
            Some(dest) => dest,
            None => self.state.lock().peer.ok_or(FsError::NotConnected)?,
        };
        if dest.port == 0 {
            return Err(FsError::InvalidArgument);
        }
        let local_port = self.ensure_bound()?;
        let src_ip = get_ip_address().ok_or(FsError::NetworkUnreachable)?;

        let len = HEADER_LEN + data.len();
        let mut segment = [0u8; HEADER_LEN + MAX_PAYLOAD];
        segment[0..2].copy_from_slice(&local_port.to_be_bytes());
        segment[2..4].copy_from_slice(&dest.port.to_be_bytes());
        segment[4..6].copy_from_slice(&(len as u16).to_be_bytes());
        segment[HEADER_LEN..len].copy_from_slice(data);
        let checksum = match helper::transport_checksum(src_ip, dest.ip, PROTOCOL, &segment[..len]) {
            0 => 0xFFFF,
            sum => sum,
        };
        segment[6..8].copy_from_slice(&checksum.to_be_bytes());

        if unsafe { ipv4::transmit_ipv4(dest.ip, PROTOCOL, &segment[..len]) } {
            Ok(data.len())
        } else {
            Err(FsError::NetworkUnreachable)
        }
    }

    /// Receive one datagram, truncated to `buf`. `timeout_ticks` of None waits
    /// forever.
    pub fn recv_from(&self, buf: &mut [u8], timeout_ticks: Option<u64>) -> FsResult<(usize, Endpoint)> {
        let deadline = timeout_ticks.map(|ticks| process::scheduler_clock_now().saturating_add(ticks.max(1)));
        loop {
            let generation = process::event_generation(self.key());
            {
                let mut state = self.state.lock();
                if state.local_port == 0 {
                    return Err(FsError::NotConnected);
                }
                if let Some(datagram) = state.queue.pop_front() {
                    let n = datagram.data.len().min(buf.len());
                    buf[..n].copy_from_slice(&datagram.data[..n]);
                    return Ok((n, datagram.from));
                }
            }
            match deadline {
                Some(deadline) if process::scheduler_clock_now() >= deadline => {
                    return Err(FsError::TimedOut);
                }
                Some(deadline) => process::wait_current_until(self.key(), deadline, generation),
                None => process::wait_current_until(self.key(), 0, generation),
            };
        }
    }

    pub fn local_port(&self) -> u16 {
        self.state.lock().local_port
    }
}

impl Drop for UdpSocket {
    fn drop(&mut self) {
        let me = self as *const Self;
        PORTS.lock().retain(|(_, socket)| socket.as_ptr() != me);
        process::forget_wait_key(self.key());
    }
}

/// Deliver a UDP segment addressed to this host.
pub fn handle_incoming(src_ip: [u8; 4], dst_ip: [u8; 4], segment: &[u8]) {
    if segment.len() < HEADER_LEN {
        return;
    }
    let src_port = u16::from_be_bytes([segment[0], segment[1]]);
    let dst_port = u16::from_be_bytes([segment[2], segment[3]]);
    let len = u16::from_be_bytes([segment[4], segment[5]]) as usize;
    if len < HEADER_LEN || len > segment.len() {
        return;
    }
    let segment = &segment[..len];
    if segment[6..8] != [0, 0] && helper::transport_checksum(src_ip, dst_ip, PROTOCOL, segment) != 0 {
        return;
    }
    if dst_port == dns::SRC_PORT {
        dns::handle_udp(src_ip, segment);
        return;
    }

    let socket = PORTS
        .lock()
        .iter()
        .find(|(port, _)| *port == dst_port)
        .and_then(|(_, socket)| socket.upgrade());
    let Some(socket) = socket else {
        return;
    };
    let from = Endpoint { ip: src_ip, port: src_port };
    let queued = {
        let mut state = socket.state.lock();
        if state.peer.is_some_and(|peer| peer != from) || state.queue.len() >= QUEUE_DATAGRAMS {
            false
        } else {
            state.queue.push_back(Datagram { from, data: segment[HEADER_LEN..].to_vec() });
            true
        }
    };
    if queued {
        process::wake_waiters(socket.key());
    }
}
