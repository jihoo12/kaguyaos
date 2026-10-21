//! Console keyboard input on top of the polled xHCI driver.
//!
//! xHCI runs without interrupts, so a task blocked on keyboard input relies on
//! someone else polling the controller: the BSP's PIT handler while any reader
//! is waiting, or the hardware shell's idle loop, which has no timer.

use crate::drivers::xhci;
use crate::process;
use core::sync::atomic::{AtomicUsize, Ordering};

/// Serializes access to the xHCI event ring and the key buffer.
pub static XHCI_LOCK: crate::sync::Spinlock<()> = crate::sync::Spinlock::new(());

const WAIT_KEY: usize = usize::MAX - 2;
static WAITERS: AtomicUsize = AtomicUsize::new(0);

/// Copy buffered keys into `buf` without blocking.
pub fn read_available(buf: &mut [u8]) -> usize {
    let _guard = XHCI_LOCK.lock();
    unsafe {
        xhci::process_events();
    }
    let mut n = 0;
    while n < buf.len() {
        match xhci::get_key() {
            Some(key) => {
                buf[n] = key;
                n += 1;
            }
            None => break,
        }
    }
    n
}

/// Block until at least one key is available.
pub fn read_blocking(buf: &mut [u8]) -> usize {
    if buf.is_empty() {
        return 0;
    }
    loop {
        let generation = process::event_generation(WAIT_KEY);
        let n = read_available(buf);
        if n > 0 {
            return n;
        }
        // A key arriving before WAITERS is raised stays in the controller
        // until the next poll, which then bumps the generation and wakes us.
        WAITERS.fetch_add(1, Ordering::SeqCst);
        process::wait_current_until(WAIT_KEY, 0, generation);
        WAITERS.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Poll the controller on behalf of blocked readers. Runs with interrupts
/// disabled, so it gives up if another CPU is using the controller.
pub fn poll() {
    if WAITERS.load(Ordering::SeqCst) == 0 {
        return;
    }
    let Some(guard) = XHCI_LOCK.try_lock() else {
        return;
    };
    unsafe {
        xhci::process_events();
    }
    let ready = xhci::has_key();
    drop(guard);
    if ready {
        process::wake_waiters(WAIT_KEY);
    }
}
