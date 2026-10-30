extern crate alloc;

use std::thread;
use std::time::{Duration, Instant};

mod fs {
    #[derive(Debug, PartialEq, Eq)]
    pub enum FsError {
        BrokenPipe,
    }
    pub type FsResult<T> = Result<T, FsError>;
}

mod sync {
    pub struct Spinlock<T>(std::sync::Mutex<T>);
    impl<T> Spinlock<T> {
        pub fn new(value: T) -> Self {
            Self(std::sync::Mutex::new(value))
        }
        pub fn lock(&self) -> std::sync::MutexGuard<'_, T> {
            self.0.lock().unwrap()
        }
    }
}

mod process {
    use std::collections::HashMap;
    use std::sync::{Condvar, Mutex, OnceLock};
    use std::time::Duration;

    #[derive(Default)]
    struct State {
        generations: HashMap<usize, u64>,
        waiters: HashMap<usize, usize>,
    }
    static EVENTS: OnceLock<(Mutex<State>, Condvar)> = OnceLock::new();
    static ON_WAIT: Mutex<Option<Box<dyn FnOnce() + Send>>> = Mutex::new(None);
    fn events() -> &'static (Mutex<State>, Condvar) {
        EVENTS.get_or_init(|| (Mutex::new(State::default()), Condvar::new()))
    }
    pub fn event_generation(key: usize) -> u64 {
        *events()
            .0
            .lock()
            .unwrap()
            .generations
            .get(&key)
            .unwrap_or(&0)
    }
    pub fn wake_waiters(key: usize) {
        let (state, ready) = events();
        *state.lock().unwrap().generations.entry(key).or_default() += 1;
        ready.notify_all();
    }
    pub fn wait_current_until(key: usize, deadline: u64, generation: u64) -> bool {
        assert_eq!(deadline, 0);
        let hook = ON_WAIT.lock().unwrap().take();
        if let Some(hook) = hook {
            hook();
        }
        let (state, ready) = events();
        let mut guard = state.lock().unwrap();
        if *guard.generations.get(&key).unwrap_or(&0) != generation {
            return false;
        }
        *guard.waiters.entry(key).or_default() += 1;
        let (mut guard, result) = ready
            .wait_timeout_while(guard, Duration::from_secs(5), |s| {
                *s.generations.get(&key).unwrap_or(&0) == generation
            })
            .unwrap();
        *guard.waiters.get_mut(&key).unwrap() -= 1;
        assert!(!result.timed_out(), "blocked pipe did not wake");
        true
    }
    pub fn forget_wait_key(key: usize) {
        let mut state = events().0.lock().unwrap();
        assert_eq!(*state.waiters.get(&key).unwrap_or(&0), 0);
        state.waiters.remove(&key);
        state.generations.remove(&key);
    }
    pub fn waiters() -> usize {
        events().0.lock().unwrap().waiters.values().sum()
    }
    pub fn key_count() -> usize {
        events().0.lock().unwrap().generations.len()
    }
    pub fn on_wait(hook: Box<dyn FnOnce() + Send>) {
        assert!(ON_WAIT.lock().unwrap().replace(hook).is_none());
    }
}

#[path = "../src/jihoo/pipe.rs"]
mod port;
#[path = "../src/pipe.rs"]
mod reference;

trait Reader: Clone + Send + Sync + 'static {
    fn read(&self, buffer: &mut [u8]) -> fs::FsResult<usize>;
}
trait Writer: Clone + Send + Sync + 'static {
    fn write(&self, data: &[u8]) -> fs::FsResult<usize>;
}
macro_rules! ends {
    ($module:ident) => {
        impl Reader for $module::PipeReader {
            fn read(&self, buffer: &mut [u8]) -> fs::FsResult<usize> {
                self.read(buffer)
            }
        }
        impl Writer for $module::PipeWriter {
            fn write(&self, data: &[u8]) -> fs::FsResult<usize> {
                self.write(data)
            }
        }
    };
}
ends!(reference);
ends!(port);

fn wait_for_block() {
    let deadline = Instant::now() + Duration::from_secs(3);
    while process::waiters() == 0 {
        assert!(Instant::now() < deadline, "operation did not block");
        thread::yield_now();
    }
}

