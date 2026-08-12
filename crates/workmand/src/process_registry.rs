//! Persistent process registry and PTY lifecycle orchestration.

use std::{
    collections::{HashMap, HashSet},
    error::Error,
    fmt,
    fs::{self, File},
    io::{self, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use uuid::Uuid;
use workman_core::{
    AgentLaunchMode, AgentTool, ClaimedTodo, Process, ProcessId, ProcessKind, ProcessSource,
    ProcessStatus, ProjectId, Store, StoreError, TimerKind,
    attention::{
        AgentState, AgentWaitingProcess, AgentWaitingReason, AttentionState, AttentionTracker,
        PendingDialog, pending_dialog,
    },
    pty::{
        DEFAULT_OUTPUT_SPILL_CAPACITY, DEFAULT_PTY_SIZE, ExitStatus, PtyProcess, PtySize,
        PtySpawnOptions, PtySubmissionEventKind, PtySubmissionVerification, RawOutput,
        WORKMAN_PTY_PROFILE_ENV,
    },
    terminal::{DEFAULT_SCROLLBACK_LINES, TerminalKeyboardProtocol, TerminalOutput},
};

// Preserve a minimum packet boundary; the process-local worker additionally
// waits for the composer redraw to settle and verifies that Enter starts a turn.
const SUBMIT_KEY_DELAY: Duration = Duration::from_millis(5);
/// A healthy interactive agent redraws its busy state promptly after Enter.
const SUBMIT_VERIFY_TIMEOUT: Duration = Duration::from_secs(1);
/// Initial Enter plus two bounded bare-CR recovery attempts.
const SUBMIT_MAX_ATTEMPTS: usize = 3;

use crate::agent_sessions::SessionCapture;
use crate::config::{
    TrustFieldChange, TrustFields, TrustReview, is_process_trusted, trust_hash_for_process,
    validate_process_working_dir,
};
use crate::mcp::agent_spawning::WORKMAN_EPHEMERAL_AGENT_HOME_ENV;
use crate::process_tree::TrackedProcessTree;
use crate::status_invalidation::StatusInvalidationHub;
use crate::user_config::user_config_path;
use crate::user_environment::{ResolvedUserEnvironment, UserEnvironmentResolver};

const DEFAULT_STOP_GRACE: Duration = Duration::from_millis(500);

/// Directory below the daemon data root containing bounded raw-output tails.
pub const OUTPUT_DIRECTORY: &str = "output";

/// Optional byte-cap override for per-process raw-output spill files.
pub const WORKMAN_OUTPUT_CAPACITY_ENV: &str = "WORKMAN_OUTPUT_CAPACITY_BYTES";

/// Errors returned by process registry operations.
#[derive(Debug)]
pub enum RegistryError {
    Store(StoreError),
    NotFound(ProcessId),
    AlreadyExists(ProcessId),
    AlreadyRunning(ProcessId),
    NotRunning(ProcessId),
    Untrusted(ProcessId),
    NotYmlBacked(ProcessId),
    TrustHashMismatch(ProcessId),
    InvalidWorkingDirectory {
        process_id: ProcessId,
        message: String,
    },
    MissingCommand(ProcessId),
    InvalidName,
    Pty {
        process_id: ProcessId,
        message: String,
    },
    OutputPersistence {
        process_id: ProcessId,
        message: String,
    },
}

impl RegistryError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Store(_) => "store_error",
            Self::NotFound(_) => "process_not_found",
            Self::AlreadyExists(_) => "process_already_exists",
            Self::AlreadyRunning(_) => "process_already_running",
            Self::NotRunning(_) => "process_not_running",
            Self::Untrusted(_) => "process_untrusted",
            Self::NotYmlBacked(_) => "process_not_yml_backed",
            Self::TrustHashMismatch(_) => "trust_hash_mismatch",
            Self::InvalidWorkingDirectory { .. } => "invalid_working_directory",
            Self::MissingCommand(_) => "process_missing_command",
            Self::InvalidName => "invalid_process_name",
            Self::Pty { .. } => "pty_error",
            Self::OutputPersistence { .. } => "output_persistence_error",
        }
    }
}

impl fmt::Display for RegistryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(error) => error.fmt(formatter),
            Self::NotFound(id) => write!(formatter, "process {id} was not found"),
            Self::AlreadyExists(id) => write!(formatter, "process {id} already exists"),
            Self::AlreadyRunning(id) => write!(formatter, "process {id} is already running"),
            Self::NotRunning(id) => write!(formatter, "process {id} is not running"),
            Self::Untrusted(id) => write!(
                formatter,
                "workman.yml process {id} must be trusted before it can start"
            ),
            Self::NotYmlBacked(id) => {
                write!(formatter, "process {id} is not backed by workman.yml")
            }
            Self::TrustHashMismatch(id) => write!(
                formatter,
                "workman.yml process {id} changed since it was reviewed"
            ),
            Self::InvalidWorkingDirectory {
                process_id,
                message,
            } => write!(
                formatter,
                "workman.yml process {process_id} has an invalid working directory: {message}"
            ),
            Self::MissingCommand(id) => write!(formatter, "process {id} has no command to start"),
            Self::InvalidName => formatter.write_str("process name must not be empty"),
            Self::Pty {
                process_id,
                message,
            } => write!(
                formatter,
                "PTY operation for process {process_id} failed: {message}"
            ),
            Self::OutputPersistence {
                process_id,
                message,
            } => write!(
                formatter,
                "output persistence for process {process_id} failed: {message}"
            ),
        }
    }
}

impl Error for RegistryError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Store(error) => Some(error),
            _ => None,
        }
    }
}

impl From<StoreError> for RegistryError {
    fn from(error: StoreError) -> Self {
        Self::Store(error)
    }
}

pub type RegistryResult<T> = Result<T, RegistryError>;

/// A bounded slice of a process's retained raw PTY byte stream.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RawOutputChunk {
    pub data: Vec<u8>,
    pub start_offset: u64,
    pub end_offset: u64,
    pub total_bytes: u64,
    pub truncated: bool,
    pub status: ProcessStatus,
}

/// Escape-free terminal text together with the raw cursor used for following.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RenderedProcessOutput {
    pub text: String,
    pub raw_end_offset: u64,
    pub status: ProcessStatus,
}

/// A clamped, zero-based, end-exclusive slice of rendered terminal rows.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RenderedOutputRange {
    pub text: String,
    pub start: usize,
    pub end: usize,
    pub total_rows: usize,
    pub viewport_start: usize,
    pub cursor_row: usize,
    pub alternate_screen: bool,
    pub raw_end_offset: u64,
    pub status: ProcessStatus,
}

/// One case-insensitive match in escape-free rendered output.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RenderedOutputSearchMatch {
    /// One-based retained terminal row.
    pub row: usize,
    pub byte_start: usize,
    pub byte_end: usize,
    pub text: String,
}

/// One case-insensitive match in the retained raw PTY stream.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RawOutputSearchMatch {
    /// One-based raw text line within the retained ring snapshot.
    pub line: usize,
    pub stream_offset: u64,
    pub byte_start: usize,
    pub byte_end: usize,
    pub text: String,
}

/// Per-process failure reported by a bulk command operation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BulkFailure {
    pub process_id: ProcessId,
    pub code: String,
    pub message: String,
}

/// Results from start/stop/restart-all. One bad entry does not block its siblings.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct BulkProcessResult {
    pub processes: Vec<Process>,
    pub failures: Vec<BulkFailure>,
}

/// A durable process record plus live, non-persisted attention signals.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ProcessStatusView {
    #[serde(flatten)]
    pub process: Process,
    pub agent_state: AgentState,
    /// Ephemeral lifecycle notices, including automatic dialog acknowledgments.
    pub events: Vec<ProcessEvent>,
    /// Conversation ID passively discovered from the agent CLI's own session store.
    pub agent_session_id: Option<String>,
    /// Strategy selected by the most recent start: exact resume, cwd latest, or fresh.
    pub agent_launch_mode: Option<AgentLaunchMode>,
    /// Unexpired todo leases held by MCP actors attached to this process.
    pub claimed_todos: Vec<ClaimedTodo>,
}

/// A visible event produced by daemon-side process orchestration.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProcessEvent {
    pub at: i64,
    pub kind: String,
    pub message: String,
}

/// Owns persisted process records and live PTY handles.
pub struct ProcessRegistry {
    store: Store,
    status_invalidations: StatusInvalidationHub,
    running: HashMap<ProcessId, PtyProcess>,
    /// Last geometry measured by a desktop terminal surface, including while stopped.
    /// Keeping this process-local lets the next spawn start at the correct size before the
    /// child emits its first frame, while a newly attached desktop can immediately replace it.
    pty_sizes: HashMap<ProcessId, PtySize>,
    outputs: HashMap<ProcessId, ProcessOutput>,
    selected: HashMap<ProjectId, ProcessId>,
    trust_snapshots: HashMap<ProcessId, TrustFields>,
    stop_grace: Duration,
    output_persistence: Option<OutputPersistence>,
    user_environment: UserEnvironmentResolver,
    agent_session_captures: HashMap<ProcessId, PendingSessionCapture>,
    raw_output_profiles: Option<HashMap<ProcessId, RawOutputProfile>>,
}

#[derive(Debug)]
struct RawOutputProfile {
    window_started: Instant,
    calls: u64,
    ring_copy_bytes: u64,
    response_copy_bytes: u64,
    empty_calls: u64,
}

impl Default for RawOutputProfile {
    fn default() -> Self {
        Self {
            window_started: Instant::now(),
            calls: 0,
            ring_copy_bytes: 0,
            response_copy_bytes: 0,
            empty_calls: 0,
        }
    }
}

#[derive(Clone, Debug)]
struct PendingSessionCapture {
    capture: SessionCapture,
    root_pid: u32,
    last_checked_at: i64,
}

#[derive(Clone, Debug)]
struct OutputPersistence {
    directory: PathBuf,
    capacity: usize,
}

impl ProcessRegistry {
    /// Create a registry and mark process rows left running by an earlier daemon as crashed.
    pub fn new(store: Store) -> RegistryResult<Self> {
        Self::with_options(
            store,
            DEFAULT_STOP_GRACE,
            None,
            UserEnvironmentResolver::new(user_config_path()),
        )
    }

    pub fn with_stop_grace(store: Store, stop_grace: Duration) -> RegistryResult<Self> {
        Self::with_options(
            store,
            stop_grace,
            None,
            UserEnvironmentResolver::new(user_config_path()),
        )
    }

    /// Create a registry with an explicit resolver (primarily for isolated embedding/tests).
    pub fn with_user_environment(
        store: Store,
        user_environment: UserEnvironmentResolver,
    ) -> RegistryResult<Self> {
        Self::with_options(store, DEFAULT_STOP_GRACE, None, user_environment)
    }

    /// Create a registry whose PTY output survives daemon restarts in `output_directory`.
    pub fn with_output_persistence(
        store: Store,
        output_directory: impl Into<PathBuf>,
        capacity: usize,
    ) -> RegistryResult<Self> {
        Self::with_output_persistence_and_environment(
            store,
            output_directory,
            capacity,
            UserEnvironmentResolver::new(user_config_path()),
        )
    }

