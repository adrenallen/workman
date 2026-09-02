//! Domain values persisted by [`crate::Store`].

use std::{collections::BTreeMap, error::Error, fmt, str::FromStr};

use rusqlite::{
    ToSql,
    types::{FromSql, FromSqlError, FromSqlResult, ToSqlOutput, ValueRef},
};
use serde::{Deserialize, Serialize};

pub type ProjectId = i64;
pub type ProfileId = i64;
pub type WorktreeRepositoryId = i64;
pub type ProcessId = i64;
pub type AgentToolId = i64;
pub type AgentTemplateId = i64;
pub type QuickPromptId = i64;
pub type TodoId = i64;
pub type TodoCommentId = i64;
pub type ScratchpadId = i64;
pub type ScratchpadCommentId = i64;
pub type RecordedFeedbackId = i64;
pub type RecordedFeedbackSnapshotId = i64;
pub type RecordedFeedbackDeliveryId = i64;
pub type TimerId = i64;

/// Error returned when persisted text does not name a supported enum variant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidEnumValue {
    type_name: &'static str,
    value: String,
}

impl InvalidEnumValue {
    fn new(type_name: &'static str, value: impl Into<String>) -> Self {
        Self {
            type_name,
            value: value.into(),
        }
    }
}

impl fmt::Display for InvalidEnumValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "invalid {} value {:?}",
            self.type_name, self.value
        )
    }
}

impl Error for InvalidEnumValue {}

macro_rules! string_enum {
    (
        $(#[$meta:meta])*
        pub enum $name:ident {
            $($variant:ident => $value:literal),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        pub enum $name {
            $($variant),+
        }

        impl $name {
            pub const fn as_str(&self) -> &'static str {
                match self {
                    $(Self::$variant => $value),+
                }
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(self.as_str())
            }
        }

        impl FromStr for $name {
            type Err = InvalidEnumValue;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                match value {
                    $($value => Ok(Self::$variant)),+,
                    _ => Err(InvalidEnumValue::new(stringify!($name), value)),
                }
            }
        }

        impl ToSql for $name {
            fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
                Ok(ToSqlOutput::Borrowed(ValueRef::Text(self.as_str().as_bytes())))
            }
        }

        impl FromSql for $name {
            fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
                let value = value.as_str()?;
                value
                    .parse()
                    .map_err(|error: InvalidEnumValue| FromSqlError::Other(Box::new(error)))
            }
        }
    };
}

string_enum! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub enum ProcessKind {
        Command => "command",
        Terminal => "terminal",
        Agent => "agent",
    }
}

string_enum! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub enum RecordedFeedbackStatus {
        Recording => "recording",
        Transcribing => "transcribing",
        Ready => "ready",
        Failed => "failed",
    }
}

string_enum! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub enum ProcessSource {
        Yml => "yml",
        Local => "local",
    }
}

string_enum! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub enum ProcessStatus {
        Stopped => "stopped",
        Starting => "starting",
        Running => "running",
        Exited => "exited",
        Crashed => "crashed",
    }
}

string_enum! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub enum TodoStatus {
        Open => "open",
        InProgress => "in_progress",
        Backlog => "backlog",
        Completed => "completed",
    }
}

string_enum! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub enum TodoPriority {
        High => "high",
        Medium => "medium",
        Low => "low",
    }
}

string_enum! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub enum TimerKind {
        Delay => "delay",
        IdleAny => "idle_any",
        IdleAll => "idle_all",
    }
}

string_enum! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub enum AgentToolSource {
        Local => "local",
        Config => "config",
    }
}

string_enum! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub enum AgentLaunchMode {
        Fresh => "fresh",
        ContinuedLatest => "continued_latest",
        ResumedSession => "resumed_session",
    }
}

/// A registered repository/workspace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    pub id: ProjectId,
    pub path: String,
    pub name: String,
    pub display_name: Option<String>,
    pub icon: Option<String>,
    pub selected: bool,
    #[serde(default)]
    pub sort_order: i64,
}

