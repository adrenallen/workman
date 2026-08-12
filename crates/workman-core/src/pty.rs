//! Unix PTY process hosting and bounded raw-output capture.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::future::Future;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{
    Arc, Mutex, MutexGuard,
    atomic::{AtomicBool, AtomicU64, Ordering},
    mpsc,
};
use std::task::{Context as TaskContext, Poll};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use event_listener::{Event, EventListener};
use nix::errno::Errno;
use nix::sys::signal::{Signal, killpg};
use nix::unistd::Pid;
use portable_pty::{Child, CommandBuilder, MasterPty, native_pty_system};

use crate::attention::{AgentState, AttentionTracker};
use crate::output_spill::{OutputSpill, OutputSpillSink};
use crate::terminal::{DEFAULT_SCROLLBACK_LINES, TerminalOutput};

/// Portable PTY exit status and terminal dimensions used by the host API.
pub use portable_pty::{ExitStatus, PtySize};

/// Environment variable that identifies the workman process to its child.
pub const WORKMAN_PROCESS_ID_ENV: &str = "WORKMAN_PROCESS_ID";

/// Environment variable carrying the per-process MCP bearer token.
pub const WORKMAN_MCP_TOKEN_ENV: &str = "WORKMAN_MCP_TOKEN";

/// Enable once-per-second per-process PTY parse/render and raw-copy diagnostics.
pub const WORKMAN_PTY_PROFILE_ENV: &str = "WORKMAN_PTY_PROFILE";

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

/// Maximum cadence for the rendered viewport used by daemon-side attention classification.
const ATTENTION_RENDER_CADENCE: Duration = Duration::from_millis(16);

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

/// A bounded read from the retained raw stream with absolute stream offsets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawOutputRead {
    pub data: Vec<u8>,
    pub start_offset: u64,
    pub end_offset: u64,
    pub total_bytes: u64,
    pub truncated: bool,
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

    /// Copy only the requested retained range, preserving absolute stream offsets.
    pub fn read(&self, offset: Option<u64>, max_bytes: usize) -> RawOutputRead {
        let retained_start = self
            .total_bytes_seen
            .saturating_sub(u64::try_from(self.len).unwrap_or(u64::MAX));
        let requested = offset.unwrap_or(retained_start);
        let start_offset = requested.clamp(retained_start, self.total_bytes_seen);
        let start = usize::try_from(start_offset - retained_start).unwrap_or(self.len);
        let data_len = max_bytes.min(self.len.saturating_sub(start));
        let mut data = Vec::with_capacity(data_len);
        if data_len > 0 {
            let physical_start = (self.start + start) % self.capacity();
            let first_len = data_len.min(self.capacity() - physical_start);
            data.extend_from_slice(&self.storage[physical_start..physical_start + first_len]);
            data.extend_from_slice(&self.storage[..data_len - first_len]);
        }
        let end_offset = start_offset.saturating_add(data.len() as u64);

        RawOutputRead {
            data,
            start_offset,
            end_offset,
            total_bytes: self.total_bytes_seen,
            truncated: requested < retained_start || end_offset < self.total_bytes_seen,
        }
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
#[derive(Debug)]
struct RawOutputInner {
    ring: Mutex<RawRingBuffer>,
    ready: Event,
}

/// One async notification that new raw PTY output may be available.
#[derive(Debug)]
pub struct RawOutputListener {
    inner: EventListener,
}

impl Future for RawOutputListener {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, context: &mut TaskContext<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.inner).poll(context)
    }
}

#[derive(Clone, Debug)]
pub struct RawOutput {
    inner: Arc<RawOutputInner>,
}

