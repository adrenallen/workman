//! Unix PTY process hosting and bounded raw-output capture.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use nix::errno::Errno;
use nix::sys::signal::{Signal, killpg};
use nix::unistd::Pid;
use portable_pty::{Child, CommandBuilder, MasterPty, native_pty_system};

use crate::terminal::{DEFAULT_SCROLLBACK_LINES, TerminalOutput};

/// Portable PTY exit status and terminal dimensions used by the host API.
pub use portable_pty::{ExitStatus, PtySize};

/// Environment variable that identifies the gbuild process to its child.
pub const GBUILD_PROCESS_ID_ENV: &str = "GBUILD_PROCESS_ID";

/// Environment variable carrying the per-process MCP bearer token.
pub const GBUILD_MCP_TOKEN_ENV: &str = "GBUILD_MCP_TOKEN";

/// Default amount of raw PTY output retained for a process.
pub const DEFAULT_RAW_BUFFER_CAPACITY: usize = 4 * 1024 * 1024;

/// Default PTY dimensions used when the caller has not observed a terminal yet.
pub const DEFAULT_PTY_SIZE: PtySize = PtySize {
    rows: 24,
    cols: 80,
    pixel_width: 0,
    pixel_height: 0,
};

/// A fixed-capacity byte ring that always retains the newest bytes.
///
/// Storage is allocated once at construction, so a noisy process cannot grow
/// daemon memory beyond the configured capacity.
#[derive(Clone, Debug)]
pub struct RawRingBuffer {
    storage: Box<[u8]>,
    start: usize,
    len: usize,
    total_bytes_seen: u64,
}

/// A literal byte match within the retained raw stream.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawSearchMatch {
    /// Offset within the currently retained ring snapshot.
    pub retained_offset: usize,
    /// Absolute byte offset since this process's capture started.
    pub stream_offset: u64,
}

impl RawRingBuffer {
    /// Allocate a ring retaining at most `capacity` bytes.
    pub fn new(capacity: usize) -> Self {
        Self {
            storage: vec![0; capacity].into_boxed_slice(),
            start: 0,
            len: 0,
            total_bytes_seen: 0,
        }
    }

    /// Append bytes, discarding the oldest content when the ring is full.
    pub fn push(&mut self, bytes: &[u8]) {
        self.total_bytes_seen = self
            .total_bytes_seen
            .saturating_add(u64::try_from(bytes.len()).unwrap_or(u64::MAX));

        let capacity = self.capacity();
        if capacity == 0 || bytes.is_empty() {
            return;
        }

        if bytes.len() >= capacity {
            self.storage
                .copy_from_slice(&bytes[bytes.len() - capacity..]);
            self.start = 0;
            self.len = capacity;
            return;
        }

        let overflow = self
            .len
            .saturating_add(bytes.len())
            .saturating_sub(capacity);
        if overflow > 0 {
            self.start = (self.start + overflow) % capacity;
            self.len -= overflow;
        }

        let write_at = (self.start + self.len) % capacity;
        let first_len = bytes.len().min(capacity - write_at);
        self.storage[write_at..write_at + first_len].copy_from_slice(&bytes[..first_len]);
        self.storage[..bytes.len() - first_len].copy_from_slice(&bytes[first_len..]);
        self.len += bytes.len();
    }

    /// Return retained bytes in their original order, oldest first.
    pub fn snapshot(&self) -> Vec<u8> {
        if self.len == 0 {
            return Vec::new();
        }

        let first_len = self.len.min(self.capacity() - self.start);
        let mut bytes = Vec::with_capacity(self.len);
        bytes.extend_from_slice(&self.storage[self.start..self.start + first_len]);
        bytes.extend_from_slice(&self.storage[..self.len - first_len]);
        bytes
    }

    /// Discard all retained bytes without reallocating the ring.
    pub fn clear(&mut self) {
        self.start = 0;
        self.len = 0;
    }

