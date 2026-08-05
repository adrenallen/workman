//! Unix PTY process hosting and bounded raw-output capture.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::sync::{
    Arc, Mutex, MutexGuard,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use nix::errno::Errno;
use nix::sys::signal::{Signal, killpg};
use nix::unistd::Pid;
use portable_pty::{Child, CommandBuilder, MasterPty, native_pty_system};

use crate::attention::{AgentState, AttentionTracker};
use crate::output_spill::{OutputSpill, OutputSpillSink};
use crate::terminal::{DEFAULT_SCROLLBACK_LINES, TerminalOutput};

/// Portable PTY exit status and terminal dimensions used by the host API.
pub use portable_pty::{ExitStatus, PtySize};

/// Environment variable that identifies the awm process to its child.
pub const AWM_PROCESS_ID_ENV: &str = "AWM_PROCESS_ID";

/// Environment variable carrying the per-process MCP bearer token.
pub const AWM_MCP_TOKEN_ENV: &str = "AWM_MCP_TOKEN";

/// Default amount of raw PTY output retained for a process.
pub const DEFAULT_RAW_BUFFER_CAPACITY: usize = 4 * 1024 * 1024;

/// Default maximum raw-output bytes retained in a daemon-managed spill file.
pub const DEFAULT_OUTPUT_SPILL_CAPACITY: usize = 8 * 1024 * 1024;

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

    /// Rebuild a bounded raw ring from a retained disk tail.
    pub fn from_replay(capacity: usize, bytes: &[u8]) -> Self {
        let output = Self::new(capacity);
        output.push(bytes);
        output
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
    /// Stable awm database ID injected into the child environment.
    pub process_id: i64,
    /// Command passed as a single argument to `/bin/sh -c`.
    pub command: String,
    /// Optional command working directory.
    pub working_dir: Option<PathBuf>,
    /// Additional environment variables. Reserved awm variables win.
    pub env: BTreeMap<OsString, OsString>,
    /// Initial terminal dimensions.
    pub size: PtySize,
    /// Maximum number of raw output bytes retained in memory.
    pub raw_buffer_capacity: usize,
    /// Maximum number of rendered rows retained above the viewport.
    pub scrollback_lines: usize,
    /// Agent tool family used for terminal-attention classification.
    pub tool_type: Option<String>,
    output_spill: Option<OutputSpillOptions>,
    mcp_token: String,
}

struct OutputSpillOptions {
    path: PathBuf,
    capacity: usize,
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
            tool_type: None,
            output_spill: None,
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

    /// Select the per-tool attention adapter for this process.
    pub fn with_tool_type(mut self, tool_type: impl Into<String>) -> Self {
        self.tool_type = Some(tool_type.into());
        self
    }

    /// Persist a bounded raw-output tail asynchronously at `path`.
    pub fn with_output_spill(mut self, path: impl Into<PathBuf>, capacity: usize) -> Self {
        self.output_spill = Some(OutputSpillOptions {
            path: path.into(),
            capacity,
        });
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
            .field("tool_type", &self.tool_type)
            .field(
                "output_spill",
                &self
                    .output_spill
                    .as_ref()
                    .map(|spill| (&spill.path, spill.capacity)),
            )
            .field("mcp_token", &"[redacted]")
            .finish()
    }
}

/// A running command attached to a native PTY.
pub struct PtyProcess {
    awm_process_id: i64,
    pid: u32,
    master: Option<Box<dyn MasterPty + Send>>,
    writer: Option<Arc<Mutex<Box<dyn Write + Send>>>>,
    submission_tx: Option<mpsc::Sender<PtySubmission>>,
    child: Option<Box<dyn Child + Send + Sync>>,
    exit_status: Option<ExitStatus>,
    raw_output: RawOutput,
    terminal_output: TerminalOutput,
    attention: AttentionTracker,
    output_spill: Option<OutputSpill>,
    reader_finished: Arc<AtomicBool>,
    reader_thread: Option<JoinHandle<()>>,
    submission_thread: Option<JoinHandle<()>>,
}

struct PtySubmission {
    content: Vec<u8>,
    key_delay: Duration,
}