impl RawOutput {
    fn new(capacity: usize) -> Self {
        Self {
            inner: Arc::new(RawOutputInner {
                ring: Mutex::new(RawRingBuffer::new(capacity)),
                ready: Event::new(),
            }),
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
            .ring
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn push(&self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        self.lock().push(bytes);
        // A process can be attached by more than one client. Wake every current listener;
        // clients independently drain from their absolute stream offsets.
        self.inner.ready.notify(usize::MAX);
    }

    /// Listen for the next append. Callers should register before checking the stream offset,
    /// then skip awaiting when bytes are already available, so an append cannot be missed.
    pub fn listen(&self) -> RawOutputListener {
        RawOutputListener {
            inner: self.inner.ready.listen(),
        }
    }

    /// Copy the retained output, oldest byte first.
    pub fn snapshot(&self) -> Vec<u8> {
        self.lock().snapshot()
    }

    /// Copy only a bounded absolute range from the retained output.
    pub fn read(&self, offset: Option<u64>, max_bytes: usize) -> RawOutputRead {
        self.lock().read(offset, max_bytes)
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
    /// Stable workman database ID injected into the child environment.
    pub process_id: i64,
    /// Command passed as a single argument to the configured shell's `-c` flag.
    pub command: String,
    /// Optional command working directory.
    pub working_dir: Option<PathBuf>,
    /// Additional environment variables. Reserved workman variables win.
    pub env: BTreeMap<OsString, OsString>,
    /// Initial terminal dimensions.
    pub size: PtySize,
    /// Maximum number of raw output bytes retained in memory.
    pub raw_buffer_capacity: usize,
    /// Maximum number of rendered rows retained above the viewport.
    pub scrollback_lines: usize,
    /// Agent tool family used for terminal-attention classification.
    pub tool_type: Option<String>,
    shell: PathBuf,
    login_shell: bool,
    interactive_shell: bool,
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
            shell: PathBuf::from("/bin/sh"),
            login_shell: false,
            interactive_shell: false,
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

    /// Run the command through `<shell> -l -c <command>`.
    pub fn with_login_shell_command(mut self, shell: impl Into<PathBuf>) -> Self {
        self.shell = shell.into();
        self.login_shell = true;
        self.interactive_shell = false;
        self
    }

    /// Start `<shell> -l` as an interactive login-shell session.
    pub fn with_login_shell(mut self, shell: impl Into<PathBuf>) -> Self {
        self.shell = shell.into();
        self.login_shell = true;
        self.interactive_shell = true;
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
    workman_process_id: i64,
    pid: u32,
    master: Option<Box<dyn MasterPty + Send>>,
    writer: Option<Arc<Mutex<Box<dyn Write + Send>>>>,
    submission_tx: Option<mpsc::Sender<PtySubmission>>,
    submission_event_rx: mpsc::Receiver<PtySubmissionEvent>,
    child: Option<Box<dyn Child + Send + Sync>>,
    exit_status: Option<ExitStatus>,
    raw_output: RawOutput,
    terminal_output: TerminalOutput,
    attention: AttentionTracker,
    output_spill: Option<OutputSpill>,
    reader_finished: Arc<AtomicBool>,
    reader_thread: Option<JoinHandle<()>>,
    attention_thread: Option<JoinHandle<()>>,
    submission_thread: Option<JoinHandle<()>>,
}

/// Cloneable, process-local PTY input path.
///
/// The daemon keeps this handle outside its lifecycle registry so a write to one running
/// terminal never waits for an unrelated process to finish spawning or stopping.
#[derive(Clone)]
pub struct PtyInputHandle {
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    submission_tx: mpsc::Sender<PtySubmission>,
}

impl PtyInputHandle {
    /// Write bytes directly to this process's terminal and flush them to the child.
    pub fn write_all(&self, bytes: &[u8]) -> io::Result<()> {
        let mut writer = self
            .writer
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        writer.write_all(bytes)?;
        writer.flush()
    }

    /// Queue content followed by Enter as one ordered process-local submission.
    pub fn submit_input(&self, content: &[u8], key_delay: Duration) -> io::Result<()> {
        self.queue_submission(content, key_delay, None)
    }

    /// Queue a submission whose Enter is retried when no turn-start output appears.
    pub fn submit_input_verified(
        &self,
        content: &[u8],
        key_delay: Duration,
        verification: PtySubmissionVerification,
    ) -> io::Result<()> {
        self.queue_submission(content, key_delay, Some(verification))
    }

    fn queue_submission(
        &self,
        content: &[u8],
        key_delay: Duration,
        verification: Option<PtySubmissionVerification>,
    ) -> io::Result<()> {
        self.submission_tx
            .send(PtySubmission {
                content: content.to_vec(),
                key_delay,
                verification,
            })
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "PTY input worker is closed"))
    }
}

struct PtySubmission {
    content: Vec<u8>,
    key_delay: Duration,
    verification: Option<PtySubmissionVerification>,
}

/// Verification policy for an interactive-agent submission.
#[derive(Clone, Copy, Debug)]
pub struct PtySubmissionVerification {
    /// How long to wait for terminal evidence that Enter started a turn.
    pub timeout: Duration,
    /// Total Enter attempts, including the initial keypress.
    pub max_attempts: usize,
}

/// A retry/failure notice emitted by the process-local input worker.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PtySubmissionEventKind {
    Retried,
    Unverified,
}

/// Non-sensitive submission event suitable for a daemon process event log.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PtySubmissionEvent {
    pub kind: PtySubmissionEventKind,
    /// Enter attempt that was (or would have been) made.
    pub attempt: usize,
    pub max_attempts: usize,
    pub timeout: Duration,
}