    /// Configured maximum number of retained bytes.
    pub fn capacity(&self) -> usize {
        self.storage.len()
    }

    /// Number of bytes currently retained.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether no output is currently retained.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Total bytes appended over the lifetime of the ring.
    pub fn total_bytes_seen(&self) -> u64 {
        self.total_bytes_seen
    }

    /// Search retained raw bytes for a literal byte sequence.
    pub fn search(&self, needle: &[u8], max_matches: usize) -> Vec<RawSearchMatch> {
        if needle.is_empty() || max_matches == 0 || needle.len() > self.len {
            return Vec::new();
        }

        let snapshot = self.snapshot();
        let stream_start = self
            .total_bytes_seen
            .saturating_sub(u64::try_from(self.len).unwrap_or(u64::MAX));
        snapshot
            .windows(needle.len())
            .enumerate()
            .filter_map(|(offset, window)| {
                (window == needle).then_some(RawSearchMatch {
                    retained_offset: offset,
                    stream_offset: stream_start.saturating_add(offset as u64),
                })
            })
            .take(max_matches)
            .collect()
    }
}

/// Thread-safe view of a process's raw output ring.
#[derive(Clone, Debug)]
pub struct RawOutput {
    inner: Arc<Mutex<RawRingBuffer>>,
}

impl RawOutput {
    fn new(capacity: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(RawRingBuffer::new(capacity))),
        }
    }

    fn lock(&self) -> MutexGuard<'_, RawRingBuffer> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn push(&self, bytes: &[u8]) {
        self.lock().push(bytes);
    }

    /// Copy the retained output, oldest byte first.
    pub fn snapshot(&self) -> Vec<u8> {
        self.lock().snapshot()
    }

    /// Discard retained output while keeping the allocation for reuse.
    pub fn clear(&self) {
        self.lock().clear();
    }

    /// Configured maximum number of retained bytes.
    pub fn capacity(&self) -> usize {
        self.lock().capacity()
    }

    /// Number of bytes currently retained.
    pub fn len(&self) -> usize {
        self.lock().len()
    }

    /// Whether no output is currently retained.
    pub fn is_empty(&self) -> bool {
        self.lock().is_empty()
    }

    /// Total bytes observed, including bytes evicted from the ring.
    pub fn total_bytes_seen(&self) -> u64 {
        self.lock().total_bytes_seen()
    }

    /// Search retained raw bytes for a literal byte sequence.
    pub fn search(&self, needle: &[u8], max_matches: usize) -> Vec<RawSearchMatch> {
        self.lock().search(needle, max_matches)
    }
}

/// Configuration for a command hosted inside a PTY.
pub struct PtySpawnOptions {
    /// Stable gbuild database ID injected into the child environment.
    pub process_id: i64,
    /// Command passed as a single argument to `/bin/sh -c`.
    pub command: String,
    /// Optional command working directory.
    pub working_dir: Option<PathBuf>,
    /// Additional environment variables. Reserved gbuild variables win.
    pub env: BTreeMap<OsString, OsString>,
    /// Initial terminal dimensions.
    pub size: PtySize,
    /// Maximum number of raw output bytes retained in memory.
    pub raw_buffer_capacity: usize,
    /// Maximum number of rendered rows retained above the viewport.
    pub scrollback_lines: usize,
    mcp_token: String,
}

impl PtySpawnOptions {
    /// Create options with a 24x80 terminal and a 4 MiB output ring.
    pub fn new(process_id: i64, mcp_token: impl Into<String>, command: impl Into<String>) -> Self {
        Self {
            process_id,
            command: command.into(),
            working_dir: None,
            env: BTreeMap::new(),
            size: DEFAULT_PTY_SIZE,
            raw_buffer_capacity: DEFAULT_RAW_BUFFER_CAPACITY,
            scrollback_lines: DEFAULT_SCROLLBACK_LINES,
            mcp_token: mcp_token.into(),
        }
    }

