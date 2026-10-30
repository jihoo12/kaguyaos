//! Synchronization primitives.
//!
//! Provides a single interrupt-safe spinlock type used throughout the kernel.

use core::cell::UnsafeCell;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::AtomicBool;
#[cfg(not(feature = "jihoo-kernel"))]
use core::sync::atomic::Ordering;

#[cfg(feature = "jihoo-kernel")]
unsafe extern "C" {
    fn jihoo_lock_acquire(lock: *mut u8) -> u64;
    fn jihoo_lock_try(lock: *mut u8) -> u64;
    fn jihoo_lock_release(lock: *mut u8, enable_interrupts: bool);
}

/// An interrupt-safe spinlock.
///
/// Disables local interrupts while held to prevent deadlocks when the same
/// CPU re-enters the lock from an interrupt handler. Suitable for use in
/// interrupt context, syscall context, and normal kernel code.
pub struct Spinlock<T> {
    lock: AtomicBool,
    data: UnsafeCell<T>,
}

unsafe impl<T: Send> Sync for Spinlock<T> {}
unsafe impl<T: Send> Send for Spinlock<T> {}

impl<T> Spinlock<T> {
    pub const fn new(data: T) -> Self {
        Self {
            lock: AtomicBool::new(false),
            data: UnsafeCell::new(data),
        }
    }

    #[cfg(feature = "jihoo-kernel")]
    pub fn lock(&self) -> SpinlockGuard<'_, T> {
        let flags = unsafe { jihoo_lock_acquire(self.lock.as_ptr().cast()) };
        SpinlockGuard { lock: self, interrupts_enabled: flags & (1 << 9) != 0 }
    }

    #[cfg(not(feature = "jihoo-kernel"))]
    pub fn lock(&self) -> SpinlockGuard<'_, T> {
        let rflags = unsafe {
            let r: u64;
            core::arch::asm!("pushfq; pop {}", out(reg) r, options(nomem, preserves_flags));
            r
        };
        unsafe {
            core::arch::asm!("cli", options(nomem, nostack, preserves_flags));
        }
        let interrupts_enabled = (rflags & (1 << 9)) != 0;

        while self
            .lock
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            core::hint::spin_loop();
        }

        SpinlockGuard {
            lock: self,
            interrupts_enabled,
        }
    }

    #[cfg(feature = "jihoo-kernel")]
    pub fn try_lock(&self) -> Option<SpinlockGuard<'_, T>> {
        let flags = unsafe { jihoo_lock_try(self.lock.as_ptr().cast()) };
        if flags == u64::MAX {
            return None;
        }
        Some(SpinlockGuard {
            lock: self,
            interrupts_enabled: flags & (1 << 9) != 0,
        })
    }

    #[cfg(not(feature = "jihoo-kernel"))]
    pub fn try_lock(&self) -> Option<SpinlockGuard<'_, T>> {
        let rflags = unsafe {
            let r: u64;
            core::arch::asm!("pushfq; pop {}", out(reg) r, options(nomem, preserves_flags));
            r
        };
        unsafe {
            core::arch::asm!("cli", options(nomem, nostack, preserves_flags));
        }
        let interrupts_enabled = (rflags & (1 << 9)) != 0;

        if self
            .lock
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
        {
            Some(SpinlockGuard {
                lock: self,
                interrupts_enabled,
            })
        } else {
            if interrupts_enabled {
                unsafe {
                    core::arch::asm!("sti", options(nomem, nostack, preserves_flags));
                }
            }
            None
        }
    }

    pub unsafe fn force_unlock(&self) {
        #[cfg(feature = "jihoo-kernel")]
        unsafe { jihoo_lock_release(self.lock.as_ptr().cast(), false); }
        #[cfg(not(feature = "jihoo-kernel"))]
        self.lock.store(false, Ordering::Release);
    }
}

pub struct SpinlockGuard<'a, T> {
    lock: &'a Spinlock<T>,
    interrupts_enabled: bool,
}

impl<T> Deref for SpinlockGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &*self.lock.data.get() }
    }
}

impl<T> DerefMut for SpinlockGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.lock.data.get() }
    }
}

impl<T> Drop for SpinlockGuard<'_, T> {
    fn drop(&mut self) {
        #[cfg(feature = "jihoo-kernel")]
        unsafe { jihoo_lock_release(self.lock.lock.as_ptr().cast(), self.interrupts_enabled); }
        #[cfg(not(feature = "jihoo-kernel"))]
        {
            self.lock.lock.store(false, Ordering::Release);
            if self.interrupts_enabled {
                unsafe {
                    core::arch::asm!("sti", options(nomem, nostack, preserves_flags));
                }
            }
        }
    }
}