    /// Create a persistent registry using one shared user-environment resolver.
    pub fn with_output_persistence_and_environment(
        store: Store,
        output_directory: impl Into<PathBuf>,
        capacity: usize,
        user_environment: UserEnvironmentResolver,
    ) -> RegistryResult<Self> {
        Self::with_options(
            store,
            DEFAULT_STOP_GRACE,
            Some(OutputPersistence {
                directory: output_directory.into(),
                capacity,
            }),
            user_environment,
        )
    }

    fn with_options(
        store: Store,
        stop_grace: Duration,
        output_persistence: Option<OutputPersistence>,
        user_environment: UserEnvironmentResolver,
    ) -> RegistryResult<Self> {
        let trust_snapshots = store
            .list_processes(None)?
            .into_iter()
            .filter(|process| process.source == ProcessSource::Yml && is_process_trusted(process))
            .map(|process| (process.id, TrustFields::from_process(&process)))
            .collect();
        let mut registry = Self {
            store,
            status_invalidations: StatusInvalidationHub::default(),
            running: HashMap::new(),
            pty_sizes: HashMap::new(),
            outputs: HashMap::new(),
            selected: HashMap::new(),
            trust_snapshots,
            stop_grace,
            output_persistence,
            user_environment,
            agent_session_captures: HashMap::new(),
            raw_output_profiles: profile_enabled().then(HashMap::new),
        };
        registry.reconcile_stale_processes()?;
        registry.reload_persisted_outputs()?;
        Ok(registry)
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    pub fn store_mut(&mut self) -> &mut Store {
        &mut self.store
    }

    pub(crate) fn status_invalidations(&self) -> StatusInvalidationHub {
        self.status_invalidations.clone()
    }

    pub fn resolved_user_environment(&self) -> ResolvedUserEnvironment {
        self.user_environment.resolve()
    }

    pub fn user_environment_resolver(&self) -> &UserEnvironmentResolver {
        &self.user_environment
    }

    /// Insert a new stopped process. An ID <= 0 is replaced with the next database ID.
    pub fn create(&mut self, mut process: Process) -> RegistryResult<Process> {
        if process.id <= 0 {
            process.id = self.store.next_process_id()?;
        } else if self.store.get_process(process.id)?.is_some() {
            return Err(RegistryError::AlreadyExists(process.id));
        }
        validate_name(&process.name)?;
        process.sort_order = self
            .store
            .next_process_sort_order(process.project_id, process.kind)?;
        process.status = ProcessStatus::Stopped;
        process.pid = None;
        process.exit_code = None;
        process.exit_signal = None;
        process.exited_at = None;
        if process.source == ProcessSource::Yml {
            process.trust_hash = None;
        }
        self.store.put_process(&process)?;
        self.status_invalidations.invalidate();
        Ok(process)
    }

    /// Update process configuration while preserving registry-owned runtime fields.
    pub fn update(&mut self, mut process: Process) -> RegistryResult<Process> {
        self.refresh_exits()?;
        validate_name(&process.name)?;
        let current = self.require(process.id)?;
        process.status = current.status;
        process.pid = current.pid;
        process.exit_code = current.exit_code;
        process.exit_signal = current.exit_signal;
        process.exited_at = current.exited_at;
        process.sort_order = current.sort_order;
        let current_hash = trust_hash_for_process(&current);
        let updated_hash = trust_hash_for_process(&process);
        let trust_still_applies = current.source == ProcessSource::Yml
            && process.source == ProcessSource::Yml
            && current.trust_hash.as_deref() == Some(current_hash.as_str())
            && current_hash == updated_hash;
        process.trust_hash = trust_still_applies.then_some(current_hash);
        self.store.put_process(&process)?;
        self.status_invalidations.invalidate();
        Ok(process)
    }

    pub fn get(&mut self, process_id: ProcessId) -> RegistryResult<Process> {
        self.refresh_exits()?;
        self.require(process_id)
    }

    /// Foreground process group reported by a live PTY, when the host exposes it.
    pub(crate) fn foreground_process_group(&self, process_id: ProcessId) -> Option<u32> {
        self.running
            .get(&process_id)
            .and_then(PtyProcess::foreground_process_group)
    }

    /// Get a process with raw signals, adapter flags, and derived attention state.
    pub fn get_status(&mut self, process_id: ProcessId) -> RegistryResult<ProcessStatusView> {
        let process = self.get(process_id)?;
        self.status_view(process)
    }

    pub fn list(&mut self, project_id: Option<ProjectId>) -> RegistryResult<Vec<Process>> {
        self.refresh_exits()?;
        Ok(match project_id {
            Some(project_id) => self.store.list_processes(Some(project_id))?,
            None => self.store.list_active_profile_processes()?,
        })
    }

    /// List process status views, including agent state for every process.
    pub fn list_statuses(
        &mut self,
        project_id: Option<ProjectId>,
    ) -> RegistryResult<Vec<ProcessStatusView>> {
        let statuses = self
            .list(project_id)?
            .into_iter()
            .map(|process| self.status_view(process))
            .collect();
        self.arm_attention_deadline();
        statuses
    }

    /// Attach attention state to an already-loaded process record.
    pub fn status_view(&self, process: Process) -> RegistryResult<ProcessStatusView> {
        let tool_type = self.tool_type_for(&process)?;
        let mut agent_state = self
            .outputs
            .get(&process.id)
            .map(|output| output.attention.snapshot())
            .unwrap_or_else(|| AgentState::exited(tool_type, process.exited_at));
        if process.kind == ProcessKind::Agent {
            let waiting_on = self.waiting_reasons(process.id)?;
            let watched = self.process_is_watched(process.id)?;
            agent_state.refine_waiting(waiting_on);
            let last_agent_activity_at = agent_state.last_output_at.max(agent_state.last_input_at);
            let notification = self.store.observe_agent_attention_with_activity(
                process.id,
                agent_state.state,
                watched,
                agent_state.last_input_at.is_some(),
                last_agent_activity_at,
                now_millis(),
            )?;
            agent_state.refine_notifications(watched, notification.unread);
        }
        let events = self
            .outputs
            .get(&process.id)
            .map(|output| output.events.clone())
            .unwrap_or_default();
        let agent_session = if process.kind == ProcessKind::Agent {
            self.store.get_agent_session(process.id)?
        } else {
            None
        };
        let claimed_todos = self
            .store
            .claimed_todos_for_process(process.id, now_millis())?;
        Ok(ProcessStatusView {
            process,
            agent_state,
            events,
            agent_session_id: agent_session
                .as_ref()
                .and_then(|session| session.session_id.clone()),
            agent_launch_mode: agent_session.map(|session| session.launch_mode),
            claimed_todos,
        })
    }

    fn waiting_reasons(&self, process_id: ProcessId) -> RegistryResult<Vec<AgentWaitingReason>> {
        let rows = {
            let mut statement = self
                .store
                .connection()
                .prepare(
                    "SELECT timer.id,
                            timer.kind,
                            COALESCE(runtime.due_at, timer.max_wait_deadline, timer.created_at),
                            timer.created_at,
                            timer.paused,
                            runtime.paused_at,
                            timer.watch_list
                     FROM timers AS timer
                     LEFT JOIN timer_runtime AS runtime ON runtime.timer_id = timer.id
                     WHERE timer.fired = 0
                       AND (
                         timer.delivery_process_id = ?1
                         OR (
                           timer.kind IN ('idle_any', 'idle_all')
                           AND EXISTS (
                             SELECT 1 FROM actors AS actor
                             WHERE actor.id = timer.owner_actor
                               AND actor.process_id = ?1
                           )
                         )
                       )
                     ORDER BY timer.paused ASC,
                              COALESCE(runtime.due_at, timer.max_wait_deadline, timer.created_at),
                              timer.id",
                )
                .map_err(StoreError::from)?;
            let mapped = statement
                .query_map([process_id], |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, TimerKind>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, bool>(4)?,
                        row.get::<_, Option<i64>>(5)?,
                        row.get::<_, String>(6)?,
                    ))
                })
                .map_err(StoreError::from)?;
            let mut rows = Vec::new();
            for row in mapped {
                rows.push(row.map_err(StoreError::from)?);
            }
            rows
        };

        let now = now_millis();
        let mut reasons = Vec::with_capacity(rows.len());
        for (timer_id, kind, due_at, created_at, paused, paused_at, watch_list) in rows {
            let watch_process_ids: Vec<ProcessId> =
                serde_json::from_str(&watch_list).map_err(StoreError::from)?;
            let mut watch_processes = Vec::with_capacity(watch_process_ids.len());
            for watched_id in watch_process_ids {
                let process_name = self
                    .store
                    .get_process(watched_id)?
                    .map(|process| process.name)
                    .unwrap_or_else(|| format!("process #{watched_id}"));
                watch_processes.push(AgentWaitingProcess {
                    process_id: watched_id,
                    process_name,
                });
            }
            let clock = if paused {
                paused_at.unwrap_or(now)
            } else {
                now
            };
            reasons.push(AgentWaitingReason {
                timer_id,
                kind,
                due_at,
                max_wait_ms: due_at.saturating_sub(created_at).max(0),
                remaining_ms: due_at.saturating_sub(clock).max(0),
                paused,
                watch_processes,
            });
        }
        Ok(reasons)
    }

    fn process_is_watched(&self, process_id: ProcessId) -> RegistryResult<bool> {
        self.store
            .connection()
            .query_row(
                "SELECT EXISTS(
                    SELECT 1
                    FROM timers AS timer
                    WHERE timer.fired = 0
                      AND (
                        timer.delivery_process_id = ?1
                        OR (
                          timer.kind IN ('idle_any', 'idle_all')
                          AND EXISTS (
                            SELECT 1
                            FROM json_each(timer.watch_list) AS watched
                            WHERE CAST(watched.value AS INTEGER) = ?1
                          )
                        )
                      )
                 )",
                [process_id],
                |row| row.get(0),
            )
            .map_err(StoreError::from)
            .map_err(RegistryError::from)
    }

    /// Clear a durable unread completion marker after the agent is viewed.
    pub fn mark_agent_read(&mut self, process_id: ProcessId) -> RegistryResult<ProcessStatusView> {
        self.refresh_exits()?;
        let process = self.require(process_id)?;
        self.store.mark_agent_read(process_id)?;
        self.status_invalidations.invalidate();
        self.status_view(process)
    }

    pub fn start(&mut self, process_id: ProcessId) -> RegistryResult<Process> {
        self.refresh_exits()?;
        if self.running.contains_key(&process_id) {
            return Err(RegistryError::AlreadyRunning(process_id));
        }

        let mut process = self.require(process_id)?;
        if process.source == ProcessSource::Yml {
            if !is_process_trusted(&process) {
                return Err(RegistryError::Untrusted(process_id));
            }
            validate_process_working_dir(&self.store, &process).map_err(|message| {
                RegistryError::InvalidWorkingDirectory {
                    process_id,
                    message,
                }
            })?;
        }
        let base_command = process
            .command
            .as_deref()
            .filter(|command| !command.trim().is_empty())
            .ok_or(RegistryError::MissingCommand(process_id))?
            .to_owned();
        let previous_session = if process.kind == ProcessKind::Agent {
            self.store.get_agent_session(process_id)?
        } else {
            None
        };
        let agent_tool = process
            .agent_tool_id
            .map(|id| self.store.get_agent_tool(id))
            .transpose()?
            .flatten();
        let tool_type = self.tool_type_for(&process)?;
        let started_at = now_millis();
        let capture = tool_type.as_deref().and_then(|tool_type| {
            SessionCapture::new(tool_type, &process.working_dir, &process.env, started_at)
        });
        let needs_continue_check = process.exited_at.is_some()
            && previous_session
                .as_ref()
                .and_then(|session| session.session_id.as_ref())
                .is_none()
            && agent_tool
                .as_ref()
                .and_then(|tool| tool.continue_args.as_ref())
                .is_some();
        let cwd_has_session = needs_continue_check.then(|| {
            capture.as_ref().and_then(|capture| {
                if !capture.supports_continue_latest_fallback() {
                    return Some(false);
                }
                capture
                    .latest_existing()
                    .map(|session| session.is_some())
                    .map_err(|error| {
                        eprintln!(
                            "process {process_id}: could not inspect cwd agent sessions before launch: {error}"
                        );
                    })
                    .ok()
            })
        }).flatten();
        let launch = agent_start_command(
            &process,
            &base_command,
            agent_tool.as_ref(),
            previous_session.as_ref(),
            cwd_has_session,
        );
        let launch_mode = launch.mode;
        let launch_message = launch.mode_message();
        let command = launch.command;
        // Terminals created by Workman store the shell executable as their command. Keep that
        // interactive, including legacy/custom shell paths; terminal-kind fixtures with an actual
        // command still run through the resolved login shell like every other command.
        let interactive_terminal = process.kind == ProcessKind::Terminal
            && Path::new(&command).is_absolute()
            && Path::new(&command).is_file();

        process.status = ProcessStatus::Starting;
        process.pid = None;
        process.exit_code = None;
        process.exit_signal = None;
        process.exited_at = None;
        self.store.put_process(&process)?;

        let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        self.store
            .set_process_mcp_token(process.id, &token, now_millis())?;
        let user_environment = self.resolved_user_environment();
        let size = self
            .pty_sizes
            .get(&process_id)
            .copied()
            .unwrap_or(DEFAULT_PTY_SIZE);
        let mut options = PtySpawnOptions::new(process.id, token, command).with_size(size);
        for (key, value) in user_environment.pty_environment() {
            options = options.with_env(key, value);
        }
        if let Some(tool_type) = tool_type.as_deref() {
            options = options.with_tool_type(tool_type);
        }
        if !process.working_dir.is_empty() {
            options = options.with_working_dir(&process.working_dir);
        }
        for (key, value) in &process.env {
            options = options.with_env(key, value);
        }
        // Terminal capability and shell identity are Workman's spawn contract, not optional
        // process metadata. Apply them last so every PTY gets a consistent baseline.
        options = options
            .with_env("TERM", "xterm-256color")
            .with_env("COLORTERM", "truecolor")
            .with_env("SHELL", user_environment.active_shell());
        // Current agent TUIs gate standards-based modified-key negotiation on a known terminal
        // program identity. Workman's agent PTYs implement the CSI-u subset used by WezTerm and
        // advertised at runtime. Avoid the literal `kitty` identity: Bun takes that as permission
        // to run Kitty-specific probes beyond the keyboard protocol. Ordinary terminals and
        // commands keep their exact existing environment and input behavior.
        if process.kind == ProcessKind::Agent {
            options = options.with_env("TERM_PROGRAM", "WezTerm");
        }
        options = if interactive_terminal {
            options.with_login_shell(user_environment.active_shell())
        } else {
            options.with_login_shell_command(user_environment.active_shell())
        };
        if let Some(persistence) = &self.output_persistence {
            options = options
                .with_raw_buffer_capacity(persistence.capacity)
                .with_output_spill(output_path(persistence, process.id), persistence.capacity);
        }

        let mut hosted = match PtyProcess::spawn(options) {
            Ok(hosted) => hosted,
            Err(error) => {
                let _ = self.store.clear_process_mcp_token(process_id);
                process.status = ProcessStatus::Crashed;
                process.exited_at = Some(now_millis());
                self.store.put_process(&process)?;
                self.status_invalidations.invalidate();
                return Err(RegistryError::Pty {
                    process_id,
                    message: format!("{error:#}"),
                });
            }
        };

        process.status = ProcessStatus::Running;
        process.pid = Some(hosted.pid());
        if let Err(error) = self.store.put_process(&process) {
            let _ = self.store.clear_process_mcp_token(process_id);
            let _ = hosted.terminate(self.stop_grace);
            return Err(error.into());
        }
        if process.kind == ProcessKind::Agent {
            if let Err(error) =
                self.store
                    .set_agent_launch_mode(process.id, launch_mode, started_at)
            {
                let _ = self.store.clear_process_mcp_token(process_id);
                let _ = hosted.terminate(self.stop_grace);
                return Err(error.into());
            }
            if let Some(capture) = capture {
                self.agent_session_captures.insert(
                    process_id,
                    PendingSessionCapture {
                        capture,
                        root_pid: hosted.pid(),
                        last_checked_at: started_at,
                    },
                );
            }
        }
        let launch_event = (process.kind == ProcessKind::Agent).then(|| ProcessEvent {
            at: started_at,
            kind: "agent_launch".into(),
            message: launch_message.into(),
        });
        let attention = hosted.attention_tracker();
        self.connect_attention_invalidation(&attention);
        self.outputs.insert(
            process_id,
            ProcessOutput {
                raw: hosted.raw_output(),
                terminal: hosted.terminal_output(),
                attention,
                events: launch_event.into_iter().collect(),
            },
        );
        self.running.insert(process_id, hosted);
        if process.kind == ProcessKind::Agent {
            let started_at = now_millis();
            self.store.observe_agent_attention_with_activity(
                process_id,
                AttentionState::Working,
                false,
                false,
                Some(started_at),
                started_at,
            )?;
        }
        self.refresh_exits()?;
        self.require(process_id)
    }

    /// Approve exactly the YAML configuration hash that a reviewer observed.
    pub fn trust_yml_process(
        &mut self,
        process_id: ProcessId,
        expected_hash: &str,
    ) -> RegistryResult<Process> {
        self.trust_yml_process_inner(process_id, expected_hash, true)
    }

    /// Approve an in-app command edit without treating `auto_start` as an immediate start request.
    pub(crate) fn trust_yml_process_without_auto_start(
        &mut self,
        process_id: ProcessId,
        expected_hash: &str,
    ) -> RegistryResult<Process> {
        self.trust_yml_process_inner(process_id, expected_hash, false)
    }

    fn trust_yml_process_inner(
        &mut self,
        process_id: ProcessId,
        expected_hash: &str,
        start_if_auto: bool,
    ) -> RegistryResult<Process> {
        self.refresh_exits()?;
        let mut process = self.require(process_id)?;
        if process.source != ProcessSource::Yml {
            return Err(RegistryError::NotYmlBacked(process_id));
        }
        validate_process_working_dir(&self.store, &process).map_err(|message| {
            RegistryError::InvalidWorkingDirectory {
                process_id,
                message,
            }
        })?;
        let actual = trust_hash_for_process(&process);
        if expected_hash != actual {
            return Err(RegistryError::TrustHashMismatch(process_id));
        }
        process.trust_hash = Some(actual);
        self.store.put_process(&process)?;
        self.status_invalidations.invalidate();
        self.trust_snapshots
            .insert(process_id, TrustFields::from_process(&process));
        if start_if_auto && process.auto_start && !self.running.contains_key(&process_id) {
            self.start(process_id)
        } else {
            Ok(process)
        }
    }

    /// Build an approval payload and compare it with the last reviewed configuration.
    pub fn trust_review(&mut self, process_id: ProcessId) -> RegistryResult<TrustReview> {
        let process = self.get(process_id)?;
        if process.source != ProcessSource::Yml {
            return Err(RegistryError::NotYmlBacked(process_id));
        }
        let fields = TrustFields::from_process(&process);
        let previous = self.trust_snapshots.get(&process_id);
        let changes = trust_field_changes(previous, &fields);
        let trusted = is_process_trusted(&process);
        let expected_hash = trust_hash_for_process(&process);
        Ok(TrustReview {
            process_id,
            process_name: process.name,
            trusted,
            expected_hash,
            fields,
            changes,
        })
    }

    /// Gracefully stop a process and every durable descendant child-first.
    pub fn stop(&mut self, process_id: ProcessId) -> RegistryResult<Process> {
        let descendants = self.descendant_processes(process_id)?;
        let mut first_error = None;
        for descendant in descendants {
            if let Err(error) = self.stop_one(descendant.id)
                && first_error.is_none()
            {
                first_error = Some(error);
            }
        }
        let process = match self.stop_one(process_id) {
            Ok(process) => Some(process),
            Err(error) => {
                if first_error.is_none() {
                    first_error = Some(error);
                }
                None
            }
        };
        match first_error {
            Some(error) => Err(error),
            None => process.ok_or(RegistryError::NotFound(process_id)),
        }
    }

    fn stop_one(&mut self, process_id: ProcessId) -> RegistryResult<Process> {
        self.refresh_exits()?;
        let mut process = self.require(process_id)?;
        self.capture_agent_session_id(process_id)?;
        let Some(mut hosted) = self.running.remove(&process_id) else {
            process.status = ProcessStatus::Stopped;
            process.pid = None;
            self.store.put_process(&process)?;
            self.cleanup_user_stopped_process(&process)?;
            return Ok(process);
        };
        let _ = self.store.clear_process_mcp_token(process_id);

        match terminate_hosted_tree(&mut hosted, self.stop_grace, false) {
            Ok(status) => {
                self.capture_agent_session_id(process_id)?;
                apply_exit_info(&mut process, &status);
                process.status = ProcessStatus::Stopped;
                self.store.put_process(&process)?;
                self.cleanup_user_stopped_process(&process)?;
                Ok(process)
            }
            Err(error) => {
                self.capture_agent_session_id(process_id)?;
                process.status = ProcessStatus::Crashed;
                process.pid = None;
                process.exited_at = Some(now_millis());
                self.store.put_process(&process)?;
                self.status_invalidations.invalidate();
                Err(RegistryError::Pty {
                    process_id,
                    message: error.to_string(),
                })
            }
        }
    }

    /// Immediately kill a process and every durable descendant child-first.
    pub fn kill(&mut self, process_id: ProcessId) -> RegistryResult<Process> {
        let descendants = self.descendant_processes(process_id)?;
        let mut first_error = None;
        for descendant in descendants {
            if let Err(error) = self.kill_one(descendant.id)
                && first_error.is_none()
            {
                first_error = Some(error);
            }
        }
        let process = match self.kill_one(process_id) {
            Ok(process) => Some(process),
            Err(error) => {
                if first_error.is_none() {
                    first_error = Some(error);
                }
                None
            }
        };
        match first_error {
            Some(error) => Err(error),
            None => process.ok_or(RegistryError::NotFound(process_id)),
        }
    }

    fn kill_one(&mut self, process_id: ProcessId) -> RegistryResult<Process> {
        self.refresh_exits()?;
        let mut process = self.require(process_id)?;
        self.capture_agent_session_id(process_id)?;
        let Some(mut hosted) = self.running.remove(&process_id) else {
            process.status = ProcessStatus::Stopped;
            process.pid = None;
            self.store.put_process(&process)?;
            self.cleanup_user_stopped_process(&process)?;
            return Ok(process);
        };
        let _ = self.store.clear_process_mcp_token(process_id);

        match terminate_hosted_tree(&mut hosted, self.stop_grace, true) {
            Ok(status) => {
                apply_exit_info(&mut process, &status);
                process.status = ProcessStatus::Stopped;
                self.store.put_process(&process)?;
                self.cleanup_user_stopped_process(&process)?;
                Ok(process)
            }
            Err(error) => {
                process.status = ProcessStatus::Crashed;
                process.pid = None;
                process.exited_at = Some(now_millis());
                self.store.put_process(&process)?;
                self.status_invalidations.invalidate();
                Err(RegistryError::Pty {
                    process_id,
                    message: error.to_string(),
                })
            }
        }
    }

    pub fn restart(&mut self, process_id: ProcessId) -> RegistryResult<Process> {
        self.refresh_exits()?;
        self.require(process_id)?;
        if self.running.contains_key(&process_id) {
            self.stop(process_id)?;
        }
        self.start(process_id)
    }

    /// Terminate a process and every durable descendant, then remove their entries child-first.
    pub fn close(&mut self, process_id: ProcessId) -> RegistryResult<Process> {
        let descendants = self.descendant_processes(process_id)?;
        let mut first_error = None;
        for descendant in descendants {
            if let Err(error) = self.close_one(descendant.id)
                && first_error.is_none()
            {
                first_error = Some(error);
            }
        }
        let process = match self.close_one(process_id) {
            Ok(process) => Some(process),
            Err(error) => {
                if first_error.is_none() {
                    first_error = Some(error);
                }
                None
            }
        };
        match first_error {
            Some(error) => Err(error),
            None => process.ok_or(RegistryError::NotFound(process_id)),
        }
    }

    fn close_one(&mut self, process_id: ProcessId) -> RegistryResult<Process> {
        self.refresh_exits()?;
        let process = if self.running.contains_key(&process_id) {
            self.stop_one(process_id)?
        } else {
            self.require(process_id)?
        };
        self.cleanup_user_stopped_process(&process)?;
        cleanup_ephemeral_agent_home(&process).map_err(|error| RegistryError::Pty {
            process_id,
            message: format!("clean private agent config home: {error}"),
        })?;
        self.store.delete_process(process_id)?;
        self.outputs.remove(&process_id);
        self.pty_sizes.remove(&process_id);
        self.remove_output_file(process_id)?;
        self.trust_snapshots.remove(&process_id);
        self.selected.retain(|_, selected| *selected != process_id);
        Ok(process)
    }

    /// Return every durable descendant in safe child-first lifecycle order.
    pub fn descendant_processes(&mut self, process_id: ProcessId) -> RegistryResult<Vec<Process>> {
        self.refresh_exits()?;
        self.require(process_id)?;
        let processes = self.store.list_processes(None)?;
        let mut children = HashMap::<ProcessId, Vec<Process>>::new();
        for child in processes {
            if let Some(parent_id) = child.spawned_by_process_id {
                children.entry(parent_id).or_default().push(child);
            }
        }
        for siblings in children.values_mut() {
            siblings.sort_by_key(|child| child.id);
        }
        let mut visited = HashSet::new();
        let mut descendants = Vec::new();
        collect_descendants(process_id, &children, &mut visited, &mut descendants);
        Ok(descendants)
    }

    fn cleanup_user_stopped_process(&mut self, process: &Process) -> RegistryResult<()> {
        self.store
            .connection()
            .execute(
                "DELETE FROM timers
             WHERE delivery_process_id = ?1
                OR owner_actor IN (
                    SELECT id FROM actors WHERE process_id = ?1
                )",
                [process.id],
            )
            .map_err(StoreError::from)?;
        self.status_invalidations.invalidate();
        if process.kind != ProcessKind::Agent {
            return Ok(());
        }

        let last_agent_activity_at = self.outputs.get_mut(&process.id).map(|output| {
            output.attention.mark_exited();
            let state = output.attention.snapshot();
            state.last_output_at.max(state.last_input_at)
        });
        self.store.mark_agent_read(process.id)?;
        self.store.observe_agent_attention_with_activity(
            process.id,
            AttentionState::Exited,
            true,
            true,
            last_agent_activity_at.flatten(),
            now_millis(),
        )?;
        Ok(())
    }

    pub fn rename(&mut self, process_id: ProcessId, name: String) -> RegistryResult<Process> {
        validate_name(&name)?;
        let mut process = self.get(process_id)?;
        process.name = name;
        self.store.put_process(&process)?;
        self.status_invalidations.invalidate();
        Ok(process)
    }

    /// Focus a process for its project. Selection is UI state and intentionally ephemeral.
    pub fn select(&mut self, process_id: ProcessId) -> RegistryResult<Process> {
        let process = self.get(process_id)?;
        self.selected.insert(process.project_id, process.id);
        Ok(process)
    }

    pub fn selected_process(&self, project_id: ProjectId) -> Option<ProcessId> {
        self.selected.get(&project_id).copied()
    }

    /// Read raw output by absolute byte offset, clamped to the retained ring.
    pub fn raw_output(
        &mut self,
        process_id: ProcessId,
        offset: Option<u64>,
        max_bytes: usize,
    ) -> RegistryResult<RawOutputChunk> {
        self.refresh_exits()?;
        let process = self.require(process_id)?;
        let Some(output) = self.outputs.get(&process_id) else {
            return Ok(RawOutputChunk {
                data: Vec::new(),
                start_offset: 0,
                end_offset: 0,
                total_bytes: 0,
                truncated: false,
                status: process.status,
            });
        };

        let read = output.raw.read(offset, max_bytes);
        self.record_raw_output_profile(process_id, read.data.len(), 0);

        Ok(RawOutputChunk {
            data: read.data,
            start_offset: read.start_offset,
            end_offset: read.end_offset,
            total_bytes: read.total_bytes,
            truncated: read.truncated,
            status: process.status,
        })
    }

    /// Clone the process-local raw stream used to await output without polling the registry.
    pub fn raw_output_source(
        &mut self,
        process_id: ProcessId,
    ) -> RegistryResult<Option<RawOutput>> {
        self.refresh_exits()?;
        self.require(process_id)?;
        Ok(self
            .outputs
            .get(&process_id)
            .map(|output| output.raw.clone()))
    }

    fn record_raw_output_profile(
        &mut self,
        process_id: ProcessId,
        ring_copy_bytes: usize,
        response_copy_bytes: usize,
    ) {
        let Some(profiles) = &mut self.raw_output_profiles else {
            return;
        };
        let profile = profiles.entry(process_id).or_default();
        profile.calls = profile.calls.saturating_add(1);
        profile.ring_copy_bytes = profile
            .ring_copy_bytes
            .saturating_add(ring_copy_bytes as u64);
        profile.response_copy_bytes = profile
            .response_copy_bytes
            .saturating_add(response_copy_bytes as u64);
        profile.empty_calls = profile
            .empty_calls
            .saturating_add(u64::from(response_copy_bytes == 0));
        let elapsed = profile.window_started.elapsed();
        if elapsed < Duration::from_secs(1) {
            return;
        }
        eprintln!(
            "raw-output-profile process_id={process_id} window_ms={} calls={} ring_copy_bytes={} response_copy_bytes={} empty_calls={}",
            elapsed.as_millis(),
            profile.calls,
            profile.ring_copy_bytes,
            profile.response_copy_bytes,
            profile.empty_calls,
        );
        *profile = RawOutputProfile::default();
    }

    /// Read the daemon-rendered terminal buffer without ANSI escape sequences.
    pub fn rendered_output(
        &mut self,
        process_id: ProcessId,
    ) -> RegistryResult<RenderedProcessOutput> {
        self.refresh_exits()?;
        let process = self.require(process_id)?;
        let Some(output) = self.outputs.get(&process_id) else {
            return Ok(RenderedProcessOutput {
                text: String::new(),
                raw_end_offset: 0,
                status: process.status,
            });
        };
        Ok(RenderedProcessOutput {
            text: output.terminal.read_rows(0..usize::MAX).text(),
            raw_end_offset: output.raw.total_bytes_seen(),
            status: process.status,
        })
    }

    /// Return the server emulator's current DEC private focus-reporting mode (1004).
    pub fn terminal_focus_reporting(&mut self, process_id: ProcessId) -> RegistryResult<bool> {
        self.refresh_exits()?;
        self.require(process_id)?;
        Ok(self
            .outputs
            .get(&process_id)
            .is_some_and(|output| output.terminal.is_focus_reporting()))
    }

    /// Return the keyboard protocol negotiated by the application running in this PTY.
    pub fn terminal_keyboard_protocol(
        &mut self,
        process_id: ProcessId,
    ) -> RegistryResult<TerminalKeyboardProtocol> {
        self.refresh_exits()?;
        self.require(process_id)?;
        Ok(self
            .outputs
            .get(&process_id)
            .map(|output| output.terminal.keyboard_protocol())
            .unwrap_or_default())
    }

    /// Return a currently rendered choice/permission dialog, if recognized.
    pub fn pending_dialog(
        &mut self,
        process_id: ProcessId,
    ) -> RegistryResult<Option<PendingDialog>> {
        self.refresh_exits()?;
        self.require(process_id)?;
        let Some(output) = self.outputs.get(&process_id) else {
            return Ok(None);
        };
        let rendered = output.terminal.read_rows(0..usize::MAX).text();
        let status = output.attention.snapshot();
        Ok(pending_dialog(&rendered, status.classification.as_deref()))
    }

    /// Acknowledge a narrowly known first-run trust dialog with Enter.
    pub fn acknowledge_known_dialog(
        &mut self,
        process_id: ProcessId,
    ) -> RegistryResult<Option<PendingDialog>> {
        let Some(dialog) = self.pending_dialog(process_id)? else {
            return Ok(None);
        };
        if !dialog.known_first_run {
            return Ok(None);
        }
        self.send_input(process_id, b"\r")?;
        let event = ProcessEvent {
            at: now_millis(),
            kind: "dialog_auto_acknowledged".into(),
            message: format!(
                "workman auto-acknowledged known first-run {} with Enter",
                dialog.classification
            ),
        };
        if let Some(output) = self.outputs.get_mut(&process_id) {
            output.events.push(event.clone());
        }
        self.status_invalidations.invalidate();
        eprintln!("process {process_id}: {}", event.message);
        Ok(Some(dialog))
    }

    /// Read a clamped range of physical terminal rows across scrollback and viewport.
    pub fn rendered_output_range(
        &mut self,
        process_id: ProcessId,
        range: std::ops::Range<usize>,
    ) -> RegistryResult<RenderedOutputRange> {
        self.refresh_exits()?;
        let process = self.require(process_id)?;
        let Some(output) = self.outputs.get(&process_id) else {
            return Ok(RenderedOutputRange {
                text: String::new(),
                start: 0,
                end: 0,
                total_rows: 0,
                viewport_start: 0,
                cursor_row: 0,
                alternate_screen: false,
                raw_end_offset: 0,
                status: process.status,
            });
        };
        let rows = output.terminal.read_rows(range);
        Ok(RenderedOutputRange {
            text: rows.text(),
            start: rows.start,
            end: rows.end,
            total_rows: rows.total_rows,
            viewport_start: rows.viewport_start,
            cursor_row: rows.cursor.row,
            alternate_screen: rows.alternate_screen,
            raw_end_offset: output.raw.total_bytes_seen(),
            status: process.status,
        })
    }

    /// Search escape-free terminal rows using an ASCII-case-insensitive substring match.
    pub fn search_rendered_output(
        &mut self,
        process_id: ProcessId,
        pattern: &str,
        max_matches: usize,
    ) -> RegistryResult<Vec<RenderedOutputSearchMatch>> {
        self.refresh_exits()?;
        self.require(process_id)?;
        let Some(output) = self.outputs.get(&process_id) else {
            return Ok(Vec::new());
        };
        if pattern.is_empty() || max_matches == 0 {
            return Ok(Vec::new());
        }

        let rows = output.terminal.read_rows(0..usize::MAX);
        let mut matches = Vec::new();
        for row in rows.rows {
            for (byte_start, byte_end) in ascii_case_insensitive_matches(
                &row.text,
                pattern,
                max_matches.saturating_sub(matches.len()),
            ) {
                matches.push(RenderedOutputSearchMatch {
                    row: row.index + 1,
                    byte_start,
                    byte_end,
                    text: row.text.clone(),
                });
                if matches.len() == max_matches {
                    return Ok(matches);
                }
            }
        }
        Ok(matches)
    }

    /// Search retained raw output while preserving absolute stream offsets.
    pub fn search_raw_output(
        &mut self,
        process_id: ProcessId,
        pattern: &str,
        max_matches: usize,
    ) -> RegistryResult<Vec<RawOutputSearchMatch>> {
        self.refresh_exits()?;
        self.require(process_id)?;
        let Some(output) = self.outputs.get(&process_id) else {
            return Ok(Vec::new());
        };
        if pattern.is_empty() || max_matches == 0 {
            return Ok(Vec::new());
        }

        let bytes = output.raw.snapshot();
        let stream_start = output
            .raw
            .total_bytes_seen()
            .saturating_sub(u64::try_from(bytes.len()).unwrap_or(u64::MAX));
        let mut matches = Vec::new();
        let mut retained_offset = 0_usize;
        for (line_index, raw_line) in bytes.split_inclusive(|byte| *byte == b'\n').enumerate() {
            let line = String::from_utf8_lossy(raw_line);
            let text = line.trim_end_matches(['\r', '\n']);
            for (byte_start, byte_end) in ascii_case_insensitive_matches(
                text,
                pattern,
                max_matches.saturating_sub(matches.len()),
            ) {
                matches.push(RawOutputSearchMatch {
                    line: line_index + 1,
                    stream_offset: stream_start
                        .saturating_add((retained_offset + byte_start) as u64),
                    byte_start,
                    byte_end,
                    text: text.to_owned(),
                });
                if matches.len() == max_matches {
                    return Ok(matches);
                }
            }
            retained_offset = retained_offset.saturating_add(raw_line.len());
        }
        Ok(matches)
    }

    /// Clear retained raw and rendered output without stopping or detaching the process.
    pub fn clear_output(&mut self, process_id: ProcessId) -> RegistryResult<Process> {
        self.refresh_exits()?;
        let process = self.require(process_id)?;
        if let Some(output) = self.outputs.get(&process_id) {
            output.raw.clear();
            output.terminal.clear();
        }
        if let Some(hosted) = self.running.get(&process_id) {
            hosted
                .clear_output_spill()
                .map_err(|error| output_error(process_id, error))?;
        } else {
            self.remove_output_file(process_id)?;
        }
        Ok(process)
    }

    /// Send raw bytes to a live process's PTY.
    ///
    /// Terminal protocol replies, focus reports, navigation keys, and ordinary
    /// draft edits are attention-neutral. Only a line submission may
    /// optimistically start a turn before the agent produces its own output.
    pub fn send_input(&mut self, process_id: ProcessId, data: &[u8]) -> RegistryResult<Process> {
        self.refresh_exits()?;
        let submits_prompt = data.iter().any(|byte| matches!(byte, b'\r' | b'\n'));
        if !submits_prompt
            && let Some(output) = self.outputs.get(&process_id)
            && matches!(
                output.attention.snapshot().state,
                AttentionState::Idle | AttentionState::Waiting
            )
        {
            output.attention.suppress_ui_activity();
        }
        let hosted = self
            .running
            .get_mut(&process_id)
            .ok_or(RegistryError::NotRunning(process_id))?;
        hosted.write_all(data).map_err(|error| RegistryError::Pty {
            process_id,
            message: error.to_string(),
        })?;
        if submits_prompt && let Some(output) = self.outputs.get(&process_id) {
            output.attention.observe_input();
        }
        self.require(process_id)
    }

    /// Queue content and Enter as distinct, ordered PTY writes.
    ///
    /// Raw-mode TUIs use burst boundaries to distinguish pasted content from
    /// key presses. Keeping CR out of the content write prevents a short prompt
    /// from absorbing Enter into a bracketed-paste/composer update.
    pub fn submit_input(
        &mut self,
        process_id: ProcessId,
        content: &[u8],
    ) -> RegistryResult<Process> {
        self.submit_input_with_delay(process_id, content, SUBMIT_KEY_DELAY)
    }

    fn submit_input_with_delay(
        &mut self,
        process_id: ProcessId,
        content: &[u8],
        key_delay: Duration,
    ) -> RegistryResult<Process> {
        self.refresh_exits()?;
        let process = self.require(process_id)?;
        let hosted = self
            .running
            .get(&process_id)
            .ok_or(RegistryError::NotRunning(process_id))?;
        let queued = if process.kind == ProcessKind::Agent {
            hosted.submit_input_verified(
                content,
                key_delay,
                PtySubmissionVerification {
                    timeout: SUBMIT_VERIFY_TIMEOUT,
                    max_attempts: SUBMIT_MAX_ATTEMPTS,
                },
            )
        } else {
            hosted.submit_input(content, key_delay)
        };
        queued.map_err(|error| RegistryError::Pty {
            process_id,
            message: error.to_string(),
        })?;
        if let Some(output) = self.outputs.get(&process_id) {
            output.attention.observe_input();
        }
        self.require(process_id)
    }

    /// Resize a live process's PTY and server-side terminal emulator together.
    pub fn resize(
        &mut self,
        process_id: ProcessId,
        rows: u16,
        cols: u16,
        pixel_width: u16,
        pixel_height: u16,
    ) -> RegistryResult<Process> {
        self.refresh_exits()?;
        self.require(process_id)?;
        let size = PtySize {
            rows: rows.max(1),
            cols: cols.max(1),
            pixel_width,
            pixel_height,
        };
        self.pty_sizes.insert(process_id, size);
        if let Some(output) = self.outputs.get(&process_id)
            && matches!(
                output.attention.snapshot().state,
                AttentionState::Idle | AttentionState::Waiting
            )
        {
            output.attention.suppress_ui_activity();
        }
        if let Some(hosted) = self.running.get(&process_id) {
            hosted.resize(size).map_err(|error| RegistryError::Pty {
                process_id,
                message: error.to_string(),
            })?;
        }
        self.require(process_id)
    }

    pub fn start_all_commands(&mut self, project_id: ProjectId) -> BulkProcessResult {
        self.bulk_commands(project_id, BulkAction::Start)
    }

    pub fn stop_all_commands(&mut self, project_id: ProjectId) -> BulkProcessResult {
        self.bulk_commands(project_id, BulkAction::Stop)
    }

    pub fn restart_all_commands(&mut self, project_id: ProjectId) -> BulkProcessResult {
        self.bulk_commands(project_id, BulkAction::Restart)
    }

    fn bulk_commands(&mut self, project_id: ProjectId, action: BulkAction) -> BulkProcessResult {
        let mut result = BulkProcessResult::default();
        let processes = match self.list(Some(project_id)) {
            Ok(processes) => processes,
            Err(error) => {
                result.failures.push(BulkFailure {
                    process_id: 0,
                    code: error.code().into(),
                    message: error.to_string(),
                });
                return result;
            }
        };

        for process in processes
            .into_iter()
            .filter(|process| process.kind == ProcessKind::Command)
        {
            let process_id = process.id;
            let operation = match action {
                BulkAction::Start if process.status == ProcessStatus::Running => Ok(process),
                BulkAction::Start => self.start(process.id),
                BulkAction::Stop if process.status != ProcessStatus::Running => Ok(process),
                BulkAction::Stop => self.stop(process.id),
                BulkAction::Restart => self.restart(process.id),
            };
            match operation {
                Ok(process) => result.processes.push(process),
                Err(error) => result.failures.push(BulkFailure {
                    process_id,
                    code: error.code().into(),
                    message: error.to_string(),
                }),
            }
        }
        result
    }

    fn refresh_exits(&mut self) -> RegistryResult<()> {
        self.drain_submission_events();
        self.refresh_agent_session_ids(false)?;
        let process_ids = self.running.keys().copied().collect::<Vec<_>>();
        for process_id in process_ids {
            let status = self
                .running
                .get_mut(&process_id)
                .expect("running process ID disappeared")
                .try_wait()
                .map_err(|error| RegistryError::Pty {
                    process_id,
                    message: error.to_string(),
                })?;
            let Some(status) = status else { continue };

            self.capture_agent_session_id(process_id)?;
            self.running.remove(&process_id);
            let _ = self.store.clear_process_mcp_token(process_id);
            let mut process = self.require(process_id)?;
            apply_exit_info(&mut process, &status);
            process.status = if status.success() {
                ProcessStatus::Exited
            } else {
                ProcessStatus::Crashed
            };
            self.store.put_process(&process)?;
            self.status_invalidations.invalidate();
        }
        Ok(())
    }

    fn refresh_agent_session_ids(&mut self, force: bool) -> RegistryResult<()> {
        let now = now_millis();
        let process_ids = self
            .agent_session_captures
            .iter()
            .filter(|(_, pending)| force || now.saturating_sub(pending.last_checked_at) >= 1_000)
            .map(|(process_id, _)| *process_id)
            .collect::<Vec<_>>();
        for process_id in process_ids {
            self.capture_agent_session_id(process_id)?;
        }
        Ok(())
    }

    fn capture_agent_session_id(&mut self, process_id: ProcessId) -> RegistryResult<()> {
        let now = now_millis();
        let Some(pending) = self.agent_session_captures.get_mut(&process_id) else {
            return Ok(());
        };
        pending.last_checked_at = now;
        match pending.capture.discover_for_process(pending.root_pid) {
            Ok(Some(session_id)) => {
                if self
                    .store
                    .set_agent_session_id(process_id, &session_id, now)?
                {
                    self.agent_session_captures.remove(&process_id);
                    self.status_invalidations.invalidate();
                } else {
                    eprintln!(
                        "process {process_id}: agent session {session_id} is already attributed to another process"
                    );
                }
            }
            Ok(None) => {}
            Err(error) => {
                eprintln!("process {process_id}: could not discover agent session: {error}");
            }
        }
        Ok(())
    }

    fn drain_submission_events(&mut self) {
        let events = self
            .running
            .iter()
            .flat_map(|(process_id, hosted)| {
                hosted
                    .submission_events()
                    .into_iter()
                    .map(|event| (*process_id, event))
            })
            .collect::<Vec<_>>();
        let changed = !events.is_empty();
        for (process_id, event) in events {
            let (kind, message) = match event.kind {
                PtySubmissionEventKind::Retried => (
                    "submit_retry",
                    format!(
                        "No turn-start output after {} ms; retried Enter ({}/{})",
                        event.timeout.as_millis(),
                        event.attempt,
                        event.max_attempts
                    ),
                ),
                PtySubmissionEventKind::Unverified => (
                    "submit_unverified",
                    format!(
                        "Could not verify turn start after {} Enter attempts",
                        event.max_attempts
                    ),
                ),
            };
            let process_event = ProcessEvent {
                at: now_millis(),
                kind: kind.into(),
                message,
            };
            if let Some(output) = self.outputs.get_mut(&process_id) {
                output.events.push(process_event.clone());
            }
            eprintln!("process {process_id}: {}", process_event.message);
        }
        if changed {
            self.status_invalidations.invalidate();
        }
    }

    fn reconcile_stale_processes(&mut self) -> RegistryResult<()> {
        for mut process in self.store.list_processes(None)? {
            let _ = self.store.clear_process_mcp_token(process.id);
            if matches!(
                process.status,
                ProcessStatus::Starting | ProcessStatus::Running
            ) {
                process.status = ProcessStatus::Crashed;
                process.pid = None;
                process.exited_at = Some(now_millis());
                self.store.put_process(&process)?;
            }
        }
        Ok(())
    }

    fn reload_persisted_outputs(&mut self) -> RegistryResult<()> {
        let Some(persistence) = self.output_persistence.clone() else {
            return Ok(());
        };
        std::fs::create_dir_all(&persistence.directory).map_err(|error| output_error(0, error))?;
        for process in self.store.list_processes(None)? {
            let bytes = match read_output_tail(
                &output_path(&persistence, process.id),
                persistence.capacity,
            ) {
                Ok(Some(bytes)) => bytes,
                Ok(None) => continue,
                Err(error) => return Err(output_error(process.id, error)),
            };
            let raw = RawOutput::from_replay(persistence.capacity, &bytes);
            let terminal = TerminalOutput::from_replay(
                DEFAULT_PTY_SIZE.rows,
                DEFAULT_PTY_SIZE.cols,
                DEFAULT_SCROLLBACK_LINES,
                &bytes,
            );
            let attention = AttentionTracker::new(self.tool_type_for(&process)?);
            attention.mark_exited();
            self.outputs.insert(
                process.id,
                ProcessOutput {
                    raw,
                    terminal,
                    attention,
                    events: Vec::new(),
                },
            );
        }
        Ok(())
    }

    fn remove_output_file(&self, process_id: ProcessId) -> RegistryResult<()> {
        let Some(persistence) = &self.output_persistence else {
            return Ok(());
        };
        let path = output_path(persistence, process_id);
        match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(output_error(process_id, error)),
        }
    }

    fn require(&self, process_id: ProcessId) -> RegistryResult<Process> {
        self.store
            .get_process(process_id)?
            .ok_or(RegistryError::NotFound(process_id))
    }

    fn connect_attention_invalidation(&self, attention: &AttentionTracker) {
        let invalidations = self.status_invalidations.clone();
        attention.set_invalidation_callback(move |next_transition_at| {
            invalidations.invalidate();
            if let Some(at) = next_transition_at {
                invalidations.arm_deadline(at);
            }
        });
    }

    fn arm_attention_deadline(&self) {
        let now = now_millis();
        if let Some(at) = self
            .outputs
            .values()
            .filter_map(|output| output.attention.next_transition_at(now))
            .min()
        {
            self.status_invalidations.arm_deadline(at);
        }
    }

    fn tool_type_for(&self, process: &Process) -> RegistryResult<Option<String>> {
        if let Some(agent_tool_id) = process.agent_tool_id
            && let Some(tool) = self.store.get_agent_tool(agent_tool_id)?
        {
            return Ok(Some(tool.tool_type));
        }

        let command_is_claude = process.command.as_deref().is_some_and(|command| {
            command
                .split(|character: char| {
                    !character.is_ascii_alphanumeric() && character != '_' && character != '-'
                })
                .any(|word| {
                    word.eq_ignore_ascii_case("claude") || word.eq_ignore_ascii_case("claude-code")
                })
        });
        if process.kind == ProcessKind::Agent && command_is_claude {
            Ok(Some("claude_code".into()))
        } else {
            Ok(Some(process.kind.as_str().into()))
        }
    }
}