impl PtyProcess {
    /// Spawn the configured shell command in a new PTY session.
    pub fn spawn(options: PtySpawnOptions) -> Result<Self> {
        if options.mcp_token.is_empty() {
            bail!("WORKMAN_MCP_TOKEN must not be empty");
        }

        let pair = native_pty_system()
            .openpty(options.size)
            .context("open PTY")?;

        let mut command = CommandBuilder::new(&options.shell);
        if options.login_shell {
            command.arg("-l");
        }
        if !options.interactive_shell {
            command.arg("-c");
            // Keep the complete command as one argv item. The login shell, not Workman,
            // owns its quoting and expansion semantics.
            command.arg(&options.command);
        }
        if let Some(working_dir) = &options.working_dir {
            command.cwd(working_dir.as_os_str());
        }
        for (key, value) in &options.env {
            command.env(key, value);
        }
        // These are process identity credentials, so callers cannot override them.
        command.env(WORKMAN_PROCESS_ID_ENV, options.process_id.to_string());
        command.env(WORKMAN_MCP_TOKEN_ENV, &options.mcp_token);

        let reader = pair.master.try_clone_reader().context("clone PTY reader")?;
        let writer = Arc::new(Mutex::new(
            pair.master.take_writer().context("take PTY writer")?,
        ));
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
        let reader_response_writer = Arc::clone(&writer);
        let attention = AttentionTracker::new(options.tool_type);
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
        let reader_attention = attention.clone();
        let capture_metrics = Arc::new(PtyCaptureMetrics {
            process_id: options.process_id,
            ..PtyCaptureMetrics::default()
        });
        let (attention_render_tx, attention_render_rx) = mpsc::sync_channel(1);
        let attention_terminal = terminal_output.clone();
        let attention_tracker = attention.clone();
        let attention_metrics = Arc::clone(&capture_metrics);
        let attention_thread = match thread::Builder::new()
            .name(format!("workman-pty-{}-attention", options.process_id))
            .spawn(move || {
                render_attention(
                    attention_render_rx,
                    attention_terminal,
                    attention_tracker,
                    attention_metrics,
                )
            }) {
            Ok(thread) => thread,
            Err(error) => {
                let _ = signal_process_group(pid, Signal::SIGKILL);
                let _ = child.kill();
                let _ = child.wait();
                return Err(error).context("spawn PTY attention renderer");
            }
        };
        let reader_thread = match thread::Builder::new()
            .name(format!("workman-pty-{}-reader", options.process_id))
            .spawn(move || {
                capture_output(
                    reader,
                    reader_output,
                    reader_terminal,
                    reader_spill,
                    reader_response_writer,
                    attention_render_tx,
                    capture_metrics,
                );
                reader_finished_flag.store(true, Ordering::Release);
                reader_attention.notify_change();
            }) {
            Ok(thread) => thread,
            Err(error) => {
                let _ = signal_process_group(pid, Signal::SIGKILL);
                let _ = child.kill();
                let _ = child.wait();
                let _ = attention_thread.join();
                return Err(error).context("spawn PTY output reader");
            }
        };
        let submission_writer = Arc::clone(&writer);
        let (submission_tx, submission_rx) = mpsc::channel();
        let (submission_event_tx, submission_event_rx) = mpsc::channel();
        let submission_output = raw_output.clone();
        let submission_attention = attention.clone();
        let submission_thread = match thread::Builder::new()
            .name(format!("workman-pty-{}-input", options.process_id))
            .spawn(move || {
                process_submissions(
                    submission_writer,
                    submission_rx,
                    submission_output,
                    submission_attention,
                    submission_event_tx,
                )
            }) {
            Ok(thread) => thread,
            Err(error) => {
                let _ = signal_process_group(pid, Signal::SIGKILL);
                let _ = child.kill();
                let _ = child.wait();
                return Err(error).context("spawn PTY input worker");
            }
        };

        Ok(Self {
            workman_process_id: options.process_id,
            pid,
            master: Some(pair.master),
            writer: Some(writer),
            submission_tx: Some(submission_tx),
            submission_event_rx,
            child: Some(child),
            exit_status: None,
            raw_output,
            terminal_output,
            attention,
            output_spill,
            reader_finished,
            reader_thread: Some(reader_thread),
            attention_thread: Some(attention_thread),
            submission_thread: Some(submission_thread),
        })
    }

