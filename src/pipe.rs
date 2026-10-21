//! Anonymous pipes: a bounded byte ring shared by read and write ends.
//!
//! Readers block while the pipe is empty and a writer remains; writers block
//! while it is full and a reader remains. Both sides wait on the pipe's
//! address as the event key, and snapshot its generation before checking the
//! ring so a wakeup between the check and the sleep is not lost. Wakeups take
//! SCHEDULER_LOCK, so ends must never be dropped while it is held.

use crate::fs::{FsError, FsResult};
use crate::process;
use crate::sync::Spinlock;
use alloc::collections::VecDeque;
use alloc::sync::Arc;

const CAPACITY: usize = 4096;

struct State {
    buf: VecDeque<u8>,
    readers: usize,
    writers: usize,
}

pub struct Pipe {
    state: Spinlock<State>,
}

impl Pipe {
    fn key(&self) -> usize {
        self as *const Self as usize
    }

    /// Blocks until data arrives; returns 0 once every writer has closed.
    fn read(&self, buf: &mut [u8]) -> FsResult<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        loop {
            let generation = process::event_generation(self.key());
            {
                let mut state = self.state.lock();
                if !state.buf.is_empty() {
                    let n = buf.len().min(state.buf.len());
                    for (dst, src) in buf.iter_mut().zip(state.buf.drain(..n)) {
                        *dst = src;
                    }
                    drop(state);
                    process::wake_waiters(self.key());
                    return Ok(n);
                }
                if state.writers == 0 {
                    return Ok(0);
                }
            }
            process::wait_current_until(self.key(), 0, generation);
        }
    }

    /// Blocks until all of `data` is queued. Fails with BrokenPipe once every
    /// reader has closed, unless some bytes were already queued.
    fn write(&self, data: &[u8]) -> FsResult<usize> {
        let mut written = 0;
        while written < data.len() {
            let generation = process::event_generation(self.key());
            let queued = {
                let mut state = self.state.lock();
                if state.readers == 0 {
                    return if written > 0 { Ok(written) } else { Err(FsError::BrokenPipe) };
                }
                let n = (CAPACITY - state.buf.len()).min(data.len() - written);
                state.buf.extend(&data[written..written + n]);
                written += n;
                n
            };
            if queued > 0 {
                process::wake_waiters(self.key());
            } else {
                process::wait_current_until(self.key(), 0, generation);
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
    let pipe = Arc::new(Pipe {
        state: Spinlock::new(State {
            buf: VecDeque::with_capacity(CAPACITY),
            readers: 1,
            writers: 1,
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
        self.0.state.lock().readers += 1;
        Self(self.0.clone())
    }
}

impl Clone for PipeWriter {
    fn clone(&self) -> Self {
        self.0.state.lock().writers += 1;
        Self(self.0.clone())
    }
}

impl Drop for PipeReader {
    fn drop(&mut self) {
        let last = {
            let mut state = self.0.state.lock();
            state.readers -= 1;
            state.readers == 0
        };
        if last {
            process::wake_waiters(self.0.key());
        }
    }
}

impl Drop for PipeWriter {
    fn drop(&mut self) {
        let last = {
            let mut state = self.0.state.lock();
            state.writers -= 1;
            state.writers == 0
        };
        if last {
            process::wake_waiters(self.0.key());
        }
    }
}