    /// Set the child working directory.
    pub fn with_working_dir(mut self, working_dir: impl Into<PathBuf>) -> Self {
        self.working_dir = Some(working_dir.into());
        self
    }

    /// Add or replace an environment variable.
    pub fn with_env(mut self, key: impl AsRef<OsStr>, value: impl AsRef<OsStr>) -> Self {
        self.env
            .insert(key.as_ref().to_owned(), value.as_ref().to_owned());
        self
    }

    /// Set the initial PTY dimensions.
    pub fn with_size(mut self, size: PtySize) -> Self {
        self.size = size;
        self
    }

    /// Set the retained raw-output capacity.
    pub fn with_raw_buffer_capacity(mut self, capacity: usize) -> Self {
        self.raw_buffer_capacity = capacity;
        self
    }

    /// Set the rendered scrollback row limit.
    pub fn with_scrollback_lines(mut self, lines: usize) -> Self {
        self.scrollback_lines = lines;
        self
    }
}

impl fmt::Debug for PtySpawnOptions {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PtySpawnOptions")
            .field("process_id", &self.process_id)
            .field("command", &self.command)
            .field("working_dir", &self.working_dir)
            .field("env_keys", &self.env.keys().collect::<Vec<_>>())
            .field("size", &self.size)
            .field("raw_buffer_capacity", &self.raw_buffer_capacity)
            .field("scrollback_lines", &self.scrollback_lines)
            .field("mcp_token", &"[redacted]")
            .finish()
    }
}

/// A running command attached to a native PTY.
pub struct PtyProcess {
    gbuild_process_id: i64,
    pid: u32,
    master: Option<Box<dyn MasterPty + Send>>,
    writer: Option<Box<dyn Write + Send>>,
    child: Option<Box<dyn Child + Send + Sync>>,
    exit_status: Option<ExitStatus>,
    raw_output: RawOutput,
    terminal_output: TerminalOutput,
    reader_thread: Option<JoinHandle<()>>,
}

impl PtyProcess {
    /// Spawn `/bin/sh -c <command>` in a new PTY session.
    pub fn spawn(options: PtySpawnOptions) -> Result<Self> {
        if options.mcp_token.is_empty() {
            bail!("GBUILD_MCP_TOKEN must not be empty");
        }

        let pair = native_pty_system()
            .openpty(options.size)
            .context("open PTY")?;

        let mut command = CommandBuilder::new("/bin/sh");
        command.arg("-c");
        command.arg(&options.command);
        if let Some(working_dir) = &options.working_dir {
            command.cwd(working_dir.as_os_str());
        }
        for (key, value) in &options.env {
            command.env(key, value);
        }
        // These are process identity credentials, so callers cannot override them.
        command.env(GBUILD_PROCESS_ID_ENV, options.process_id.to_string());
        command.env(GBUILD_MCP_TOKEN_ENV, &options.mcp_token);

        let reader = pair.master.try_clone_reader().context("clone PTY reader")?;
        let writer = pair.master.take_writer().context("take PTY writer")?;
        let mut child = pair
            .slave
            .spawn_command(command)
            .context("spawn command in PTY")?;
        drop(pair.slave);

        let pid = match child.process_id() {
            Some(pid) => pid,
            None => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(anyhow!("PTY child did not expose a process ID"));
            }
        };

        let raw_output = RawOutput::new(options.raw_buffer_capacity);
        let reader_output = raw_output.clone();
        let terminal_output = TerminalOutput::new(
            options.size.rows,
            options.size.cols,
            options.scrollback_lines,
        );
        let reader_terminal = terminal_output.clone();
        let reader_thread = match thread::Builder::new()
            .name(format!("gbuild-pty-{}-reader", options.process_id))
            .spawn(move || capture_output(reader, reader_output, reader_terminal))
        {
            Ok(thread) => thread,
            Err(error) => {
                let _ = signal_process_group(pid, Signal::SIGKILL);
                let _ = child.kill();
                let _ = child.wait();
                return Err(error).context("spawn PTY output reader");
            }
        };