    /// Stable workman process ID injected into the child.
    pub fn workman_process_id(&self) -> i64 {
        self.workman_process_id
    }

    /// Host operating-system process ID of the shell/session leader.
    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// Read the foreground process group currently attached to this PTY.
    ///
    /// Interactive shells keep their own process group in the foreground while
    /// resting at a prompt and hand the terminal to a job's process group while
    /// that job runs. A missing value is treated as unknown by telemetry callers.
    #[cfg(unix)]
    pub fn foreground_process_group(&self) -> Option<u32> {
        use std::os::fd::BorrowedFd;

        let raw_fd = self.master.as_ref()?.as_raw_fd()?;
        // SAFETY: `raw_fd` remains owned by `self.master` and the temporary
        // borrow cannot outlive this call.
        let borrowed = unsafe { BorrowedFd::borrow_raw(raw_fd) };
        let process_group = nix::unistd::tcgetpgrp(borrowed).ok()?.as_raw();
        u32::try_from(process_group).ok()
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

    /// Clone a process-local input handle independent of the owning lifecycle handle.
    pub fn input_handle(&self) -> Option<PtyInputHandle> {
        Some(PtyInputHandle {
            writer: Arc::clone(self.writer.as_ref()?),
            submission_tx: self.submission_tx.as_ref()?.clone(),
        })
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
        self.input_handle()
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "PTY writer is closed"))?
            .write_all(bytes)
    }

    /// Queue content followed by Enter as one ordered per-process submission.
    ///
    /// The input worker owns the boundary delay, so callers do not block while
    /// the terminal distinguishes pasted content from the Enter keypress.
    pub fn submit_input(&self, content: &[u8], key_delay: Duration) -> io::Result<()> {
        self.queue_submission(content, key_delay, None)
    }

    /// Queue a submission whose Enter keypress is retried when the terminal
    /// remains at its resting composer without producing turn-start output.
    pub fn submit_input_verified(
        &self,
        content: &[u8],
        key_delay: Duration,
        verification: PtySubmissionVerification,
    ) -> io::Result<()> {
        self.queue_submission(content, key_delay, Some(verification))
    }

    fn queue_submission(
        &self,
        content: &[u8],
        key_delay: Duration,
        verification: Option<PtySubmissionVerification>,
    ) -> io::Result<()> {
        self.input_handle()
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "PTY input worker is closed"))?
            .queue_submission(content, key_delay, verification)
    }

    /// Drain retry/failure notices produced since the previous read.
    pub fn submission_events(&self) -> Vec<PtySubmissionEvent> {
        self.submission_event_rx.try_iter().collect()
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
            .field("workman_process_id", &self.workman_process_id)
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
            if let Some(attention_thread) = self.attention_thread.take() {
                let _ = attention_thread.join();
            }
        } else {
            drop(self.reader_thread.take());
            drop(self.attention_thread.take());
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
    raw_output: RawOutput,
    attention: AttentionTracker,
    events: mpsc::Sender<PtySubmissionEvent>,
) {
    for submission in submissions {
        let output_before_content = raw_output.total_bytes_seen();
        if write_and_flush(&writer, &submission.content).is_err() {
            break;
        }
        thread::sleep(submission.key_delay);
        if submission.verification.is_some() {
            // Interactive composers redraw asynchronously after receiving the
            // pasted body. Establish the Enter baseline only after that redraw
            // settles, otherwise late draft output can masquerade as a turn.
            wait_for_output_quiet(&raw_output, output_before_content);
        }

        let attempts = submission
            .verification
            .map(|verification| verification.max_attempts.max(1))
            .unwrap_or(1);
        for attempt in 1..=attempts {
            let output_before_enter = raw_output.total_bytes_seen();
            if write_and_flush(&writer, b"\r").is_err() {
                return;
            }
            let Some(verification) = submission.verification else {
                break;
            };
            if wait_for_turn_start(
                &raw_output,
                &attention,
                output_before_enter,
                verification.timeout,
            ) {
                break;
            }
            if attempt < attempts {
                let _ = events.send(PtySubmissionEvent {
                    kind: PtySubmissionEventKind::Retried,
                    attempt: attempt + 1,
                    max_attempts: attempts,
                    timeout: verification.timeout,
                });
            } else {
                let _ = events.send(PtySubmissionEvent {
                    kind: PtySubmissionEventKind::Unverified,
                    attempt,
                    max_attempts: attempts,
                    timeout: verification.timeout,
                });
            }
        }
    }
}