impl PtyProcess {
    /// Spawn `/bin/sh -c <command>` in a new PTY session.
    pub fn spawn(options: PtySpawnOptions) -> Result<Self> {
        if options.mcp_token.is_empty() {
            bail!("AWM_MCP_TOKEN must not be empty");
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
        command.env(AWM_PROCESS_ID_ENV, options.process_id.to_string());
        command.env(AWM_MCP_TOKEN_ENV, &options.mcp_token);

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
        let attention = AttentionTracker::new(options.tool_type);
        let reader_attention = attention.clone();
        let output_spill = match options
            .output_spill
            .map(|spill| OutputSpill::start(spill.path, spill.capacity))
            .transpose()
        {
            Ok(spill) => spill,
            Err(error) => {
                let _ = signal_process_group(pid, Signal::SIGKILL);
                let _ = child.kill();
                let _ = child.wait();
                return Err(error).context("initialize raw-output spill");
            }
        };
        let reader_spill = output_spill.as_ref().map(OutputSpill::sink);
        let reader_finished = Arc::new(AtomicBool::new(false));
        let reader_finished_flag = Arc::clone(&reader_finished);
        let reader_thread = match thread::Builder::new()
            .name(format!("awm-pty-{}-reader", options.process_id))
            .spawn(move || {
                capture_output(
                    reader,
                    reader_output,
                    reader_terminal,
                    reader_attention,
                    reader_spill,
                );
                reader_finished_flag.store(true, Ordering::Release);
            }) {
            Ok(thread) => thread,
            Err(error) => {
                let _ = signal_process_group(pid, Signal::SIGKILL);
                let _ = child.kill();
                let _ = child.wait();
                return Err(error).context("spawn PTY output reader");
            }
        };
        let writer = Arc::new(Mutex::new(writer));
        let submission_writer = Arc::clone(&writer);
        let (submission_tx, submission_rx) = mpsc::channel();
        let submission_thread = match thread::Builder::new()
            .name(format!("awm-pty-{}-input", options.process_id))
            .spawn(move || process_submissions(submission_writer, submission_rx))
        {
            Ok(thread) => thread,
            Err(error) => {
                let _ = signal_process_group(pid, Signal::SIGKILL);
                let _ = child.kill();
                let _ = child.wait();
                return Err(error).context("spawn PTY input worker");
            }
        };

        Ok(Self {
            awm_process_id: options.process_id,
            pid,
            master: Some(pair.master),
            writer: Some(writer),
            submission_tx: Some(submission_tx),
            child: Some(child),
            exit_status: None,
            raw_output,
            terminal_output,
            attention,
            output_spill,
            reader_finished,
            reader_thread: Some(reader_thread),
            submission_thread: Some(submission_thread),
        })
    }

    /// Stable awm process ID injected into the child.
    pub fn awm_process_id(&self) -> i64 {
        self.awm_process_id
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

    /// Clone the attention tracker driven by this process's PTY output.
    pub fn attention_tracker(&self) -> AttentionTracker {
        self.attention.clone()
    }

    /// Read the current tool-aware attention state.
    pub fn agent_state(&self) -> AgentState {
        self.attention.snapshot()
    }

    /// Flush the current raw-output batch to disk, when persistence is enabled.
    pub fn flush_output_spill(&self) -> io::Result<()> {
        match &self.output_spill {
            Some(spill) => spill.flush(),
            None => Ok(()),
        }
    }

    /// Clear the persisted tail without detaching the running PTY reader.
    pub fn clear_output_spill(&self) -> io::Result<()> {
        match &self.output_spill {
            Some(spill) => spill.clear(),
            None => Ok(()),
        }
    }

    /// Write bytes to the terminal and flush them to the child.
    pub fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        let writer = self
            .writer
            .as_ref()
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "PTY writer is closed"))?;
        let mut writer = writer
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        writer.write_all(bytes)?;
        writer.flush()
    }

    /// Queue content followed by Enter as one ordered per-process submission.
    ///
    /// The input worker owns the boundary delay, so callers do not block while
    /// the terminal distinguishes pasted content from the Enter keypress.
    pub fn submit_input(&self, content: &[u8], key_delay: Duration) -> io::Result<()> {
        if self.writer.is_none() {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "PTY writer is closed",
            ));
        }
        self.submission_tx
            .as_ref()
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "PTY input worker is closed"))?
            .send(PtySubmission {
                content: content.to_vec(),
                key_delay,
            })
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "PTY input worker is closed"))
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
            self.attention.mark_exited();
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
        self.attention.mark_exited();
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

        self.kill()
    }

    /// Immediately kill the complete process group and reap the direct child.
    pub fn kill(&mut self) -> Result<ExitStatus> {
        if let Some(status) = self.try_wait().context("poll PTY child")? {
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
            .field("awm_process_id", &self.awm_process_id)
            .field("pid", &self.pid)
            .field("exit_status", &self.exit_status)
            .field("raw_output", &self.raw_output)
            .field("terminal_output", &self.terminal_output)
            .field("attention", &self.attention)
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
        self.attention.mark_exited();

        // Closing these handles releases the PTY. The reader owns a cloned fd;
        // detaching avoids a potentially unbounded join if a child escaped.
        drop(self.submission_tx.take());
        drop(self.writer.take());
        drop(self.master.take());
        let reader_deadline = Instant::now() + Duration::from_millis(250);
        while !self.reader_finished.load(Ordering::Acquire) && Instant::now() < reader_deadline {
            thread::sleep(Duration::from_millis(5));
        }
        if self.reader_finished.load(Ordering::Acquire) {
            if let Some(reader_thread) = self.reader_thread.take() {
                let _ = reader_thread.join();
            }
        } else {
            drop(self.reader_thread.take());
        }
        if let Some(mut spill) = self.output_spill.take() {
            let _ = spill.shutdown();
        }
        drop(self.submission_thread.take());
    }
}