        Ok(Self {
            gbuild_process_id: options.process_id,
            pid,
            master: Some(pair.master),
            writer: Some(writer),
            child: Some(child),
            exit_status: None,
            raw_output,
            terminal_output,
            reader_thread: Some(reader_thread),
        })
    }

    /// Stable gbuild process ID injected into the child.
    pub fn gbuild_process_id(&self) -> i64 {
        self.gbuild_process_id
    }

    /// Host operating-system process ID of the shell/session leader.
    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// Clone a thread-safe handle to the bounded raw output ring.
    pub fn raw_output(&self) -> RawOutput {
        self.raw_output.clone()
    }

    /// Clone a thread-safe handle to rendered grid and scrollback reads.
    pub fn terminal_output(&self) -> TerminalOutput {
        self.terminal_output.clone()
    }

    /// Write bytes to the terminal and flush them to the child.
    pub fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        let writer = self
            .writer
            .as_mut()
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "PTY writer is closed"))?;
        writer.write_all(bytes)?;
        writer.flush()
    }

    /// Resize the PTY, causing the kernel to deliver SIGWINCH as appropriate.
    pub fn resize(&self, size: PtySize) -> Result<()> {
        // Keep the parser blocked until its grid matches the kernel dimensions,
        // so output emitted immediately after SIGWINCH cannot use the old size.
        let mut terminal = self.terminal_output.lock();
        self.master
            .as_ref()
            .ok_or_else(|| anyhow!("PTY master is closed"))?
            .resize(size)
            .context("resize PTY")?;
        terminal.resize(size.rows, size.cols);
        Ok(())
    }

    /// Read the terminal dimensions currently recorded by the kernel.
    pub fn size(&self) -> Result<PtySize> {
        self.master
            .as_ref()
            .ok_or_else(|| anyhow!("PTY master is closed"))?
            .get_size()
            .context("read PTY size")
    }

    /// Poll the direct child without blocking.
    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        if let Some(status) = &self.exit_status {
            return Ok(Some(status.clone()));
        }

        let status = self
            .child
            .as_mut()
            .ok_or_else(|| io::Error::other("PTY child handle is closed"))?
            .try_wait()?;
        if let Some(status) = &status {
            self.exit_status = Some(status.clone());
        }
        Ok(status)
    }

    /// Wait for the direct child to exit and return its cached status.
    pub fn wait(&mut self) -> io::Result<ExitStatus> {
        if let Some(status) = &self.exit_status {
            return Ok(status.clone());
        }

        let status = self
            .child
            .as_mut()
            .ok_or_else(|| io::Error::other("PTY child handle is closed"))?
            .wait()?;
        self.exit_status = Some(status.clone());
        Ok(status)
    }

    /// Gracefully stop the complete process group, then force-kill on timeout.
    pub fn terminate(&mut self, grace_period: Duration) -> Result<ExitStatus> {
        if let Some(status) = self.try_wait().context("poll PTY child")? {
            return Ok(status);
        }

        signal_process_group(self.pid, Signal::SIGTERM).context("send SIGTERM to process group")?;
        if let Some(status) = self
            .wait_for_exit(grace_period)
            .context("wait for PTY child after SIGTERM")?
        {
            return Ok(status);
        }

        signal_process_group(self.pid, Signal::SIGKILL).context("send SIGKILL to process group")?;
        // Also target the direct child in case it changed process groups.
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
        }
        self.wait().context("reap PTY child after SIGKILL")
    }

    fn wait_for_exit(&mut self, timeout: Duration) -> io::Result<Option<ExitStatus>> {
        let started_at = Instant::now();
        loop {
            if let Some(status) = self.try_wait()? {
                return Ok(Some(status));
            }

            let elapsed = started_at.elapsed();
            if elapsed >= timeout {
                return Ok(None);
            }

            thread::sleep((timeout - elapsed).min(Duration::from_millis(10)));
        }
    }
}

