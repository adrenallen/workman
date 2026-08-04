//! Domain values persisted by [`crate::Store`].

use std::{collections::BTreeMap, error::Error, fmt, str::FromStr};

use rusqlite::{
    ToSql,
    types::{FromSql, FromSqlError, FromSqlResult, ToSqlOutput, ValueRef},
};
use serde::{Deserialize, Serialize};

pub type ProjectId = i64;
pub type ProcessId = i64;
pub type AgentToolId = i64;
pub type TodoId = i64;
pub type TodoCommentId = i64;
pub type ScratchpadId = i64;
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

/// A registered repository/workspace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    pub id: ProjectId,
    pub path: String,
    pub name: String,
    pub display_name: Option<String>,
    pub icon: Option<String>,
    pub selected: bool,
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
}

/// A project-scoped coordination lease.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectLock {
    pub project_id: ProjectId,
    pub key: String,
    pub owner_actor: String,
    pub acquired_at: i64,
    pub ttl_ms: i64,
}

/// A deferred prompt delivery owned by an MCP actor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Timer {
    pub id: TimerId,
    pub owner_actor: String,
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