fn process_submissions(
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    submissions: mpsc::Receiver<PtySubmission>,
) {
    for submission in submissions {
        let mut writer = writer
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let result = writer
            .write_all(&submission.content)
            .and_then(|()| writer.flush())
            .and_then(|()| {
                thread::sleep(submission.key_delay);
                writer.write_all(b"\r")
            })
            .and_then(|()| writer.flush());
        if result.is_err() {
            break;
        }
    }
}

fn capture_output(
    mut reader: Box<dyn Read + Send>,
    raw_output: RawOutput,
    terminal_output: TerminalOutput,
    attention: AttentionTracker,
    output_spill: Option<OutputSpillSink>,
) {
    let mut chunk = [0_u8; 8192];
    loop {
        match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(count) => {
                raw_output.push(&chunk[..count]);
                if let Some(spill) = &output_spill {
                    spill.push(&chunk[..count]);
                }
                let rendered = terminal_output.feed_and_read_viewport(&chunk[..count]);
                attention.observe_output(
                    &chunk[..count],
                    &rendered.text(),
                    rendered.alternate_screen,
                );
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
            "\"$AWM_PROCESS_ID\" \"$AWM_MCP_TOKEN\"; ",
            "IFS= read -r line; printf 'got:%s\\n' \"$line\"; sleep 30"
        );
        let options = PtySpawnOptions::new(42, "secret-token", command)
            .with_env(AWM_PROCESS_ID_ENV, "wrong")
            .with_env(AWM_MCP_TOKEN_ENV, "wrong");
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
    fn submission_queue_is_nonblocking_and_preserves_per_process_order() {
        let command = r#"stty raw -echo; printf 'READY\r\n'; exec perl -e '$|=1; my $message=""; while (1) { my $count = sysread(STDIN, my $chunk, 4096); exit 2 unless defined($count) && $count > 0; for my $character (split //, $chunk) { if ($character eq "\r") { print "MSG:$message\r\n"; exit 0 if $message eq "second"; $message = ""; } else { $message .= $character; } } }'"#;
        let mut process = PtyProcess::spawn(PtySpawnOptions::new(77, "token", command)).unwrap();
        wait_for_output(&process, b"READY");

        // A deliberately large boundary makes a synchronous implementation
        // obvious while keeping the test fast. Both submissions must enqueue
        // immediately and the process-local worker must serialize them.
        let started = Instant::now();
        process
            .submit_input(b"first", Duration::from_millis(250))
            .unwrap();
        process
            .submit_input(b"second", Duration::from_millis(250))
            .unwrap();
        assert!(
            started.elapsed() < Duration::from_millis(125),
            "submission enqueue unexpectedly blocked for {:?}",
            started.elapsed()
        );

        let output = wait_for_output(&process, b"MSG:second");
        let output = String::from_utf8_lossy(&output);
        let first = output.find("MSG:first").expect("first submission output");
        let second = output.find("MSG:second").expect("second submission output");
        assert!(first < second, "submissions were reordered: {output}");
        assert!(
            !output.contains("MSG:firstsecond"),
            "submissions interleaved"
        );
        let status = process.wait().expect("reap completed submission fixture");
        assert!(status.success(), "submission fixture exited with {status:?}");
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
    fn kill_immediately_kills_the_process_group() {
        let mut process = PtyProcess::spawn(PtySpawnOptions::new(
            44,
            "secret-token",
            "sleep 30 & child=$!; printf 'child:%s\\n' \"$child\"; wait",
        ))
        .expect("spawn process tree");
        let output = wait_for_output(&process, b"child:");
        let child_pid = parse_child_pid(&output);
        assert!(process_exists(child_pid));

        process.kill().expect("kill process group");

        let deadline = Instant::now() + Duration::from_secs(2);
        while process_exists(child_pid) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(
            !process_exists(child_pid),
            "descendant {child_pid} survived immediate group kill"
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

    #[test]
    fn write_behind_spill_keeps_up_with_yes_flood() {
        const TARGET_BYTES: u64 = 8 * 1024 * 1024;

        fn measure(process_id: i64, spill_path: Option<PathBuf>) -> (Duration, u64) {
            let mut options = PtySpawnOptions::new(process_id, "secret-token", "yes 0123456789")
                .with_raw_buffer_capacity(DEFAULT_OUTPUT_SPILL_CAPACITY);
            if let Some(path) = spill_path {
                options = options.with_output_spill(path, DEFAULT_OUTPUT_SPILL_CAPACITY);
            }
            let mut process = PtyProcess::spawn(options).expect("spawn yes flood fixture");
            let output = process.raw_output();
            let started = Instant::now();
            let deadline = started + Duration::from_secs(5);
            while output.total_bytes_seen() < TARGET_BYTES && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(5));
            }
            let elapsed = started.elapsed();
            let bytes = output.total_bytes_seen();
            process
                .terminate(Duration::from_millis(100))
                .expect("terminate yes flood fixture");
            process
                .flush_output_spill()
                .expect("flush yes flood fixture");
            assert!(
                bytes >= TARGET_BYTES,
                "yes flood captured only {bytes} bytes in {elapsed:?}"
            );
            (elapsed, bytes)
        }

        let baseline = measure(46, None);
        let temp = tempfile::tempdir().unwrap();
        let persisted = measure(47, Some(temp.path().join("47.raw")));
        let baseline_rate = baseline.1 as f64 / baseline.0.as_secs_f64();
        let persisted_rate = persisted.1 as f64 / persisted.0.as_secs_f64();
        eprintln!(
            "yes flood: memory={:.1} MiB/s, persisted={:.1} MiB/s, ratio={:.2}",
            baseline_rate / (1024.0 * 1024.0),
            persisted_rate / (1024.0 * 1024.0),
            persisted_rate / baseline_rate,
        );
        assert!(
            persisted.0 <= baseline.0.saturating_mul(3).max(Duration::from_secs(2)),
            "write-behind spill regressed yes throughput: memory={baseline:?}, persisted={persisted:?}"
        );
        assert!(
            std::fs::metadata(temp.path().join("47.raw")).unwrap().len()
                <= DEFAULT_OUTPUT_SPILL_CAPACITY as u64
        );
    }

    #[test]
    fn pty_output_drives_tool_aware_attention_state() {
        let command = concat!(
            "printf '\\033[2J\\033[HClaude wants to use Bash\\n'; ",
            "printf 'Do you want to proceed?\\n❯ 1. Yes, allow\\n'; ",
            "sleep 30"
        );
        let options =
            PtySpawnOptions::new(45, "secret-token", command).with_tool_type("claude_code");
        let mut process = PtyProcess::spawn(options).expect("spawn Claude-like PTY process");

        wait_for_rendered_output(&process, "Do you want to proceed?");
        let deadline = Instant::now() + Duration::from_secs(2);
        let state = loop {
            let state = process.agent_state();
            if state.state == crate::attention::AttentionState::NeedsInput {
                break state;
            }
            assert!(
                Instant::now() < deadline,
                "permission dialog was not classified: {state:?}"
            );
            thread::yield_now();
        };
        assert_eq!(state.state, crate::attention::AttentionState::NeedsInput);
        assert!(state.needs_input);
        assert!(!state.idle, "permission prompts must never look done");

        process
            .terminate(Duration::from_millis(100))
            .expect("terminate PTY process");
        assert_eq!(
            process.agent_state().state,
            crate::attention::AttentionState::Exited
        );
    }
}