/// One switchable collection of projects and user-level runtime preferences.
///
/// Project-owned coordination data follows its canonical project when that project is
/// attached to more than one profile. Daemon credentials and update secrets are global.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    pub id: ProfileId,
    pub name: String,
    pub active: bool,
    pub project_count: usize,
    pub agent_tool_count: usize,
    pub created_at: i64,
}

/// One Git repository shared by a main checkout and its linked worktrees.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorktreeRepository {
    pub id: WorktreeRepositoryId,
    /// Canonical path of Git's first/main worktree.
    pub root_path: String,
    pub name: String,
    /// Canonical parent directory used for workman-managed worktrees.
    pub managed_root: String,
}

/// Git relationship metadata for a registered workman project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectWorktree {
    pub project_id: ProjectId,
    pub repository_id: WorktreeRepositoryId,
    pub parent_project_id: Option<ProjectId>,
    pub branch: String,
    /// True only when workman (or the faithfully detected SWM predecessor) owns removal.
    pub managed: bool,
}

/// Minimal crash-recovery journal for one destructive project removal.
///
/// This deliberately does not persist general worktree-operation state. It only
/// records enough to reconcile a removal that was active when the daemon exited.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActiveWorktreeRemoval {
    pub project_id: ProjectId,
    pub phase: String,
    pub delete_from_disk: bool,
}

/// Configuration for one supported coding-agent command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentTool {
    pub id: AgentToolId,
    pub name: String,
    pub command: String,
    pub tool_type: String,
    pub enabled: bool,
    pub source: AgentToolSource,
    /// Shell arguments appended to the original command when an exact session is known.
    /// The template must contain `{session_id}`.
    pub resume_args: Option<String>,
    /// Shell arguments appended to continue the cwd-scoped latest session.
    pub continue_args: Option<String>,
}

/// A reusable agent launch choice scoped to one workspace profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentTemplate {
    pub id: AgentTemplateId,
    pub profile_id: ProfileId,
    pub name: String,
    pub agent_tool_id: AgentToolId,
    pub extra_args: Vec<String>,
    pub prompt: String,
    pub sort_order: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

/// One reusable prompt snippet owned by a workspace profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuickPrompt {
    pub id: QuickPromptId,
    pub name: String,
    pub body: String,
    pub sort_order: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Durable conversation identity and the strategy used for an agent's latest launch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSession {
    pub process_id: ProcessId,
    pub session_id: Option<String>,
    pub launch_mode: AgentLaunchMode,
    pub launched_at: i64,
    pub captured_at: Option<i64>,
}

/// Persisted process metadata. PTY output is intentionally kept out of SQLite.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Process {
    pub id: ProcessId,
    pub project_id: ProjectId,
    pub kind: ProcessKind,
    pub name: String,
    pub command: Option<String>,
    pub working_dir: String,
    pub env: BTreeMap<String, String>,
    pub auto_start: bool,
    pub auto_restart: bool,
    pub restart_when_changed: Vec<String>,
    pub source: ProcessSource,
    pub trust_hash: Option<String>,
    pub status: ProcessStatus,
    pub pid: Option<u32>,
    pub exit_code: Option<i32>,
    pub exit_signal: Option<i32>,
    pub exited_at: Option<i64>,
    pub agent_tool_id: Option<AgentToolId>,
    #[serde(default)]
    pub spawned_by_process_id: Option<ProcessId>,
    #[serde(default)]
    pub sort_order: i64,
}

/// A work item and its lease-based edit-lock metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Todo {
    pub id: TodoId,
    pub project_id: ProjectId,
    pub title: String,
    pub body: String,
    pub status: TodoStatus,
    pub priority: TodoPriority,
    pub completed: bool,
    pub tags: Vec<String>,
    pub lock_actor: Option<String>,
    pub lock_process_id: Option<ProcessId>,
    pub lock_expiry: Option<i64>,
}

/// One directed edge in the todo dependency graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TodoBlocker {
    pub todo_id: TodoId,
    pub blocked_by_todo_id: TodoId,
}

/// A durable comment on a todo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TodoComment {
    pub id: TodoCommentId,
    pub todo_id: TodoId,
    pub actor: String,
    pub body: String,
    pub created_at: i64,
    pub updated_at: i64,
}