impl fmt::Debug for PtyProcess {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PtyProcess")
            .field("gbuild_process_id", &self.gbuild_process_id)
            .field("pid", &self.pid)
            .field("exit_status", &self.exit_status)
            .field("raw_output", &self.raw_output)
            .field("terminal_output", &self.terminal_output)
            .finish_non_exhaustive()
    }
}

impl Drop for PtyProcess {
    fn drop(&mut self) {
        let running = self.try_wait().ok().flatten().is_none();
        if running {
            let _ = signal_process_group(self.pid, Signal::SIGKILL);
            if let Some(child) = self.child.as_mut() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }

        // Closing these handles releases the PTY. The reader owns a cloned fd;
        // detaching avoids a potentially unbounded join if a child escaped.
        drop(self.writer.take());
        drop(self.master.take());
        drop(self.reader_thread.take());
    }
}

fn capture_output(
    mut reader: Box<dyn Read + Send>,
    raw_output: RawOutput,
    terminal_output: TerminalOutput,
) {
    let mut chunk = [0_u8; 8192];
    loop {
        match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(count) => {
                raw_output.push(&chunk[..count]);
                terminal_output.feed(&chunk[..count]);
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            // Unix PTYs commonly report EIO when their final slave closes.
            Err(_) => break,
        }
    }
}