fn cleanup_ephemeral_agent_home(process: &Process) -> io::Result<()> {
    let Some(value) = process.env.get(WORKMAN_EPHEMERAL_AGENT_HOME_ENV) else {
        return Ok(());
    };
    let path = Path::new(value);
    let temp = std::env::temp_dir();
    let safe_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with("workman-") && name.contains("-mcp."));
    if path.parent() != Some(temp.as_path()) || !safe_name {
        return Err(io::Error::other(format!(
            "refusing to remove unexpected path {}",
            path.display()
        )));
    }
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(io::Error::other(format!(
            "refusing to remove non-directory {}",
            path.display()
        )));
    }
    fs::remove_dir_all(path)
}

/// Resolve the configured spill cap, falling back to the bounded 8 MiB default.
pub fn output_spill_capacity_from_env() -> usize {
    std::env::var(WORKMAN_OUTPUT_CAPACITY_ENV)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(DEFAULT_OUTPUT_SPILL_CAPACITY)
}

fn profile_enabled() -> bool {
    std::env::var(WORKMAN_PTY_PROFILE_ENV).is_ok_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

fn output_path(persistence: &OutputPersistence, process_id: ProcessId) -> PathBuf {
    persistence.directory.join(format!("{process_id}.raw"))
}

fn read_output_tail(path: &Path, capacity: usize) -> io::Result<Option<Vec<u8>>> {
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let length = file.metadata()?.len();
    let retained = length.min(capacity as u64);
    file.seek(SeekFrom::Start(length.saturating_sub(retained)))?;
    let mut bytes = Vec::with_capacity(retained as usize);
    file.read_to_end(&mut bytes)?;
    Ok(Some(bytes))
}

fn output_error(process_id: ProcessId, error: io::Error) -> RegistryError {
    RegistryError::OutputPersistence {
        process_id,
        message: error.to_string(),
    }
}

struct ProcessOutput {
    raw: RawOutput,
    terminal: TerminalOutput,
    attention: AttentionTracker,
    events: Vec<ProcessEvent>,
}

impl Drop for ProcessRegistry {
    fn drop(&mut self) {
        let process_ids = self.running.keys().copied().collect::<Vec<_>>();
        for process_id in process_ids {
            let _ = self.capture_agent_session_id(process_id);
        }
        for (process_id, mut hosted) in self.running.drain() {
            let _ = self.store.clear_process_mcp_token(process_id);
            let status = terminate_hosted_tree(&mut hosted, self.stop_grace, false).ok();
            if let Some(pending) = self.agent_session_captures.remove(&process_id)
                && let Ok(Some(session_id)) = pending.capture.discover()
            {
                let _ = self
                    .store
                    .set_agent_session_id(process_id, &session_id, now_millis());
            }
            if let Ok(Some(mut process)) = self.store.get_process(process_id) {
                if let Some(status) = status {
                    apply_exit_info(&mut process, &status);
                } else {
                    process.pid = None;
                    process.exited_at = Some(now_millis());
                }
                process.status = ProcessStatus::Stopped;
                let _ = self.store.put_process(&process);
            }
        }
    }
}

#[derive(Debug)]
struct AgentStartCommand {
    command: String,
    mode: AgentLaunchMode,
    missing_session_fallback: bool,
}

impl AgentStartCommand {
    const fn mode_message(&self) -> &'static str {
        match self.mode {
            AgentLaunchMode::Fresh if self.missing_session_fallback => {
                "Started a fresh agent conversation because no captured session ID was available"
            }
            AgentLaunchMode::Fresh => "Started a fresh agent conversation",
            AgentLaunchMode::ContinuedLatest => {
                "Continued the latest agent conversation for this working directory"
            }
            AgentLaunchMode::ResumedSession => "Resumed the captured agent conversation",
        }
    }
}