fn write_and_flush(writer: &Arc<Mutex<Box<dyn Write + Send>>>, bytes: &[u8]) -> io::Result<()> {
    let mut writer = writer
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    writer.write_all(bytes)?;
    writer.flush()
}

fn wait_for_turn_start(
    raw_output: &RawOutput,
    attention: &AttentionTracker,
    output_before_enter: u64,
    timeout: Duration,
) -> bool {
    let deadline = Instant::now() + timeout;
    let mut last_total = output_before_enter;
    let mut first_change_at = None;
    loop {
        let total = raw_output.total_bytes_seen();
        if total > output_before_enter {
            let now = Instant::now();
            if total != last_total {
                first_change_at.get_or_insert(now);
                last_total = total;
            }
            let status = attention.snapshot();
            // A draft redraw still classifies as the resting composer. Busy,
            // permission, generic output, and exit output all prove that the
            // Enter key reached the application rather than its paste buffer.
            if status.classification.as_deref() != Some("resting_prompt") {
                return true;
            }
            // Codex keeps its composer row rendered while a turn runs. In that
            // layout the adapter still sees a resting prompt, so sustained PTY
            // activity (rather than a one-off draft redraw) is the proof.
            if first_change_at
                .is_some_and(|started| now.duration_since(started) >= Duration::from_millis(50))
            {
                return true;
            }
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(Duration::from_millis(10).min(timeout));
    }
}

fn wait_for_output_quiet(raw_output: &RawOutput, before_content: u64) {
    const QUIET_FOR: Duration = Duration::from_millis(50);
    const MAX_SETTLE: Duration = Duration::from_millis(500);

    let started = Instant::now();
    let mut last_total = raw_output.total_bytes_seen();
    let mut last_change = started;
    loop {
        let total = raw_output.total_bytes_seen();
        if total != last_total {
            last_total = total;
            last_change = Instant::now();
        }
        let saw_redraw = last_total > before_content;
        if saw_redraw && last_change.elapsed() >= QUIET_FOR {
            return;
        }
        if started.elapsed() >= MAX_SETTLE {
            return;
        }
        thread::sleep(Duration::from_millis(5));
    }
}

fn capture_output(
    mut reader: Box<dyn Read + Send>,
    raw_output: RawOutput,
    terminal_output: TerminalOutput,
    output_spill: Option<OutputSpillSink>,
    response_writer: Arc<Mutex<Box<dyn Write + Send>>>,
    attention_render_tx: mpsc::SyncSender<()>,
    metrics: Arc<PtyCaptureMetrics>,
) {
    let mut profiler = PtyCaptureProfiler::new(Arc::clone(&metrics));
    let mut chunk = [0_u8; 8192];
    loop {
        match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(count) => {
                metrics.parse_calls.fetch_add(1, Ordering::Relaxed);
                metrics
                    .parsed_bytes
                    .fetch_add(count as u64, Ordering::Relaxed);
                let replies = terminal_output.feed_with_replies(&chunk[..count]);
                // Publish raw bytes only after their terminal modes have been parsed. The daemon
                // attaches the current keyboard mode to each raw-output frame, so exposing the
                // bytes first could strand the frontend on the previous mode until more output.
                raw_output.push(&chunk[..count]);
                if let Some(spill) = &output_spill {
                    spill.push(&chunk[..count]);
                }
                if !replies.is_empty() {
                    let mut writer = response_writer
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    for reply in replies {
                        if writer.write_all(&reply).is_err() {
                            break;
                        }
                    }
                    let _ = writer.flush();
                }
                let _ = attention_render_tx.try_send(());
                profiler.report_if_due();
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            // Unix PTYs commonly report EIO when their final slave closes.
            Err(_) => break,
        }
    }
    profiler.report_final();
}

fn render_attention(
    attention_render_rx: mpsc::Receiver<()>,
    terminal_output: TerminalOutput,
    attention: AttentionTracker,
    metrics: Arc<PtyCaptureMetrics>,
) {
    let mut last_rendered_at = Instant::now()
        .checked_sub(ATTENTION_RENDER_CADENCE)
        .unwrap_or_else(Instant::now);
    while attention_render_rx.recv().is_ok() {
        let delay = ATTENTION_RENDER_CADENCE.saturating_sub(last_rendered_at.elapsed());
        if !delay.is_zero() {
            thread::sleep(delay);
        }
        while attention_render_rx.try_recv().is_ok() {}

        let rendered = terminal_output.read_viewport();
        metrics.render_calls.fetch_add(1, Ordering::Relaxed);
        attention.observe_output(b"\0", &rendered.text(), rendered.alternate_screen);
        last_rendered_at = Instant::now();
    }
}

#[derive(Default)]
struct PtyCaptureMetrics {
    process_id: i64,
    parse_calls: AtomicU64,
    parsed_bytes: AtomicU64,
    render_calls: AtomicU64,
}

struct PtyCaptureProfiler {
    process_id: i64,
    metrics: Arc<PtyCaptureMetrics>,
    enabled: bool,
    window_started: Instant,
    parse_calls: u64,
    parsed_bytes: u64,
    render_calls: u64,
}

impl PtyCaptureProfiler {
    fn new(metrics: Arc<PtyCaptureMetrics>) -> Self {
        Self {
            process_id: metrics.process_id,
            metrics,
            enabled: profile_enabled(),
            window_started: Instant::now(),
            parse_calls: 0,
            parsed_bytes: 0,
            render_calls: 0,
        }
    }

    fn report_if_due(&mut self) {
        if self.enabled && self.window_started.elapsed() >= Duration::from_secs(1) {
            self.report(false);
        }
    }

    fn report_final(&mut self) {
        if self.enabled {
            self.report(true);
        }
    }

    fn report(&mut self, final_report: bool) {
        let elapsed = self.window_started.elapsed();
        let parse_calls = self.metrics.parse_calls.load(Ordering::Relaxed);
        let parsed_bytes = self.metrics.parsed_bytes.load(Ordering::Relaxed);
        let render_calls = self.metrics.render_calls.load(Ordering::Relaxed);
        let window_parse_calls = parse_calls.saturating_sub(self.parse_calls);
        let window_parsed_bytes = parsed_bytes.saturating_sub(self.parsed_bytes);
        let window_render_calls = render_calls.saturating_sub(self.render_calls);
        if window_parse_calls > 0 || window_render_calls > 0 {
            eprintln!(
                "pty-profile process_id={} window_ms={} parse_calls={} parsed_bytes={} render_calls={} final={final_report}",
                self.process_id,
                elapsed.as_millis(),
                window_parse_calls,
                window_parsed_bytes,
                window_render_calls,
            );
        }
        self.window_started = Instant::now();
        self.parse_calls = parse_calls;
        self.parsed_bytes = parsed_bytes;
        self.render_calls = render_calls;
    }
}

fn profile_enabled() -> bool {
    std::env::var(WORKMAN_PTY_PROFILE_ENV).is_ok_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
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
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

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
            ring.read(Some(3), 3),
            RawOutputRead {
                data: b"def".to_vec(),
                start_offset: 3,
                end_offset: 6,
                total_bytes: 7,
                truncated: true,
            }
        );
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

        assert_eq!(
            ring.read(Some(13), 3),
            RawOutputRead {
                data: b"678".to_vec(),
                start_offset: 13,
                end_offset: 16,
                total_bytes: 17,
                truncated: true,
            }
        );
        assert_eq!(
            ring.read(Some(17), usize::MAX),
            RawOutputRead {
                data: Vec::new(),
                start_offset: 17,
                end_offset: 17,
                total_bytes: 17,
                truncated: false,
            }
        );
        assert_eq!(
            ring.read(Some(0), usize::MAX),
            RawOutputRead {
                data: b"56789".to_vec(),
                start_offset: 12,
                end_offset: 17,
                total_bytes: 17,
                truncated: true,
            }
        );
    }

    #[test]
    fn zero_capacity_ring_counts_but_discards_bytes() {
        let mut ring = RawRingBuffer::new(0);
        ring.push(b"discarded");

        assert!(ring.is_empty());
        assert_eq!(ring.snapshot(), b"");
        assert_eq!(ring.total_bytes_seen(), 9);
        assert_eq!(
            ring.read(Some(0), usize::MAX),
            RawOutputRead {
                data: Vec::new(),
                start_offset: 9,
                end_offset: 9,
                total_bytes: 9,
                truncated: true,
            }
        );
    }

    #[tokio::test]
    async fn raw_output_append_wakes_every_current_listener() {
        let output = RawOutput::new(64);
        let first = output.listen();
        let second = output.listen();
        output.push(b"ready");

        tokio::time::timeout(Duration::from_secs(1), async {
            tokio::join!(first, second);
        })
        .await
        .expect("raw output listeners were not notified");
        assert_eq!(output.snapshot(), b"ready");
    }

    #[test]
    fn attention_rendering_coalesces_bursts_and_flushes_the_final_viewport() {
        let terminal = TerminalOutput::new(24, 80, 100);
        let attention = AttentionTracker::new(None);
        let metrics = Arc::new(PtyCaptureMetrics::default());
        let (render_tx, render_rx) = mpsc::sync_channel(1);
        let render_thread = thread::spawn({
            let terminal = terminal.clone();
            let attention = attention.clone();
            let metrics = Arc::clone(&metrics);
            move || render_attention(render_rx, terminal, attention, metrics)
        });

        for _ in 0..1_000 {
            terminal.feed_with_replies(b"burst output\r\n");
            metrics.parse_calls.fetch_add(1, Ordering::Relaxed);
            metrics.parsed_bytes.fetch_add(14, Ordering::Relaxed);
            let _ = render_tx.try_send(());
        }
        let final_viewport = b"\x1b[2J\x1b[HFINAL-VIEWPORT-72";
        terminal.feed_with_replies(final_viewport);
        metrics.parse_calls.fetch_add(1, Ordering::Relaxed);
        metrics
            .parsed_bytes
            .fetch_add(final_viewport.len() as u64, Ordering::Relaxed);
        let _ = render_tx.try_send(());
        drop(render_tx);
        render_thread.join().expect("join attention renderer");

        let parse_calls = metrics.parse_calls.load(Ordering::Relaxed);
        let render_calls = metrics.render_calls.load(Ordering::Relaxed);
        assert!(render_calls > 0);
        assert!(
            render_calls < parse_calls / 10,
            "reader burst was not coalesced: parses={parse_calls}, renders={render_calls}"
        );
        assert!(
            terminal
                .read_viewport()
                .text()
                .contains("FINAL-VIEWPORT-72")
        );
        assert!(attention.snapshot().last_output_at.is_some());
    }

    #[test]
    fn pty_injects_identity_writes_reads_and_resizes() {
        let command = concat!(
            "trap 'printf cleanup-complete\\n; exit 0' TERM; ",
            "printf '\\033[32menv:%s:%s\\033[0m\\n' ",
            "\"$WORKMAN_PROCESS_ID\" \"$WORKMAN_MCP_TOKEN\"; ",
            "IFS= read -r line; printf 'got:%s\\n' \"$line\"; sleep 30"
        );
        let options = PtySpawnOptions::new(42, "secret-token", command)
            .with_env(WORKMAN_PROCESS_ID_ENV, "wrong")
            .with_env(WORKMAN_MCP_TOKEN_ENV, "wrong");
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
    fn pty_answers_supported_keyboard_protocol_queries_once() {
        let command = r#"stty raw -echo; printf '\033[>1u\033[>4;2m\033[?u\033[?4m'; exec perl -e '$|=1; my $reply=""; while (length($reply) < 12) { my $count = sysread(STDIN, my $chunk, 12 - length($reply)); exit 2 unless defined($count) && $count > 0; $reply .= $chunk; } print "REPLY:", unpack("H*", $reply), "\r\n";'"#;
        let mut process = PtyProcess::spawn(PtySpawnOptions::new(49, "token", command))
            .expect("spawn keyboard query fixture");

        wait_for_output(&process, b"REPLY:1b5b3f31751b5b3e343b326d");
        assert_eq!(
            process.terminal_output().keyboard_protocol(),
            crate::terminal::TerminalKeyboardProtocol {
                kitty_flags: 1,
                modify_other_keys: 2,
            }
        );
        assert!(
            process
                .wait()
                .expect("reap keyboard query fixture")
                .success()
        );
    }

    #[cfg(unix)]
    #[test]
    fn login_shell_keeps_command_quoting_and_injected_credentials() {
        let temp = tempfile::tempdir().unwrap();
        let shell = temp.path().join("login-shell");
        std::fs::write(
            &shell,
            concat!(
                "#!/bin/sh\n",
                "[ \"$1\" = -l ] || exit 91\n",
                "[ \"$2\" = -c ] || exit 92\n",
                ". \"$HOME/.profile\"\n",
                "shift\n",
                "exec /bin/sh \"$@\"\n",
            ),
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&shell).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&shell, permissions).unwrap();
        std::fs::write(
            temp.path().join(".profile"),
            "export PROFILE_MARKER='profile sourced'\n",
        )
        .unwrap();

        let command = r#"printf 'login:%s|token:%s|quoted:%s\n' "$PROFILE_MARKER" "$WORKMAN_MCP_TOKEN" "two words and a ' quote""#;
        let mut process = PtyProcess::spawn(
            PtySpawnOptions::new(48, "real-token", command)
                .with_env("HOME", temp.path())
                .with_env(WORKMAN_MCP_TOKEN_ENV, "wrong")
                .with_login_shell_command(&shell),
        )
        .expect("spawn command through login shell");
        let output = wait_for_output(&process, b"login:profile sourced|token:real-token");
        let output = String::from_utf8_lossy(&output);
        assert!(
            output.contains("quoted:two words and a ' quote"),
            "command was reinterpreted or split: {output}"
        );
        assert!(process.wait().unwrap().success());
    }

    #[cfg(unix)]
    #[test]
    fn foreground_process_group_tracks_an_interactive_shell_job() {
        fn wait_for_group(process: &PtyProcess, expected: impl Fn(u32) -> bool) -> u32 {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                if let Some(group) = process.foreground_process_group()
                    && expected(group)
                {
                    return group;
                }
                assert!(
                    Instant::now() < deadline,
                    "timed out waiting for PTY foreground group; root={} current={:?}",
                    process.pid(),
                    process.foreground_process_group()
                );
                thread::sleep(Duration::from_millis(10));
            }
        }

        let mut process = PtyProcess::spawn(
            PtySpawnOptions::new(50, "token", "/bin/sh").with_login_shell("/bin/sh"),
        )
        .expect("spawn interactive shell");
        process
            .write_all(b"printf '__WORKMAN_SHELL_READY__\\n'\n")
            .expect("probe interactive shell");
        wait_for_output(&process, b"__WORKMAN_SHELL_READY__");

        assert_eq!(
            wait_for_group(&process, |group| group == process.pid()),
            process.pid(),
            "the resting shell should own the foreground"
        );

        process
            .write_all(b"sleep 30\n")
            .expect("start foreground job");
        let job_group = wait_for_group(&process, |group| group != process.pid());
        assert_ne!(job_group, process.pid());

        process
            .write_all(&[0x03])
            .expect("interrupt foreground job");
        assert_eq!(
            wait_for_group(&process, |group| group == process.pid()),
            process.pid(),
            "the shell should reclaim the foreground after the job exits"
        );
        process
            .terminate(Duration::from_millis(500))
            .expect("stop interactive shell");
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
        assert!(
            status.success(),
            "submission fixture exited with {status:?}"
        );
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