fn signal_process_group(pid: u32, signal: Signal) -> Result<()> {
    let pid = i32::try_from(pid).context("PTY process ID exceeds Unix pid_t")?;
    match killpg(Pid::from_raw(pid), signal) {
        Ok(()) | Err(Errno::ESRCH) => Ok(()),
        Err(error) => Err(error).with_context(|| format!("signal process group {pid}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wait_for_output(process: &PtyProcess, needle: &[u8]) -> Vec<u8> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let output = process.raw_output().snapshot();
            if output.windows(needle.len()).any(|window| window == needle) {
                return output;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for PTY output: {output:?}"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn wait_for_rendered_output(process: &PtyProcess, needle: &str) {
        let output = process.terminal_output();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if !output.search_rendered(needle, 1).is_empty() {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for rendered PTY output: {:?}",
                output.read_rows(0..usize::MAX)
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn parse_child_pid(output: &[u8]) -> u32 {
        String::from_utf8_lossy(output)
            .lines()
            .find_map(|line| line.trim_end_matches('\r').strip_prefix("child:"))
            .expect("child pid line")
            .parse()
            .expect("numeric child pid")
    }

    fn process_exists(pid: u32) -> bool {
        let Ok(pid) = i32::try_from(pid) else {
            return false;
        };
        match nix::sys::signal::kill(Pid::from_raw(pid), None) {
            Ok(()) | Err(Errno::EPERM) => true,
            Err(Errno::ESRCH) => false,
            Err(error) => panic!("failed to inspect process {pid}: {error}"),
        }
    }

    #[test]
    fn ring_retains_only_the_newest_bytes() {
        let mut ring = RawRingBuffer::new(5);
        ring.push(b"abc");
        ring.push(b"defg");

        assert_eq!(ring.snapshot(), b"cdefg");
        assert_eq!(ring.len(), 5);
        assert_eq!(ring.capacity(), 5);
        assert_eq!(ring.total_bytes_seen(), 7);
        assert_eq!(
            ring.search(b"de", 10),
            vec![RawSearchMatch {
                retained_offset: 1,
                stream_offset: 3,
            }]
        );

        ring.push(b"0123456789");
        assert_eq!(ring.snapshot(), b"56789");
        assert_eq!(ring.len(), 5);
        assert_eq!(ring.total_bytes_seen(), 17);
        assert_eq!(
            ring.search(b"78", 1),
            vec![RawSearchMatch {
                retained_offset: 2,
                stream_offset: 14,
            }]
        );
    }

    #[test]
    fn zero_capacity_ring_counts_but_discards_bytes() {
        let mut ring = RawRingBuffer::new(0);
        ring.push(b"discarded");

        assert!(ring.is_empty());
        assert_eq!(ring.snapshot(), b"");
        assert_eq!(ring.total_bytes_seen(), 9);
    }

    #[test]
    fn pty_injects_identity_writes_reads_and_resizes() {
        let command = concat!(
            "trap 'printf cleanup-complete\\n; exit 0' TERM; ",
            "printf '\\033[32menv:%s:%s\\033[0m\\n' ",
            "\"$GBUILD_PROCESS_ID\" \"$GBUILD_MCP_TOKEN\"; ",
            "IFS= read -r line; printf 'got:%s\\n' \"$line\"; sleep 30"
        );
        let options = PtySpawnOptions::new(42, "secret-token", command)
            .with_env(GBUILD_PROCESS_ID_ENV, "wrong")
            .with_env(GBUILD_MCP_TOKEN_ENV, "wrong");
        let mut process = PtyProcess::spawn(options).expect("spawn PTY process");

        wait_for_output(&process, b"env:42:secret-token");
        wait_for_rendered_output(&process, "env:42:secret-token");
        process.write_all(b"hello pty\n").expect("write PTY input");
        wait_for_output(&process, b"got:hello pty");
        wait_for_rendered_output(&process, "got:hello pty");

        let size = PtySize {
            rows: 40,
            cols: 120,
            pixel_width: 800,
            pixel_height: 600,
        };
        process.resize(size).expect("resize PTY");
        let actual = process.size().expect("read PTY size");
        assert_eq!(actual.rows, size.rows);
        assert_eq!(actual.cols, size.cols);
        assert_eq!(actual.pixel_width, size.pixel_width);
        assert_eq!(actual.pixel_height, size.pixel_height);
        assert_eq!(process.terminal_output().screen_rows(), 40);
        assert_eq!(process.terminal_output().columns(), 120);

        let status = process
            .terminate(Duration::from_millis(500))
            .expect("terminate PTY process");
        assert!(
            status.success(),
            "TERM trap should exit cleanly: {status:?}"
        );
        wait_for_output(&process, b"cleanup-complete");
    }

    #[test]
    fn terminate_kills_the_process_group() {
        let mut process = PtyProcess::spawn(PtySpawnOptions::new(
            43,
            "secret-token",
            "trap '' TERM; sleep 30 & child=$!; printf 'child:%s\\n' \"$child\"; wait",
        ))
        .expect("spawn process tree");
        let output = wait_for_output(&process, b"child:");
        let child_pid = parse_child_pid(&output);
        assert!(process_exists(child_pid));

        process
            .terminate(Duration::from_millis(50))
            .expect("terminate process group");

        let deadline = Instant::now() + Duration::from_secs(2);
        while process_exists(child_pid) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(
            !process_exists(child_pid),
            "descendant {child_pid} survived group kill"
        );
    }

    #[test]
    fn flood_output_never_exceeds_ring_capacity() {
        const CAPACITY: usize = 4096;
        let mut process = PtyProcess::spawn(
            PtySpawnOptions::new(44, "secret-token", "yes 0123456789")
                .with_raw_buffer_capacity(CAPACITY),
        )
        .expect("spawn noisy process");
        let output = process.raw_output();
        let deadline = Instant::now() + Duration::from_secs(5);
        while output.total_bytes_seen() < (CAPACITY * 8) as u64 && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }

        process
            .terminate(Duration::from_millis(500))
            .expect("terminate noisy process");
        assert!(output.total_bytes_seen() >= (CAPACITY * 8) as u64);
        assert_eq!(output.len(), CAPACITY);
        assert_eq!(output.snapshot().len(), CAPACITY);
    }
}