fn agent_start_command(
    process: &Process,
    base_command: &str,
    tool: Option<&AgentTool>,
    session: Option<&workman_core::AgentSession>,
    cwd_has_session: Option<bool>,
) -> AgentStartCommand {
    if process.kind != ProcessKind::Agent || process.exited_at.is_none() {
        return AgentStartCommand {
            command: base_command.to_owned(),
            mode: AgentLaunchMode::Fresh,
            missing_session_fallback: false,
        };
    }
    if let (Some(session_id), Some(resume_args)) = (
        session.and_then(|session| session.session_id.as_deref()),
        tool.and_then(|tool| tool.resume_args.as_deref()),
    ) {
        return AgentStartCommand {
            command: append_shell_args(
                base_command,
                &resume_args.replace("{session_id}", &shell_quote(session_id)),
            ),
            mode: AgentLaunchMode::ResumedSession,
            missing_session_fallback: false,
        };
    }
    if cwd_has_session == Some(true)
        && let Some(continue_args) = tool.and_then(|tool| tool.continue_args.as_deref())
    {
        return AgentStartCommand {
            command: append_shell_args(base_command, continue_args),
            mode: AgentLaunchMode::ContinuedLatest,
            missing_session_fallback: false,
        };
    }
    AgentStartCommand {
        command: base_command.to_owned(),
        mode: AgentLaunchMode::Fresh,
        missing_session_fallback: true,
    }
}

