//! Jihoo ring transfers and end counts, with Rust ownership and scheduler waits.
//! Wakeups acquire the scheduler lock; release the pipe lock before waking,
//! and never drop an end while holding the scheduler lock.

use crate::fs::{FsError, FsResult};
use crate::process;
use crate::sync::Spinlock;
use alloc::boxed::Box;
use alloc::sync::Arc;

const CAPACITY: usize = 4096;
const WOULD_BLOCK: isize = -1;
const BROKEN_PIPE: isize = -2;

#[repr(C)]
struct RawState {
    data: *mut u8,
    head: usize,
    length: usize,
    readers: usize,
    writers: usize,
}

unsafe extern "C" {
    fn jihoo_pipe_init(state: *mut RawState, data: *mut u8);
    fn jihoo_pipe_read(state: *mut RawState, output: *mut u8, length: usize) -> isize;
    fn jihoo_pipe_write(state: *mut RawState, input: *const u8, length: usize) -> isize;
    fn jihoo_pipe_clone(state: *mut RawState, writer: bool);
    fn jihoo_pipe_close(state: *mut RawState, writer: bool) -> bool;
}

struct State {
    raw: RawState,
    _buffer: Box<[u8; CAPACITY]>,
}

// The buffer is owned here, stays at a stable address, and is only used under the lock.
unsafe impl Send for State {}

pub struct Pipe {
    state: Spinlock<State>,
}

impl Pipe {
    fn key(&self) -> usize {
        self as *const Self as usize
    }

    fn read(&self, buf: &mut [u8]) -> FsResult<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        loop {
            let generation = process::event_generation(self.key());
            let n = {
                let mut state = self.state.lock();
                unsafe { jihoo_pipe_read(&mut state.raw, buf.as_mut_ptr(), buf.len()) }
            };
            if n != WOULD_BLOCK {
                if n > 0 {
                    process::wake_waiters(self.key());
                }
                return Ok(n as usize);
            }
            process::wait_current_until(self.key(), 0, generation);
        }
    }

    fn write(&self, data: &[u8]) -> FsResult<usize> {
        let mut written = 0;
        while written < data.len() {
            let generation = process::event_generation(self.key());
            let queued = {
                let mut state = self.state.lock();
                unsafe {
                    jihoo_pipe_write(
                        &mut state.raw,
                        data[written..].as_ptr(),
                        data.len() - written,
                    )
                }
            };
            match queued {
                BROKEN_PIPE => {
                    return if written > 0 {
                        Ok(written)
                    } else {
                        Err(FsError::BrokenPipe)
                    };
                }
                WOULD_BLOCK => {
                    process::wait_current_until(self.key(), 0, generation);
                }
                _ => {
                    written += queued as usize;
                    process::wake_waiters(self.key());
                }
            }
        }
        Ok(written)
    }
}

impl Drop for Pipe {
    fn drop(&mut self) {
        process::forget_wait_key(self.key());
    }
}

pub fn new() -> (PipeReader, PipeWriter) {
    let mut buffer = unsafe { Box::<[u8; CAPACITY]>::new_zeroed().assume_init() };
    let mut raw = core::mem::MaybeUninit::uninit();
    unsafe { jihoo_pipe_init(raw.as_mut_ptr(), buffer.as_mut_ptr()) };
    let pipe = Arc::new(Pipe {
        state: Spinlock::new(State {
            raw: unsafe { raw.assume_init() },
            _buffer: buffer,
        }),
    });
    (PipeReader(pipe.clone()), PipeWriter(pipe))
}

pub struct PipeReader(Arc<Pipe>);
pub struct PipeWriter(Arc<Pipe>);

impl PipeReader {
    pub fn read(&self, buf: &mut [u8]) -> FsResult<usize> {
        self.0.read(buf)
    }
}

impl PipeWriter {
    pub fn write(&self, data: &[u8]) -> FsResult<usize> {
        self.0.write(data)
    }
}

impl Clone for PipeReader {
    fn clone(&self) -> Self {
        unsafe { jihoo_pipe_clone(&mut self.0.state.lock().raw, false) };
        Self(self.0.clone())
    }
}

impl Clone for PipeWriter {
    fn clone(&self) -> Self {
        unsafe { jihoo_pipe_clone(&mut self.0.state.lock().raw, true) };
        Self(self.0.clone())
    }
}

impl Drop for PipeReader {
    fn drop(&mut self) {
        let last = unsafe { jihoo_pipe_close(&mut self.0.state.lock().raw, false) };
        if last {
            process::wake_waiters(self.0.key());
        }
    }
}

impl Drop for PipeWriter {
    fn drop(&mut self) {
        let last = unsafe { jihoo_pipe_close(&mut self.0.state.lock().raw, true) };
        if last {
            process::wake_waiters(self.0.key());
        }
    }
}
