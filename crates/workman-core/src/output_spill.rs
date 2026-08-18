//! Bounded, asynchronous raw-output spill files for PTY processes.

use std::{
    collections::VecDeque,
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering};

const FLUSH_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Clone)]
pub(crate) struct OutputSpillSink {
    shared: Arc<Shared>,
}

pub(crate) struct OutputSpill {
    sink: OutputSpillSink,
    worker: Option<JoinHandle<()>>,
}

struct Shared {
    path: PathBuf,
    capacity: usize,
    state: Mutex<State>,
    wake: Condvar,
    #[cfg(test)]
    snapshot_writes: AtomicUsize,
}

struct State {
    pending: VecDeque<u8>,
    overflowed: bool,
    dirty: bool,
    clear_generation: u64,
    flush_requested: u64,
    flush_completed: u64,
    shutdown: bool,
    worker_running: bool,
    error: Option<String>,
}

impl OutputSpill {
    pub(crate) fn start(path: PathBuf, capacity: usize) -> io::Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        write_snapshot(&path, &VecDeque::new())?;

        let shared = Arc::new(Shared {
            path,
            capacity,
            state: Mutex::new(State {
                pending: VecDeque::with_capacity(capacity.min(256 * 1024)),
                overflowed: false,
                dirty: false,
                clear_generation: 0,
                flush_requested: 0,
                flush_completed: 0,
                shutdown: false,
                worker_running: true,
                error: None,
            }),
            wake: Condvar::new(),
            #[cfg(test)]
            snapshot_writes: AtomicUsize::new(0),
        });
        let worker_shared = Arc::clone(&shared);
        let worker = thread::Builder::new()
            .name("workman-output-spill".to_owned())
            .spawn(move || writer_loop(worker_shared))?;
        Ok(Self {
            sink: OutputSpillSink { shared },
            worker: Some(worker),
        })
    }

    pub(crate) fn sink(&self) -> OutputSpillSink {
        self.sink.clone()
    }

    pub(crate) fn flush(&self) -> io::Result<()> {
        self.sink.flush(false)
    }

    pub(crate) fn clear(&self) -> io::Result<()> {
        let shared = &self.sink.shared;
        let request = {
            let mut state = lock_state(shared);
            state.pending.clear();
            state.overflowed = false;
            state.dirty = true;
            state.clear_generation = state.clear_generation.wrapping_add(1);
            state.flush_requested = state.flush_requested.wrapping_add(1);
            state.flush_requested
        };
        shared.wake.notify_one();
        wait_for_flush(shared, request)
    }

    pub(crate) fn shutdown(&mut self) -> io::Result<()> {
        let flush_result = self.sink.flush(true);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        flush_result
    }

    #[cfg(test)]
    fn snapshot_writes(&self) -> usize {
        self.sink.shared.snapshot_writes.load(Ordering::Relaxed)
    }
}

impl Drop for OutputSpill {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

impl OutputSpillSink {
    pub(crate) fn push(&self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        let shared = &self.shared;
        let mut state = lock_state(shared);
        if state.shutdown || shared.capacity == 0 {
            return;
        }

        if bytes.len() >= shared.capacity {
            state.pending.clear();
            state
                .pending
                .extend(bytes[bytes.len() - shared.capacity..].iter().copied());
            state.overflowed = true;
        } else {
            state.pending.extend(bytes.iter().copied());
            let overflow = state.pending.len().saturating_sub(shared.capacity);
            if overflow > 0 {
                state.pending.drain(..overflow);
                state.overflowed = true;
            }
        }
        let wake_worker = !state.dirty;
        state.dirty = true;
        drop(state);
        if wake_worker {
            shared.wake.notify_one();
        }
    }

    fn flush(&self, shutdown: bool) -> io::Result<()> {
        let shared = &self.shared;
        let request = {
            let mut state = lock_state(shared);
            state.flush_requested = state.flush_requested.wrapping_add(1);
            if shutdown {
                state.shutdown = true;
            }
            state.flush_requested
        };
        shared.wake.notify_one();
        wait_for_flush(shared, request)
    }
}

fn writer_loop(shared: Arc<Shared>) {
    let mut retained = VecDeque::with_capacity(shared.capacity.min(1024 * 1024));
    let mut clear_generation = 0_u64;
    let mut next_periodic_flush = None;

    loop {
        let (pending, overflowed, requested, shutdown, next_clear_generation) = {
            let mut state = lock_state(&shared);
            loop {
                let flush_required = state.clear_generation != clear_generation
                    || state.flush_requested != state.flush_completed
                    || state.shutdown;
                if flush_required {
                    break;
                }

                if !state.dirty {
                    next_periodic_flush = None;
                    state = shared
                        .wake
                        .wait(state)
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    continue;
                }

                let deadline =
                    next_periodic_flush.get_or_insert_with(|| Instant::now() + FLUSH_INTERVAL);
                let wait = deadline.saturating_duration_since(Instant::now());
                if wait.is_zero() {
                    break;
                }

                state = shared
                    .wake
                    .wait_timeout(state, wait)
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .0;
            }

            let pending = state.pending.drain(..).collect::<Vec<_>>();
            let overflowed = std::mem::take(&mut state.overflowed);
            let requested = state.flush_requested;
            let shutdown = state.shutdown;
            let next_clear_generation = state.clear_generation;
            state.dirty = false;
            (
                pending,
                overflowed,
                requested,
                shutdown,
                next_clear_generation,
            )
        };

        if next_clear_generation != clear_generation || overflowed {
            retained.clear();
            clear_generation = next_clear_generation;
        }
        retained.extend(pending);
        let overflow = retained.len().saturating_sub(shared.capacity);
        if overflow > 0 {
            retained.drain(..overflow);
        }

        let result = write_snapshot(&shared.path, &retained);
        #[cfg(test)]
        shared.snapshot_writes.fetch_add(1, Ordering::Relaxed);
        next_periodic_flush = None;
        {
            let mut state = lock_state(&shared);
            state.error = result.err().map(|error| error.to_string());
            state.flush_completed = requested;
        }
        shared.wake.notify_all();

        if shutdown {
            break;
        }
    }

    let mut state = lock_state(&shared);
    state.worker_running = false;
    shared.wake.notify_all();
}

fn wait_for_flush(shared: &Shared, request: u64) -> io::Result<()> {
    let mut state = lock_state(shared);
    while state.flush_completed < request && state.worker_running {
        state = shared
            .wake
            .wait(state)
            .unwrap_or_else(|poisoned| poisoned.into_inner());
    }
    if state.flush_completed < request {
        return Err(io::Error::other(
            "output spill worker stopped before flushing",
        ));
    }
    match &state.error {
        Some(error) => Err(io::Error::other(error.clone())),
        None => Ok(()),
    }
}

fn lock_state(shared: &Shared) -> std::sync::MutexGuard<'_, State> {
    shared
        .state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn write_snapshot(path: &Path, retained: &VecDeque<u8>) -> io::Result<()> {
    let temporary = path.with_extension("raw.tmp");
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary)?;
    let (first, second) = retained.as_slices();
    file.write_all(first)?;
    file.write_all(second)?;
    file.flush()?;
    drop(file);
    fs::rename(&temporary, path)
}