fn contract<R: Reader, W: Writer>(new: fn() -> (R, W)) {
    {
        let (reader, writer) = new();
        assert_eq!(reader.read(&mut []), Ok(0));
        assert_eq!(writer.write(&[]), Ok(0));
        assert_eq!(writer.write(b"abc"), Ok(3));
        let mut data = [0xa5; 2];
        assert_eq!(reader.read(&mut data), Ok(2));
        assert_eq!(&data, b"ab");
        drop(writer);
        assert_eq!(reader.read(&mut data), Ok(1));
        assert_eq!(data, [b'c', b'b']);
        assert_eq!(reader.read(&mut data), Ok(0));
    }
    {
        let (reader, writer) = new();
        let writer2 = writer.clone();
        drop(writer);
        let task = thread::spawn(move || {
            let mut data = [0u8; 8];
            let n = reader.read(&mut data).unwrap();
            (reader, data, n)
        });
        wait_for_block();
        assert_eq!(writer2.write(b"wake"), Ok(4));
        let (reader, data, n) = task.join().unwrap();
        assert_eq!(&data[..n], b"wake");
        drop(writer2);
        assert_eq!(reader.read(&mut [0; 1]), Ok(0));
    }
    {
        let (reader, writer) = new();
        let reader2 = reader.clone();
        drop(reader);
        assert_eq!(writer.write(b"still open"), Ok(10));
        drop(reader2);
        assert_eq!(writer.write(b"closed"), Err(fs::FsError::BrokenPipe));
        assert_eq!(writer.write(&[]), Ok(0));
    }
    for already_queued in [false, true] {
        let (reader, writer) = new();
        if already_queued {
            assert_eq!(writer.write(&[0; 4096]), Ok(4096));
        }
        let task = thread::spawn(move || writer.write(&vec![0x5a; 5000]));
        wait_for_block();
        drop(reader);
        assert_eq!(
            task.join().unwrap(),
            if already_queued {
                Err(fs::FsError::BrokenPipe)
            } else {
                Ok(4096)
            }
        );
    }
    {
        let (reader, writer) = new();
        let expected: Vec<u8> = (0..50000).map(|i| (i * 31 + i / 127) as u8).collect();
        let input = expected.clone();
        let task = thread::spawn(move || writer.write(&input));
        let mut data = [0xa5; 777];
        let mut actual = Vec::new();
        loop {
            let n = reader.read(&mut data).unwrap();
            if n == 0 {
                break;
            }
            actual.extend_from_slice(&data[..n]);
        }
        assert_eq!(task.join().unwrap(), Ok(expected.len()));
        assert_eq!(actual, expected);
    }
    {
        let (reader, writer) = new();
        let writer2 = writer.clone();
        let a = thread::spawn(move || writer.write(&vec![b'a'; 10000]));
        let b = thread::spawn(move || writer2.write(&vec![b'b'; 7000]));
        let mut counts = [0; 256];
        let mut data = [0; 321];
        loop {
            let n = reader.read(&mut data).unwrap();
            if n == 0 {
                break;
            }
            for &value in &data[..n] {
                counts[value as usize] += 1;
            }
        }
        assert_eq!(a.join().unwrap(), Ok(10000));
        assert_eq!(b.join().unwrap(), Ok(7000));
        assert_eq!(counts[b'a' as usize], 10000);
        assert_eq!(counts[b'b' as usize], 7000);
        assert_eq!(counts.iter().sum::<usize>(), 17000);
    }
    {
        let (reader, writer) = new();
        process::on_wait(Box::new(move || drop(writer)));
        assert_eq!(
            reader.read(&mut [0; 1]),
            Ok(0),
            "close between check and sleep must not lose the wakeup"
        );
    }
    assert_eq!(process::key_count(), 0, "last end must retire the wait key");
}

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
}

fn ring_boundaries() {
    use std::collections::VecDeque;
    let mut backing = [0xa5; 4098];
    let mut state = std::mem::MaybeUninit::uninit();
    unsafe { jihoo_pipe_init(state.as_mut_ptr(), backing.as_mut_ptr().add(1)) };
    let mut state = unsafe { state.assume_init() };
    let mut model = VecDeque::new();
    let mut seed = 0x123456789abcdefu64;
    for _ in 0..20000 {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let length = (seed as usize >> 8) % 5000;
        if seed & 1 == 0 {
            let data: Vec<u8> = (0..length).map(|i| (seed as usize + i) as u8).collect();
            let count = length.min(4096 - model.len());
            let expected = if length != 0 && count == 0 {
                -1
            } else {
                count as isize
            };
            assert_eq!(
                unsafe { jihoo_pipe_write(&mut state, data.as_ptr(), length) },
                expected
            );
            model.extend(data[..count].iter().copied());
        } else {
            let mut output = vec![0xa5; length + 2];
            let count = length.min(model.len());
            let expected = if length != 0 && count == 0 {
                -1
            } else {
                count as isize
            };
            assert_eq!(
                unsafe { jihoo_pipe_read(&mut state, output.as_mut_ptr().add(1), length) },
                expected
            );
            let drained: Vec<u8> = model.drain(..count).collect();
            assert_eq!(&output[1..1 + count], drained);
            assert!(output[1 + count..].iter().all(|&b| b == 0xa5));
            assert_eq!(output[0], 0xa5);
        }
        assert_eq!(state.length, model.len());
        assert!(state.head < 4096);
        assert_eq!((backing[0], backing[4097]), (0xa5, 0xa5));
    }
}

fn main() {
    contract(reference::new);
    contract(port::new);
    ring_boundaries();
    println!(
        "jihoo pipe: wrap/full/empty, clones, EOF, BrokenPipe, partial writes, blocking, lost wakeups and 20,000 model operations passed; Rust reference passed the same contract"
    );
}
