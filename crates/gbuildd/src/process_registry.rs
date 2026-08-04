//! Persistent process registry and PTY lifecycle orchestration.

use std::{
    collections::HashMap,
    error::Error,
    fmt,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use gbuild_core::{
    Process, ProcessId, ProcessKind, ProcessStatus, ProjectId, Store, StoreError,
    attention::{AgentState, AttentionTracker},
    pty::{ExitStatus, PtyProcess, PtySize, PtySpawnOptions, RawOutput},
    terminal::TerminalOutput,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const DEFAULT_STOP_GRACE: Duration = Duration::from_millis(500);

/// Errors returned by process registry operations.
#[derive(Debug)]
pub enum RegistryError {
    Store(StoreError),
    NotFound(ProcessId),
    AlreadyExists(ProcessId),
    AlreadyRunning(ProcessId),
    NotRunning(ProcessId),
    MissingCommand(ProcessId),
    InvalidName,
    Pty {
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
            Self::MissingCommand(_) => "process_missing_command",
            Self::InvalidName => "invalid_process_name",
            Self::Pty { .. } => "pty_error",
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
            Self::MissingCommand(id) => write!(formatter, "process {id} has no command to start"),
            Self::InvalidName => formatter.write_str("process name must not be empty"),
            Self::Pty {
                process_id,
                message,
            } => write!(
                formatter,
                "PTY operation for process {process_id} failed: {message}"
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
}

/// Owns persisted process records and live PTY handles.
pub struct ProcessRegistry {
    store: Store,
    running: HashMap<ProcessId, PtyProcess>,
    outputs: HashMap<ProcessId, ProcessOutput>,
    selected: HashMap<ProjectId, ProcessId>,
    stop_grace: Duration,
}

impl ProcessRegistry {
    /// Create a registry and mark process rows left running by an earlier daemon as crashed.
    pub fn new(store: Store) -> RegistryResult<Self> {
        Self::with_stop_grace(store, DEFAULT_STOP_GRACE)
    }

    pub fn with_stop_grace(store: Store, stop_grace: Duration) -> RegistryResult<Self> {
        let mut registry = Self {
            store,
            running: HashMap::new(),
            outputs: HashMap::new(),
            selected: HashMap::new(),
            stop_grace,
        };
        registry.reconcile_stale_processes()?;
        Ok(registry)
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    pub fn store_mut(&mut self) -> &mut Store {
        &mut self.store
    }

    /// Insert a new stopped process. An ID <= 0 is replaced with the next database ID.
    pub fn create(&mut self, mut process: Process) -> RegistryResult<Process> {
        if process.id <= 0 {
            process.id = self.store.next_process_id()?;
        } else if self.store.get_process(process.id)?.is_some() {
            return Err(RegistryError::AlreadyExists(process.id));
        }
        validate_name(&process.name)?;
        process.status = ProcessStatus::Stopped;
        process.pid = None;
        process.exit_code = None;
        process.exit_signal = None;
        process.exited_at = None;
        self.store.put_process(&process)?;
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
        self.store.put_process(&process)?;
        Ok(process)
    }

    pub fn get(&mut self, process_id: ProcessId) -> RegistryResult<Process> {
        self.refresh_exits()?;
        self.require(process_id)
    }

    /// Get a process with raw signals, adapter flags, and derived attention state.
    pub fn get_status(&mut self, process_id: ProcessId) -> RegistryResult<ProcessStatusView> {
        let process = self.get(process_id)?;
        self.status_view(process)
    }

    pub fn list(&mut self, project_id: Option<ProjectId>) -> RegistryResult<Vec<Process>> {
        self.refresh_exits()?;
        Ok(self.store.list_processes(project_id)?)
    }

    /// List process status views, including agent state for every process.
    pub fn list_statuses(
        &mut self,
        project_id: Option<ProjectId>,
    ) -> RegistryResult<Vec<ProcessStatusView>> {
        self.list(project_id)?
            .into_iter()
            .map(|process| self.status_view(process))
            .collect()
    }

    /// Attach attention state to an already-loaded process record.
    pub fn status_view(&self, process: Process) -> RegistryResult<ProcessStatusView> {
        let tool_type = self.tool_type_for(&process)?;
        let agent_state = self
            .outputs
            .get(&process.id)
            .map(|output| output.attention.snapshot())
            .unwrap_or_else(|| AgentState::exited(tool_type, process.exited_at));
        Ok(ProcessStatusView {
            process,
            agent_state,
        })
    }

    pub fn start(&mut self, process_id: ProcessId) -> RegistryResult<Process> {
        self.refresh_exits()?;
        if self.running.contains_key(&process_id) {
            return Err(RegistryError::AlreadyRunning(process_id));
        }

        let mut process = self.require(process_id)?;
        let command = process
            .command
            .as_deref()
            .filter(|command| !command.trim().is_empty())
            .ok_or(RegistryError::MissingCommand(process_id))?
            .to_owned();

        process.status = ProcessStatus::Starting;
        process.pid = None;
        process.exit_code = None;
        process.exit_signal = None;
        process.exited_at = None;
        self.store.put_process(&process)?;

        let tool_type = self.tool_type_for(&process)?;
        let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        self.store
            .set_process_mcp_token(process.id, &token, now_millis())?;
        let mut options = PtySpawnOptions::new(process.id, token, command);
        if let Some(tool_type) = tool_type {
            options = options.with_tool_type(tool_type);
        }
        if !process.working_dir.is_empty() {
            options = options.with_working_dir(&process.working_dir);
        }
        for (key, value) in &process.env {
            options = options.with_env(key, value);
        }

        let mut hosted = match PtyProcess::spawn(options) {
            Ok(hosted) => hosted,
            Err(error) => {
                let _ = self.store.clear_process_mcp_token(process_id);
                process.status = ProcessStatus::Crashed;
                process.exited_at = Some(now_millis());
                self.store.put_process(&process)?;
                return Err(RegistryError::Pty {
                    process_id,
                    message: error.to_string(),
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
        self.outputs.insert(
            process_id,
            ProcessOutput {
                raw: hosted.raw_output(),
                terminal: hosted.terminal_output(),
                attention: hosted.attention_tracker(),
            },
        );
        self.running.insert(process_id, hosted);
        self.refresh_exits()?;
        self.require(process_id)
    }

    pub fn stop(&mut self, process_id: ProcessId) -> RegistryResult<Process> {
        self.refresh_exits()?;
        let mut process = self.require(process_id)?;
        let Some(mut hosted) = self.running.remove(&process_id) else {
            return Ok(process);
        };
        let _ = self.store.clear_process_mcp_token(process_id);

        match hosted.terminate(self.stop_grace) {
            Ok(status) => {
                apply_exit_info(&mut process, &status);
                process.status = ProcessStatus::Stopped;
                self.store.put_process(&process)?;
                Ok(process)
            }
            Err(error) => {
                process.status = ProcessStatus::Crashed;
                process.pid = None;
                process.exited_at = Some(now_millis());
                self.store.put_process(&process)?;
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

    /// Terminate a live process and remove its durable entry.
    pub fn close(&mut self, process_id: ProcessId) -> RegistryResult<Process> {
        self.refresh_exits()?;
        let process = if self.running.contains_key(&process_id) {
            self.stop(process_id)?
        } else {
            self.require(process_id)?
        };
        self.store.delete_process(process_id)?;
        self.outputs.remove(&process_id);
        self.selected.retain(|_, selected| *selected != process_id);
        Ok(process)
    }

    pub fn rename(&mut self, process_id: ProcessId, name: String) -> RegistryResult<Process> {
        validate_name(&name)?;
        let mut process = self.get(process_id)?;
        process.name = name;
        self.store.put_process(&process)?;
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

        let snapshot = output.raw.snapshot();
        let total_bytes = output.raw.total_bytes_seen();
        let retained_start = total_bytes.saturating_sub(snapshot.len() as u64);
        let requested = offset.unwrap_or(retained_start);
        let start_offset = requested.clamp(retained_start, total_bytes);
        let start = usize::try_from(start_offset - retained_start).unwrap_or(snapshot.len());
        let end = start.saturating_add(max_bytes).min(snapshot.len());
        let data = snapshot[start..end].to_vec();
        let end_offset = start_offset.saturating_add(data.len() as u64);

        Ok(RawOutputChunk {
            data,
            start_offset,
            end_offset,
            total_bytes,
            truncated: requested < retained_start || end_offset < total_bytes,
            status: process.status,
        })
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

    /// Send raw bytes to a live process's PTY.
    pub fn send_input(&mut self, process_id: ProcessId, data: &[u8]) -> RegistryResult<Process> {
        self.refresh_exits()?;
        let hosted = self
            .running
            .get_mut(&process_id)
            .ok_or(RegistryError::NotRunning(process_id))?;
        hosted.write_all(data).map_err(|error| RegistryError::Pty {
            process_id,
            message: error.to_string(),
        })?;
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
        let hosted = self
            .running
            .get(&process_id)
            .ok_or(RegistryError::NotRunning(process_id))?;
        hosted
            .resize(PtySize {
                rows: rows.max(1),
                cols: cols.max(1),
                pixel_width,
                pixel_height,
            })
            .map_err(|error| RegistryError::Pty {
                process_id,
                message: error.to_string(),
            })?;
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
        }
        Ok(())
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

    fn require(&self, process_id: ProcessId) -> RegistryResult<Process> {
        self.store
            .get_process(process_id)?
            .ok_or(RegistryError::NotFound(process_id))
    }

    fn tool_type_for(&self, process: &Process) -> RegistryResult<Option<String>> {
        if let Some(agent_tool_id) = process.agent_tool_id {
            if let Some(tool) = self.store.get_agent_tool(agent_tool_id)? {
                return Ok(Some(tool.tool_type));
            }
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

struct ProcessOutput {
    raw: RawOutput,
    terminal: TerminalOutput,
    attention: AttentionTracker,
}

impl Drop for ProcessRegistry {
    fn drop(&mut self) {
        for (process_id, mut hosted) in self.running.drain() {
            let _ = self.store.clear_process_mcp_token(process_id);
            let status = hosted.terminate(self.stop_grace).ok();
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

#[derive(Clone, Copy)]
enum BulkAction {
    Start,
    Stop,
    Restart,
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::thread;

    use gbuild_core::{AgentTool, ProcessSource, Project, attention::AttentionState};

    use super::*;

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
            })
            .unwrap();

        let mut registry = ProcessRegistry::new(store).unwrap();
        let process = registry.get(2).unwrap();
        assert_eq!(process.status, ProcessStatus::Crashed);
        assert_eq!(process.pid, None);
        assert!(process.exited_at.is_some());
    }

    #[test]
    fn signal_text_is_converted_to_persistable_numbers() {
        assert_eq!(signal_number("Terminated: 15"), Some(15));
        assert_eq!(signal_number("Killed"), Some(9));
        assert_eq!(signal_number("unknown"), None);
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
            })
            .unwrap();
        store
            .put_agent_tool(&AgentTool {
                id: 9,
                name: "Claude Code".into(),
                command: "claude".into(),
                tool_type: "claude_code".into(),
                enabled: true,
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