fn append_shell_args(command: &str, args: &str) -> String {
    format!("{} {}", command.trim_end(), args.trim())
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[derive(Clone, Copy)]
enum BulkAction {
    Start,
    Stop,
    Restart,
}

fn trust_field_changes(
    previous: Option<&TrustFields>,
    current: &TrustFields,
) -> Vec<TrustFieldChange> {
    let mut changes = Vec::new();
    macro_rules! field_change {
        ($field:ident) => {{
            let previous_value = previous.map(|fields| {
                serde_json::to_value(&fields.$field).expect("trust fields always serialize")
            });
            let current_value =
                serde_json::to_value(&current.$field).expect("trust fields always serialize");
            if previous_value.as_ref() != Some(&current_value) {
                changes.push(TrustFieldChange {
                    field: stringify!($field).to_owned(),
                    previous: previous_value,
                    current: current_value,
                });
            }
        }};
    }
    field_change!(command);
    field_change!(working_dir);
    field_change!(env);
    field_change!(auto_start);
    field_change!(auto_restart);
    field_change!(restart_when_changed);
    changes
}

fn validate_name(name: &str) -> RegistryResult<()> {
    if name.trim().is_empty() {
        Err(RegistryError::InvalidName)
    } else {
        Ok(())
    }
}

fn apply_exit_info(process: &mut Process, status: &ExitStatus) {
    process.pid = None;
    process.exit_code = Some(i32::try_from(status.exit_code()).unwrap_or(i32::MAX));
    process.exit_signal = status.signal().and_then(signal_number);
    process.exited_at = Some(now_millis());
}

fn terminate_hosted_tree(
    hosted: &mut PtyProcess,
    grace_period: Duration,
    immediate: bool,
) -> Result<ExitStatus, String> {
    let tree = TrackedProcessTree::capture(hosted.pid());
    let termination = if immediate {
        hosted.kill()
    } else {
        hosted.terminate(grace_period)
    }
    .map_err(|error| error.to_string());
    let cleanup = tree.kill_remaining();

    match (termination, cleanup) {
        (Ok(status), Ok(())) => Ok(status),
        (Err(error), Ok(())) => Err(error),
        (Ok(_), Err(error)) => Err(error),
        (Err(termination), Err(cleanup)) => Err(format!("{termination}; {cleanup}")),
    }
}

fn collect_descendants(
    process_id: ProcessId,
    children: &HashMap<ProcessId, Vec<Process>>,
    visited: &mut HashSet<ProcessId>,
    descendants: &mut Vec<Process>,
) {
    if !visited.insert(process_id) {
        return;
    }
    let Some(direct_children) = children.get(&process_id) else {
        return;
    };
    for child in direct_children {
        if visited.contains(&child.id) {
            continue;
        }
        collect_descendants(child.id, children, visited, descendants);
        descendants.push(child.clone());
    }
}

fn signal_number(signal: &str) -> Option<i32> {
    if let Some(number) = signal
        .rsplit(|character: char| !character.is_ascii_digit())
        .find(|part| !part.is_empty())
        .and_then(|part| part.parse().ok())
    {
        return Some(number);
    }

    let lowercase = signal.to_ascii_lowercase();
    [
        ("hangup", 1),
        ("interrupt", 2),
        ("quit", 3),
        ("illegal instruction", 4),
        ("aborted", 6),
        ("killed", 9),
        ("segmentation fault", 11),
        ("broken pipe", 13),
        ("terminated", 15),
    ]
    .into_iter()
    .find_map(|(name, number)| lowercase.contains(name).then_some(number))
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(i64::MAX)
}

fn ascii_case_insensitive_matches(
    haystack: &str,
    needle: &str,
    limit: usize,
) -> Vec<(usize, usize)> {
    let haystack = haystack.to_ascii_lowercase();
    let needle = needle.to_ascii_lowercase();
    haystack
        .match_indices(&needle)
        .map(|(start, value)| (start, start + value.len()))
        .take(limit)
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::os::unix::fs::PermissionsExt;
    use std::{thread, time::Instant};

    use workman_core::{
        AgentSession, AgentTool, ProcessSource, Project, attention::AttentionState,
    };

    use super::*;

    fn output_test_process(project_path: &str) -> Process {
        Process {
            id: 31,
            project_id: 1,
            kind: ProcessKind::Terminal,
            name: "persistent-terminal".into(),
            command: Some(
                "printf '\x1b[32mPERSISTED-OUTPUT-313\x1b[0m\nsecond-line\n'; sleep 30".into(),
            ),
            working_dir: project_path.into(),
            env: BTreeMap::new(),
            auto_start: false,
            auto_restart: false,
            restart_when_changed: Vec::new(),
            source: ProcessSource::Local,
            trust_hash: None,
            status: ProcessStatus::Stopped,
            pid: None,
            exit_code: None,
            exit_signal: None,
            exited_at: None,
            agent_tool_id: None,
            spawned_by_process_id: None,
            sort_order: 0,
        }
    }

    #[test]
    fn dead_agent_start_prefers_exact_then_latest_then_fresh_without_losing_flags() {
        let mut process = output_test_process("/tmp/repo");
        process.kind = ProcessKind::Agent;
        process.exited_at = Some(1);
        let tool = AgentTool {
            id: 7,
            name: "Codex".into(),
            command: "codex --model gpt-5 --dangerously-bypass-approvals-and-sandbox".into(),
            tool_type: "codex".into(),
            enabled: true,
            source: workman_core::AgentToolSource::Config,
            resume_args: Some("resume {session_id}".into()),
            continue_args: Some("resume --last".into()),
        };
        let session = AgentSession {
            process_id: process.id,
            session_id: Some("abc'def".into()),
            launch_mode: AgentLaunchMode::Fresh,
            launched_at: 1,
            captured_at: Some(2),
        };

        let exact = agent_start_command(
            &process,
            &tool.command,
            Some(&tool),
            Some(&session),
            Some(true),
        );
        assert_eq!(exact.mode, AgentLaunchMode::ResumedSession);
        assert_eq!(
            exact.command,
            "codex --model gpt-5 --dangerously-bypass-approvals-and-sandbox resume 'abc'\\''def'"
        );

        let latest = agent_start_command(&process, &tool.command, Some(&tool), None, Some(true));
        assert_eq!(latest.mode, AgentLaunchMode::ContinuedLatest);
        assert_eq!(
            latest.command,
            "codex --model gpt-5 --dangerously-bypass-approvals-and-sandbox resume --last"
        );

        let custom = AgentTool {
            resume_args: None,
            continue_args: None,
            ..tool.clone()
        };
        let fresh = agent_start_command(
            &process,
            &custom.command,
            Some(&custom),
            Some(&session),
            Some(true),
        );
        assert_eq!(fresh.mode, AgentLaunchMode::Fresh);
        assert_eq!(fresh.command, custom.command);
    }

    #[test]
    fn first_agent_start_is_fresh_even_when_the_preset_can_continue() {
        let mut process = output_test_process("/tmp/repo");
        process.kind = ProcessKind::Agent;
        let tool = AgentTool {
            id: 7,
            name: "Claude".into(),
            command: "claude --dangerously-skip-permissions".into(),
            tool_type: "claude_code".into(),
            enabled: true,
            source: workman_core::AgentToolSource::Config,
            resume_args: Some("--resume {session_id}".into()),
            continue_args: Some("--continue".into()),
        };
        let launch = agent_start_command(&process, &tool.command, Some(&tool), None, Some(true));
        assert_eq!(launch.mode, AgentLaunchMode::Fresh);
        assert_eq!(launch.command, tool.command);
    }

    #[test]
    fn dead_agent_without_a_cwd_session_falls_back_to_fresh() {
        let mut process = output_test_process("/tmp/repo");
        process.kind = ProcessKind::Agent;
        process.exited_at = Some(1);
        let tool = AgentTool {
            id: 7,
            name: "Claude".into(),
            command: "claude --dangerously-skip-permissions".into(),
            tool_type: "claude_code".into(),
            enabled: true,
            source: workman_core::AgentToolSource::Config,
            resume_args: Some("--resume {session_id}".into()),
            continue_args: Some("--continue".into()),
        };
        let launch = agent_start_command(&process, &tool.command, Some(&tool), None, Some(false));
        assert_eq!(launch.mode, AgentLaunchMode::Fresh);
        assert_eq!(launch.command, tool.command);
    }

    #[test]
    fn grok_restart_uses_an_exact_session_and_never_guesses_continue_latest() {
        let mut process = output_test_process("/tmp/repo");
        process.kind = ProcessKind::Agent;
        process.exited_at = Some(1);
        let tool = AgentTool {
            id: 8,
            name: "Grok".into(),
            command: "grok --always-approve".into(),
            tool_type: "grok".into(),
            enabled: true,
            source: workman_core::AgentToolSource::Local,
            resume_args: Some("--resume {session_id}".into()),
            continue_args: Some("--continue".into()),
        };
        let session = AgentSession {
            process_id: process.id,
            session_id: Some("625235f8-8eca-4295-9aef-2ce34e19f512".into()),
            launch_mode: AgentLaunchMode::Fresh,
            launched_at: 1,
            captured_at: Some(2),
        };

        let exact = agent_start_command(
            &process,
            &tool.command,
            Some(&tool),
            Some(&session),
            Some(true),
        );
        assert_eq!(exact.mode, AgentLaunchMode::ResumedSession);
        assert_eq!(
            exact.command,
            "grok --always-approve --resume '625235f8-8eca-4295-9aef-2ce34e19f512'"
        );

        let fresh = agent_start_command(&process, &tool.command, Some(&tool), None, Some(false));
        assert_eq!(fresh.mode, AgentLaunchMode::Fresh);
        assert_eq!(fresh.command, tool.command);
    }

    #[test]
    fn closing_a_grok_process_removes_only_its_private_launch_home() {
        let source = tempfile::tempdir().unwrap();
        let auth = source.path().join("auth.json");
        fs::write(&auth, "fixture-auth").unwrap();
        let home = std::env::temp_dir().join(format!(
            "workman-grok-mcp.test-{}",
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir(&home).unwrap();
        std::os::unix::fs::symlink(&auth, home.join("auth.json")).unwrap();
        let mut process = output_test_process("/tmp/repo");
        process.env.insert(
            WORKMAN_EPHEMERAL_AGENT_HOME_ENV.into(),
            home.to_string_lossy().into_owned(),
        );

        cleanup_ephemeral_agent_home(&process).unwrap();

        assert!(!home.exists());
        assert_eq!(fs::read_to_string(auth).unwrap(), "fixture-auth");
    }

    fn wait_for_persisted_output(registry: &mut ProcessRegistry) {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let output = registry.rendered_output(31).unwrap();
            if output.text.contains("PERSISTED-OUTPUT-313") {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for PTY output"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn daemon_startup_marks_orphaned_running_rows_as_crashed() {
        let store = Store::open_in_memory().unwrap();
        store
            .put_project(&Project {
                id: 1,
                path: "/tmp/stale-process-test".into(),
                name: "stale".into(),
                display_name: None,
                icon: None,
                selected: false,
                sort_order: 0,
            })
            .unwrap();
        store
            .put_process(&Process {
                id: 2,
                project_id: 1,
                kind: ProcessKind::Command,
                name: "orphan".into(),
                command: Some("sleep 30".into()),
                working_dir: "/tmp".into(),
                env: BTreeMap::new(),
                auto_start: false,
                auto_restart: false,
                restart_when_changed: Vec::new(),
                source: ProcessSource::Local,
                trust_hash: None,
                status: ProcessStatus::Running,
                pid: Some(999_999),
                exit_code: None,
                exit_signal: None,
                exited_at: None,
                agent_tool_id: None,
                spawned_by_process_id: None,
                sort_order: 0,
            })
            .unwrap();

        let mut registry = ProcessRegistry::new(store).unwrap();
        let process = registry.get(2).unwrap();
        assert_eq!(process.status, ProcessStatus::Crashed);
        assert_eq!(process.pid, None);
        assert!(process.exited_at.is_some());
    }

    #[test]
    fn command_spawn_uses_login_profile_and_complete_pty_environment() {
        let temp = tempfile::tempdir().unwrap();
        let shell = temp.path().join("fixture-shell");
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
        std::fs::set_permissions(&shell, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::write(
            temp.path().join(".profile"),
            "export PROFILE_VALUE='from login profile'\n",
        )
        .unwrap();
        let config = temp.path().join("config.yml");
        std::fs::write(
            &config,
            format!("terminal:\n  shell: {:?}\n", shell.to_string_lossy()),
        )
        .unwrap();

        let store = Store::open_in_memory().unwrap();
        store
            .put_project(&Project {
                id: 1,
                path: temp.path().to_string_lossy().into_owned(),
                name: "environment-fixture".into(),
                display_name: None,
                icon: None,
                selected: true,
                sort_order: 0,
            })
            .unwrap();
        let mut registry =
            ProcessRegistry::with_user_environment(store, UserEnvironmentResolver::new(&config))
                .unwrap();
        let mut environment = BTreeMap::new();
        environment.insert(
            "HOME".to_owned(),
            temp.path().to_string_lossy().into_owned(),
        );
        environment.insert(
            "WORKMAN_MCP_URL".to_owned(),
            "http://127.0.0.1:4777/mcp".to_owned(),
        );
        registry
            .create(Process {
                id: 41,
                project_id: 1,
                kind: ProcessKind::Command,
                name: "environment command".into(),
                command: Some(
                    r#"printf 'ENV:%s|%s|%s|%s|%s|%s|%s|%s\n' "$PROFILE_VALUE" "$TERM" "$COLORTERM" "$LANG" "$SHELL" "$WORKMAN_MCP_URL" "two words and a ' quote" "$TERM_PROGRAM""#.into(),
                ),
                working_dir: temp.path().to_string_lossy().into_owned(),
                env: environment.clone(),
                auto_start: false,
                auto_restart: false,
                restart_when_changed: Vec::new(),
                source: ProcessSource::Local,
                trust_hash: None,
                status: ProcessStatus::Stopped,
                pid: None,
                exit_code: None,
                exit_signal: None,
                exited_at: None,
                agent_tool_id: None,
                spawned_by_process_id: None,
                sort_order: 0,
            })
            .unwrap();
        registry.start(41).unwrap();

        let deadline = Instant::now() + Duration::from_secs(5);
        let output = loop {
            let output = registry.raw_output(41, None, usize::MAX).unwrap().data;
            if output.windows(4).any(|window| window == b"ENV:") {
                break String::from_utf8_lossy(&output).into_owned();
            }
            assert!(Instant::now() < deadline, "timed out: {output:?}");
            thread::sleep(Duration::from_millis(10));
        };
        assert!(output.contains("ENV:from login profile|xterm-256color|truecolor|"));
        assert!(output.contains(shell.to_string_lossy().as_ref()));
        assert!(output.contains("|http://127.0.0.1:4777/mcp|two words and a ' quote|"));

        registry
            .create(Process {
                id: 42,
                project_id: 1,
                kind: ProcessKind::Agent,
                name: "agent terminal capability".into(),
                command: Some(r#"printf 'AGENT_TERM_PROGRAM:%s\n' "$TERM_PROGRAM""#.into()),
                working_dir: temp.path().to_string_lossy().into_owned(),
                env: environment,
                auto_start: false,
                auto_restart: false,
                restart_when_changed: Vec::new(),
                source: ProcessSource::Local,
                trust_hash: None,
                status: ProcessStatus::Stopped,
                pid: None,
                exit_code: None,
                exit_signal: None,
                exited_at: None,
                agent_tool_id: None,
                spawned_by_process_id: None,
                sort_order: 0,
            })
            .unwrap();
        registry.start(42).unwrap();

        let deadline = Instant::now() + Duration::from_secs(5);
        let output = loop {
            let output = registry.raw_output(42, None, usize::MAX).unwrap().data;
            if output
                .windows(b"AGENT_TERM_PROGRAM:WezTerm".len())
                .any(|window| window == b"AGENT_TERM_PROGRAM:WezTerm")
            {
                break String::from_utf8_lossy(&output).into_owned();
            }
            assert!(Instant::now() < deadline, "timed out: {output:?}");
            thread::sleep(Duration::from_millis(10));
        };
        assert!(output.contains("AGENT_TERM_PROGRAM:WezTerm"));
    }

    #[test]
    fn output_reloads_after_registry_restart_and_is_removed_on_clear_and_close() {
        let temp = tempfile::tempdir().unwrap();
        let project_dir = temp.path().join("project");
        let database = temp.path().join("workman.sqlite3");
        let output_dir = temp.path().join(OUTPUT_DIRECTORY);
        std::fs::create_dir(&project_dir).unwrap();
        let project_path = project_dir.to_string_lossy().into_owned();

        {
            let store = Store::open(&database).unwrap();
            store
                .put_project(&Project {
                    id: 1,
                    path: project_path.clone(),
                    name: "persistence".into(),
                    display_name: None,
                    icon: None,
                    selected: true,
                    sort_order: 0,
                })
                .unwrap();
            let mut registry =
                ProcessRegistry::with_output_persistence(store, &output_dir, 64 * 1024).unwrap();
            registry.create(output_test_process(&project_path)).unwrap();
            registry.start(31).unwrap();
            wait_for_persisted_output(&mut registry);
            // Dropping the registry is the daemon's graceful-shutdown path: it
            // terminates live PTYs and performs the spill's final flush.
        }

        let spill_path = output_dir.join("31.raw");
        let spilled = std::fs::read(&spill_path).unwrap();
        assert!(
            String::from_utf8_lossy(&spilled).contains("PERSISTED-OUTPUT-313"),
            "distinctive output was not flushed: {spilled:?}"
        );
        assert!(spilled.len() <= 64 * 1024);

        {
            let store = Store::open(&database).unwrap();
            let mut registry =
                ProcessRegistry::with_output_persistence(store, &output_dir, 64 * 1024).unwrap();
            assert_eq!(registry.get(31).unwrap().status, ProcessStatus::Stopped);
            let raw = registry.raw_output(31, None, usize::MAX).unwrap();
            assert!(String::from_utf8_lossy(&raw.data).contains("PERSISTED-OUTPUT-313"));
            let rendered = registry.rendered_output(31).unwrap();
            assert!(rendered.text.contains("PERSISTED-OUTPUT-313"));
            assert!(!rendered.text.contains("\u{1b}[32m"));

            registry.clear_output(31).unwrap();
        }
        assert!(!spill_path.exists(), "clear_output left the spill behind");

        {
            let store = Store::open(&database).unwrap();
            let mut registry =
                ProcessRegistry::with_output_persistence(store, &output_dir, 64 * 1024).unwrap();
            assert!(registry.rendered_output(31).unwrap().text.is_empty());
            registry.start(31).unwrap();
            wait_for_persisted_output(&mut registry);
            registry.stop(31).unwrap();
            assert!(spill_path.exists());
            registry.close(31).unwrap();
            assert!(!spill_path.exists(), "close left the spill behind");
        }
    }

    #[test]
    fn signal_text_is_converted_to_persistable_numbers() {
        assert_eq!(signal_number("Terminated: 15"), Some(15));
        assert_eq!(signal_number("Killed"), Some(9));
        assert_eq!(signal_number("unknown"), None);
    }

    #[test]
    fn submit_input_does_not_wait_for_the_pty_key_boundary() {
        let store = Store::open_in_memory().unwrap();
        store
            .put_project(&Project {
                id: 1,
                path: "/tmp/submit-registry-test".into(),
                name: "submit".into(),
                display_name: None,
                icon: None,
                selected: false,
                sort_order: 0,
            })
            .unwrap();
        let mut registry = ProcessRegistry::with_stop_grace(store, Duration::from_millis(50))
            .expect("create process registry");
        registry
            .create(Process {
                id: 20,
                project_id: 1,
                kind: ProcessKind::Terminal,
                name: "input-target".into(),
                command: Some("sleep 30".into()),
                working_dir: "/tmp".into(),
                env: BTreeMap::new(),
                auto_start: false,
                auto_restart: false,
                restart_when_changed: Vec::new(),
                source: ProcessSource::Local,
                trust_hash: None,
                status: ProcessStatus::Stopped,
                pid: None,
                exit_code: None,
                exit_signal: None,
                exited_at: None,
                agent_tool_id: None,
                spawned_by_process_id: None,
                sort_order: 0,
            })
            .unwrap();
        registry.start(20).unwrap();

        let started = Instant::now();
        registry
            .submit_input_with_delay(20, b"queued", Duration::from_millis(250))
            .unwrap();
        assert!(
            started.elapsed() < Duration::from_millis(125),
            "registry remained blocked for {:?}",
            started.elapsed()
        );
        // A second registry operation is immediately available while the
        // process-local worker waits to write Enter.
        assert_eq!(registry.get(20).unwrap().status, ProcessStatus::Running);
        registry.stop(20).unwrap();
    }

    #[test]
    fn status_view_exposes_attention_and_never_calls_permission_idle() {
        let store = Store::open_in_memory().unwrap();
        store
            .put_project(&Project {
                id: 1,
                path: "/tmp/attention-registry-test".into(),
                name: "attention".into(),
                display_name: None,
                icon: None,
                selected: false,
                sort_order: 0,
            })
            .unwrap();
        store
            .put_agent_tool(&AgentTool {
                id: 9,
                name: "Claude Code".into(),
                command: "claude".into(),
                tool_type: "claude_code".into(),
                enabled: true,
                source: workman_core::AgentToolSource::Local,
                resume_args: None,
                continue_args: None,
            })
            .unwrap();

        let mut registry = ProcessRegistry::with_stop_grace(store, Duration::from_millis(100))
            .expect("create process registry");
        registry
            .create(Process {
                id: 10,
                project_id: 1,
                kind: ProcessKind::Agent,
                name: "claude".into(),
                command: Some(
                    "printf 'Do you want to proceed?\\n❯ 1. Yes, allow\\n'; sleep 30".into(),
                ),
                working_dir: "/tmp".into(),
                env: BTreeMap::new(),
                auto_start: false,
                auto_restart: false,
                restart_when_changed: Vec::new(),
                source: ProcessSource::Local,
                trust_hash: None,
                status: ProcessStatus::Stopped,
                pid: None,
                exit_code: None,
                exit_signal: None,
                exited_at: None,
                agent_tool_id: Some(9),
                spawned_by_process_id: None,
                sort_order: 0,
            })
            .unwrap();
        registry.start(10).unwrap();

        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        let waiting = loop {
            let status = registry.get_status(10).unwrap();
            if status.agent_state.state == AttentionState::NeedsInput {
                break status;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "permission dialog was not classified: {:?}",
                status.agent_state
            );
            thread::sleep(Duration::from_millis(10));
        };
        assert_eq!(
            waiting.agent_state.tool_type.as_deref(),
            Some("claude_code")
        );
        assert!(waiting.agent_state.needs_input);
        assert!(!waiting.agent_state.idle);
        let payload = serde_json::to_value(&waiting).unwrap();
        assert_eq!(payload["agent_state"]["state"], "needs_input");
        assert!(payload["agent_state"]["idle_seconds"].is_number());
        assert!(payload["agent_state"]["last_output_at"].is_number());
        assert!(payload["agent_state"]["last_content_change_at"].is_number());

        registry.stop(10).unwrap();
        let stopped = registry.get_status(10).unwrap();
        assert_eq!(stopped.agent_state.state, AttentionState::Exited);
        assert!(stopped.agent_state.exited);
    }
}