#[cfg(test)]
mod tests {
    use std::{
        sync::atomic::{AtomicBool, Ordering},
        time::Instant,
    };

    use super::*;

    #[test]
    fn spill_is_bounded_flushable_and_clearable() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("1.raw");
        let mut spill = OutputSpill::start(path.clone(), 8).unwrap();
        let sink = spill.sink();
        sink.push(b"abc");
        sink.push(b"defghijk");
        spill.flush().unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"defghijk");

        spill.clear().unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"");
        sink.push(b"after");
        spill.shutdown().unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"after");
    }

    #[test]
    fn enqueue_path_stays_memory_only_and_bounded_under_flood() {
        const CAPACITY: usize = 8 * 1024 * 1024;
        let temp = tempfile::tempdir().unwrap();
        let mut spill = OutputSpill::start(temp.path().join("2.raw"), CAPACITY).unwrap();
        let sink = spill.sink();
        let chunk = vec![b'y'; 8192];
        let started = Instant::now();
        for _ in 0..8192 {
            sink.push(&chunk);
        }
        let enqueue_elapsed = started.elapsed();
        assert!(
            enqueue_elapsed < Duration::from_secs(2),
            "64 MiB of spill enqueueing took {enqueue_elapsed:?}"
        );
        spill.shutdown().unwrap();
        assert_eq!(
            fs::metadata(temp.path().join("2.raw")).unwrap().len(),
            CAPACITY as u64
        );
    }

    #[test]
    fn continuous_output_coalesces_periodic_snapshot_writes() {
        let temp = tempfile::tempdir().unwrap();
        let mut spill = OutputSpill::start(temp.path().join("3.raw"), 64 * 1024).unwrap();
        let sink = spill.sink();
        let stop = Arc::new(AtomicBool::new(false));
        let producer_stop = Arc::clone(&stop);
        let producer = thread::spawn(move || {
            let chunk = [b'x'; 8192];
            while !producer_stop.load(Ordering::Relaxed) {
                sink.push(&chunk);
                thread::yield_now();
            }
        });

        let first_write_deadline = Instant::now() + Duration::from_secs(2);
        while spill.snapshot_writes() == 0 && Instant::now() < first_write_deadline {
            thread::sleep(Duration::from_millis(10));
        }
        let first_write_count = spill.snapshot_writes();
        thread::sleep(FLUSH_INTERVAL * 3);
        stop.store(true, Ordering::Relaxed);
        producer.join().unwrap();

        let snapshot_writes = spill.snapshot_writes();
        assert!(
            first_write_count > 0,
            "spill worker did not write within 2s"
        );
        assert!(
            snapshot_writes.saturating_sub(first_write_count) <= 4,
            "continuous output caused {} additional snapshots in {:?}",
            snapshot_writes.saturating_sub(first_write_count),
            FLUSH_INTERVAL * 3
        );
        spill.shutdown().unwrap();
    }

    #[test]
    fn idle_spill_worker_parks_until_output_arrives() {
        let temp = tempfile::tempdir().unwrap();
        let mut spill = OutputSpill::start(temp.path().join("4.raw"), 64 * 1024).unwrap();

        thread::sleep(FLUSH_INTERVAL * 2);
        assert_eq!(spill.snapshot_writes(), 0);

        spill.sink().push(b"one line\n");
        let flush_deadline = Instant::now() + Duration::from_secs(2);
        while spill.snapshot_writes() == 0 && Instant::now() < flush_deadline {
            thread::sleep(Duration::from_millis(10));
        }
        let writes_after_output = spill.snapshot_writes();
        assert_eq!(writes_after_output, 1);

        thread::sleep(FLUSH_INTERVAL * 2);
        assert_eq!(spill.snapshot_writes(), writes_after_output);
        spill.shutdown().unwrap();
    }
}