/// A revision-guarded shared markdown buffer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scratchpad {
    pub id: ScratchpadId,
    pub project_id: ProjectId,
    pub name: String,
    pub content: String,
    pub revision: i64,
    pub tags: Vec<String>,
    pub archived: bool,
    pub created_by: String,
    pub updated_by: String,
}

/// A durable whole-document or text-anchored comment on a scratchpad.
///
/// Anchor offsets are UTF-16 code-unit offsets so they use the same coordinate
/// system as CodeMirror and browser selection APIs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScratchpadComment {
    pub id: ScratchpadCommentId,
    pub scratchpad_id: ScratchpadId,
    pub actor: String,
    pub body: String,
    pub quote: Option<String>,
    pub anchor_start: Option<usize>,
    pub anchor_end: Option<usize>,
    pub anchor_prefix: Option<String>,
    pub anchor_suffix: Option<String>,
    pub anchor_revision: Option<i64>,
    pub resolved: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

/// One locally recorded feedback session. Media paths always point inside Workman's data root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordedFeedback {
    pub id: RecordedFeedbackId,
    pub project_id: ProjectId,
    pub title: String,
    pub status: RecordedFeedbackStatus,
    pub revision: i64,
    pub duration_ms: i64,
    pub audio_path: Option<String>,
    pub transcript: Vec<RecordedFeedbackTranscriptSegment>,
    pub blocks: Vec<RecordedFeedbackBlock>,
    pub snapshots: Vec<RecordedFeedbackSnapshot>,
    pub deliveries: Vec<RecordedFeedbackDelivery>,
    pub error_code: Option<String>,
    pub archived: bool,
    pub lease_owner: Option<String>,
    pub lease_expires_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordedFeedbackSummary {
    pub id: RecordedFeedbackId,
    pub project_id: ProjectId,
    pub title: String,
    pub status: RecordedFeedbackStatus,
    pub revision: i64,
    pub duration_ms: i64,
    pub snapshot_count: usize,
    pub archived: bool,
    pub error_code: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordedFeedbackTranscriptSegment {
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RecordedFeedbackBlock {
    Text {
        text: String,
        start_ms: i64,
        end_ms: i64,
    },
    Image {
        snapshot_id: RecordedFeedbackSnapshotId,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordedFeedbackSnapshot {
    pub id: RecordedFeedbackSnapshotId,
    pub feedback_id: RecordedFeedbackId,
    pub ordinal: i64,
    pub anchor_ms: i64,
    pub anchor_samples: i64,
    pub invoked_at_ms: i64,
    pub completed_at_ms: i64,
    pub image_path: String,
    pub caption: String,
    pub width: u32,
    pub height: u32,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordedFeedbackDelivery {
    pub id: RecordedFeedbackDeliveryId,
    pub feedback_id: RecordedFeedbackId,
    pub target_kind: String,
    pub target_id: Option<i64>,
    pub status: String,
    pub packet_path: Option<String>,
    pub error_message: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// A project-scoped coordination lease.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectLock {
    pub project_id: ProjectId,
    pub key: String,
    pub owner_actor: String,
    pub owner_process_id: Option<ProcessId>,
    pub acquired_at: i64,
    pub ttl_ms: i64,
}

/// A deferred prompt delivery owned by a durable process when one is available.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Timer {
    pub id: TimerId,
    #[serde(skip_serializing)]
    pub owner_actor: String,
    pub owner_process_id: Option<ProcessId>,
    pub delivery_process_id: ProcessId,
    pub body: String,
    pub kind: TimerKind,
    pub watch_process_ids: Vec<ProcessId>,
    pub interval_ms: Option<i64>,
    pub repeating: bool,
    pub max_wait_deadline: Option<i64>,
    pub paused: bool,
    pub fired: bool,
    pub fired_at: Option<i64>,
    pub created_at: i64,
}

/// Identity and effective-project state for one MCP session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Actor {
    pub id: String,
    pub session_id: String,
    pub process_id: Option<ProcessId>,
    pub selected_project_id: Option<ProjectId>,
    pub created_at: i64,
    pub last_seen_at: i64,
}
