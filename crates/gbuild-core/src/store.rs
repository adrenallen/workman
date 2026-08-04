//! SQLite connection setup, schema migration, and domain persistence.

use std::{error::Error, fmt, path::Path, time::Duration};

use rusqlite::{Connection, OptionalExtension, Row, TransactionBehavior, params, types::Type};
use serde::{Serialize, de::DeserializeOwned};

use crate::domain::{
    Actor, AgentTool, Process, ProcessId, Project, ProjectId, ProjectLock, Scratchpad, Timer, Todo,
    TodoBlocker, TodoComment, TodoId,
};

const MIGRATIONS: &[(i64, &str, &str)] = &[
    (1, "initial", include_str!("../migrations/0001_initial.sql")),
    (
        2,
        "mcp_identity",
        include_str!("../migrations/0002_mcp_identity.sql"),
    ),
    (
        3,
        "agent_tool_presets",
        include_str!("../migrations/0003_agent_tool_presets.sql"),
    ),
    (
        4,
        "timer_runtime",
        include_str!("../migrations/0004_timer_runtime.sql"),
    ),
    (
        5,
        "agent_tool_source",
        include_str!("../migrations/0005_agent_tool_source.sql"),
    ),
];

/// Version of the newest migration compiled into this crate.
pub const LATEST_SCHEMA_VERSION: i64 = 5;

/// Errors produced while opening, migrating, or using the SQLite store.
#[derive(Debug)]
pub enum StoreError {
    Sqlite(rusqlite::Error),
    Json(serde_json::Error),
    SchemaTooNew { found: i64, supported: i64 },
}

impl fmt::Display for StoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sqlite(error) => error.fmt(formatter),
            Self::Json(error) => error.fmt(formatter),
            Self::SchemaTooNew { found, supported } => write!(
                formatter,
                "database schema version {found} is newer than supported version {supported}"
            ),
        }
    }
}

impl Error for StoreError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Sqlite(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::SchemaTooNew { .. } => None,
        }
    }
}

impl From<rusqlite::Error> for StoreError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

impl From<serde_json::Error> for StoreError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

pub type StoreResult<T> = Result<T, StoreError>;

/// The durable gbuild state store.
///
/// Every constructor applies the connection invariants (foreign keys, a busy timeout,
/// WAL, and normal synchronous writes) and migrates the database before returning.
pub struct Store {
    connection: Connection,
}

impl Store {
    /// Open or create a file-backed database and migrate it to the current schema.
    pub fn open(path: impl AsRef<Path>) -> StoreResult<Self> {
        Self::from_connection(Connection::open(path)?)
    }

    /// Open an isolated in-memory store. Intended for tests and short-lived services.
    pub fn open_in_memory() -> StoreResult<Self> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    /// Adopt a caller-created connection, apply gbuild's pragmas, and run migrations.
    pub fn from_connection(connection: Connection) -> StoreResult<Self> {
        connection.pragma_update(None, "foreign_keys", true)?;
        connection.busy_timeout(Duration::from_secs(5))?;

        // In-memory SQLite databases report `memory` here; file databases switch to WAL.
        let _: String = connection.query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))?;
        connection.pragma_update(None, "synchronous", "NORMAL")?;

        let mut store = Self { connection };
        store.migrate()?;
        Ok(store)
    }

    /// Apply all migrations not yet recorded in `schema_migrations`.
    pub fn migrate(&mut self) -> StoreResult<()> {
        self.connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_migrations (
                version    INTEGER PRIMARY KEY,
                name       TEXT NOT NULL,
                applied_at INTEGER NOT NULL DEFAULT (unixepoch())
            );",
        )?;

        let found = self.schema_version()?;
        if found > LATEST_SCHEMA_VERSION {
            return Err(StoreError::SchemaTooNew {
                found,
                supported: LATEST_SCHEMA_VERSION,
            });
        }

        for &(version, name, sql) in MIGRATIONS {
            let applied = self.connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version = ?1)",
                [version],
                |row| row.get::<_, bool>(0),
            )?;
            if applied {
                continue;
            }

            let transaction = self
                .connection
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            transaction.execute_batch(sql)?;
            transaction.execute(
                "INSERT INTO schema_migrations (version, name) VALUES (?1, ?2)",
                params![version, name],
            )?;
            transaction.pragma_update(None, "user_version", version)?;
            transaction.commit()?;
        }

        Ok(())
    }

    /// Return the highest successfully applied migration, or zero for a blank database.
    pub fn schema_version(&self) -> StoreResult<i64> {
        let version = self.connection.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |row| row.get(0),
        )?;
        Ok(version)
    }

    /// Access the connection for specialized reads that do not yet have store methods.
    pub fn connection(&self) -> &Connection {
        &self.connection
    }

    pub fn put_project(&self, project: &Project) -> StoreResult<()> {
        self.connection.execute(
            "INSERT INTO projects (id, path, name, display_name, icon, selected)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET
                path = excluded.path,
                name = excluded.name,
                display_name = excluded.display_name,
                icon = excluded.icon,
                selected = excluded.selected",
            params![
                project.id,
                project.path,
                project.name,
                project.display_name,
                project.icon,
                project.selected,
            ],
        )?;
        Ok(())
    }

    pub fn get_project(&self, id: ProjectId) -> StoreResult<Option<Project>> {
        let project = self
            .connection
            .query_row(
                "SELECT id, path, name, display_name, icon, selected
                 FROM projects WHERE id = ?1",
                [id],
                |row| {
                    Ok(Project {
                        id: row.get(0)?,
                        path: row.get(1)?,
                        name: row.get(2)?,
                        display_name: row.get(3)?,
                        icon: row.get(4)?,
                        selected: row.get(5)?,
                    })
                },
            )
            .optional()?;
        Ok(project)
    }

    pub fn list_projects(&self) -> StoreResult<Vec<Project>> {
        let mut statement = self.connection.prepare(
            "SELECT id, path, name, display_name, icon, selected FROM projects ORDER BY name, id",
        )?;
        let projects = statement
            .query_map([], |row| {
                Ok(Project {
                    id: row.get(0)?,
                    path: row.get(1)?,
                    name: row.get(2)?,
                    display_name: row.get(3)?,
                    icon: row.get(4)?,
                    selected: row.get(5)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(projects)
    }

    pub fn next_project_id(&self) -> StoreResult<ProjectId> {
        let id = self.connection.query_row(
            "SELECT COALESCE(MAX(id), 0) + 1 FROM projects",
            [],
            |row| row.get(0),
        )?;
        Ok(id)
    }

    pub fn delete_project(&self, id: ProjectId) -> StoreResult<bool> {
        Ok(self
            .connection
            .execute("DELETE FROM projects WHERE id = ?1", [id])?
            > 0)
    }

    pub fn put_agent_tool(&self, tool: &AgentTool) -> StoreResult<()> {
        self.connection.execute(
            "INSERT INTO agent_tools (id, name, command, tool_type, enabled, source)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                command = excluded.command,
                tool_type = excluded.tool_type,
                enabled = excluded.enabled,
                source = excluded.source",
            params![
                tool.id,
                tool.name,
                tool.command,
                tool.tool_type,
                tool.enabled,
                tool.source
            ],
        )?;
        Ok(())
    }

    pub fn get_agent_tool(&self, id: i64) -> StoreResult<Option<AgentTool>> {
        let tool = self
            .connection
            .query_row(
                "SELECT id, name, command, tool_type, enabled, source
                 FROM agent_tools WHERE id = ?1",
                [id],
                |row| {
                    Ok(AgentTool {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        command: row.get(2)?,
                        tool_type: row.get(3)?,
                        enabled: row.get(4)?,
                        source: row.get(5)?,
                    })
                },
            )
            .optional()?;
        Ok(tool)
    }

    pub fn list_agent_tools(&self) -> StoreResult<Vec<AgentTool>> {
        let mut statement = self.connection.prepare(
            "SELECT id, name, command, tool_type, enabled, source
             FROM agent_tools ORDER BY id",
        )?;
        let tools = statement
            .query_map([], |row| {
                Ok(AgentTool {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    command: row.get(2)?,
                    tool_type: row.get(3)?,
                    enabled: row.get(4)?,
                    source: row.get(5)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(tools)
    }

    pub fn next_agent_tool_id(&self) -> StoreResult<i64> {
        Ok(self.connection.query_row(
            "SELECT COALESCE(MAX(id), 0) + 1 FROM agent_tools",
            [],
            |row| row.get(0),
        )?)
    }

    pub fn delete_agent_tool(&self, id: i64) -> StoreResult<bool> {
        Ok(self
            .connection
            .execute("DELETE FROM agent_tools WHERE id = ?1", [id])?
            > 0)
    }

    pub fn put_process(&self, process: &Process) -> StoreResult<()> {
        let env = to_json(&process.env)?;
        let restart_when_changed = to_json(&process.restart_when_changed)?;
        self.connection.execute(
            "INSERT INTO processes (
                id, project_id, kind, name, command, working_dir, env, auto_start,
                auto_restart, restart_when_changed, source, trust_hash, status, pid,
                exit_code, exit_signal, exited_at, agent_tool_id
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
                ?15, ?16, ?17, ?18
             )
             ON CONFLICT(id) DO UPDATE SET
                project_id = excluded.project_id,
                kind = excluded.kind,
                name = excluded.name,
                command = excluded.command,
                working_dir = excluded.working_dir,
                env = excluded.env,
                auto_start = excluded.auto_start,
                auto_restart = excluded.auto_restart,
                restart_when_changed = excluded.restart_when_changed,
                source = excluded.source,
                trust_hash = excluded.trust_hash,
                status = excluded.status,
                pid = excluded.pid,
                exit_code = excluded.exit_code,
                exit_signal = excluded.exit_signal,
                exited_at = excluded.exited_at,
                agent_tool_id = excluded.agent_tool_id",
            params![
                process.id,
                process.project_id,
                process.kind,
                process.name,
                process.command,
                process.working_dir,
                env,
                process.auto_start,
                process.auto_restart,
                restart_when_changed,
                process.source,
                process.trust_hash,
                process.status,
                process.pid,
                process.exit_code,
                process.exit_signal,
                process.exited_at,
                process.agent_tool_id,
            ],
        )?;
        Ok(())
    }

    pub fn get_process(&self, id: ProcessId) -> StoreResult<Option<Process>> {
        let process = self
            .connection
            .query_row(
                "SELECT id, project_id, kind, name, command, working_dir, env, auto_start,
                        auto_restart, restart_when_changed, source, trust_hash, status, pid,
                        exit_code, exit_signal, exited_at, agent_tool_id
                 FROM processes WHERE id = ?1",
                [id],
                process_from_row,
            )
            .optional()?;
        Ok(process)
    }

    /// List process records, optionally restricting the result to one project.
    pub fn list_processes(&self, project_id: Option<ProjectId>) -> StoreResult<Vec<Process>> {
        let (sql, parameter) = match project_id {
            Some(project_id) => (
                "SELECT id, project_id, kind, name, command, working_dir, env, auto_start,
                        auto_restart, restart_when_changed, source, trust_hash, status, pid,
                        exit_code, exit_signal, exited_at, agent_tool_id
                 FROM processes WHERE project_id = ?1 ORDER BY id",
                Some(project_id),
            ),
            None => (
                "SELECT id, project_id, kind, name, command, working_dir, env, auto_start,
                        auto_restart, restart_when_changed, source, trust_hash, status, pid,
                        exit_code, exit_signal, exited_at, agent_tool_id
                 FROM processes ORDER BY project_id, id",
                None,
            ),
        };

        let mut statement = self.connection.prepare(sql)?;
        let processes = match parameter {
            Some(project_id) => statement
                .query_map([project_id], process_from_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?,
            None => statement
                .query_map([], process_from_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?,
        };
        Ok(processes)
    }

    /// Return an unused positive process ID for a caller that serializes creates.
    pub fn next_process_id(&self) -> StoreResult<ProcessId> {
        let id = self.connection.query_row(
            "SELECT COALESCE(MAX(id), 0) + 1 FROM processes",
            [],
            |row| row.get(0),
        )?;
        Ok(id)
    }

    /// Delete one process record. Related timers and actor links follow schema FK rules.
    pub fn delete_process(&self, id: ProcessId) -> StoreResult<bool> {
        Ok(self
            .connection
            .execute("DELETE FROM processes WHERE id = ?1", [id])?
            > 0)
    }

    pub fn set_process_mcp_token(
        &self,
        process_id: ProcessId,
        token: &str,
        created_at: i64,
    ) -> StoreResult<()> {
        self.connection.execute(
            "INSERT INTO process_mcp_tokens (process_id, token, created_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(process_id) DO UPDATE SET
                token = excluded.token,
                created_at = excluded.created_at",
            params![process_id, token, created_at],
        )?;
        Ok(())
    }

    pub fn clear_process_mcp_token(&self, process_id: ProcessId) -> StoreResult<bool> {
        Ok(self.connection.execute(
            "DELETE FROM process_mcp_tokens WHERE process_id = ?1",
            [process_id],
        )? > 0)
    }

    pub fn get_process_by_mcp_token(&self, token: &str) -> StoreResult<Option<Process>> {
        let process = self
            .connection
            .query_row(
                "SELECT p.id, p.project_id, p.kind, p.name, p.command, p.working_dir, p.env,
                        p.auto_start, p.auto_restart, p.restart_when_changed, p.source,
                        p.trust_hash, p.status, p.pid, p.exit_code, p.exit_signal, p.exited_at,
                        p.agent_tool_id
                 FROM process_mcp_tokens AS token
                 JOIN processes AS p ON p.id = token.process_id
                 WHERE token.token = ?1",
                [token],
                process_from_row,
            )
            .optional()?;
        Ok(process)
    }

    pub fn put_todo(&mut self, todo: &Todo) -> StoreResult<()> {
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "INSERT INTO todos (
                id, project_id, title, body, status, priority, completed, lock_actor, lock_expiry
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(id) DO UPDATE SET
                project_id = excluded.project_id,
                title = excluded.title,
                body = excluded.body,
                status = excluded.status,
                priority = excluded.priority,
                completed = excluded.completed,
                lock_actor = excluded.lock_actor,
                lock_expiry = excluded.lock_expiry",
            params![
                todo.id,
                todo.project_id,
                todo.title,
                todo.body,
                todo.status,
                todo.priority,
                todo.completed,
                todo.lock_actor,
                todo.lock_expiry,
            ],
        )?;
        transaction.execute("DELETE FROM todo_tags WHERE todo_id = ?1", [todo.id])?;
        for (position, tag) in todo.tags.iter().enumerate() {
            transaction.execute(
                "INSERT INTO todo_tags (todo_id, tag, position) VALUES (?1, ?2, ?3)",
                params![todo.id, tag, position as i64],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn get_todo(&self, id: TodoId) -> StoreResult<Option<Todo>> {
        let mut todo = self
            .connection
            .query_row(
                "SELECT id, project_id, title, body, status, priority, completed,
                        lock_actor, lock_expiry
                 FROM todos WHERE id = ?1",
                [id],
                |row| {
                    Ok(Todo {
                        id: row.get(0)?,
                        project_id: row.get(1)?,
                        title: row.get(2)?,
                        body: row.get(3)?,
                        status: row.get(4)?,
                        priority: row.get(5)?,
                        completed: row.get(6)?,
                        tags: Vec::new(),
                        lock_actor: row.get(7)?,
                        lock_expiry: row.get(8)?,
                    })
                },
            )
            .optional()?;

        if let Some(todo) = &mut todo {
            todo.tags = query_strings(
                &self.connection,
                "SELECT tag FROM todo_tags WHERE todo_id = ?1 ORDER BY position",
                todo.id,
            )?;
        }
        Ok(todo)
    }

    pub fn put_todo_blocker(&self, blocker: &TodoBlocker) -> StoreResult<()> {
        self.connection.execute(
            "INSERT INTO todo_blockers (todo_id, blocked_by_todo_id) VALUES (?1, ?2)
             ON CONFLICT(todo_id, blocked_by_todo_id) DO NOTHING",
            params![blocker.todo_id, blocker.blocked_by_todo_id],
        )?;
        Ok(())
    }

    pub fn get_todo_blocker(
        &self,
        todo_id: TodoId,
        blocked_by_todo_id: TodoId,
    ) -> StoreResult<Option<TodoBlocker>> {
        let blocker = self
            .connection
            .query_row(
                "SELECT todo_id, blocked_by_todo_id FROM todo_blockers
                 WHERE todo_id = ?1 AND blocked_by_todo_id = ?2",
                params![todo_id, blocked_by_todo_id],
                |row| {
                    Ok(TodoBlocker {
                        todo_id: row.get(0)?,
                        blocked_by_todo_id: row.get(1)?,
                    })
                },
            )
            .optional()?;
        Ok(blocker)
    }

    pub fn put_todo_comment(&self, comment: &TodoComment) -> StoreResult<()> {
        self.connection.execute(
            "INSERT INTO todo_comments (id, todo_id, actor, body, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET
                todo_id = excluded.todo_id,
                actor = excluded.actor,
                body = excluded.body,
                created_at = excluded.created_at,
                updated_at = excluded.updated_at",
            params![
                comment.id,
                comment.todo_id,
                comment.actor,
                comment.body,
                comment.created_at,
                comment.updated_at,
            ],
        )?;
        Ok(())
    }

    pub fn get_todo_comment(&self, id: i64) -> StoreResult<Option<TodoComment>> {
        let comment = self
            .connection
            .query_row(
                "SELECT id, todo_id, actor, body, created_at, updated_at
                 FROM todo_comments WHERE id = ?1",
                [id],
                |row| {
                    Ok(TodoComment {
                        id: row.get(0)?,
                        todo_id: row.get(1)?,
                        actor: row.get(2)?,
                        body: row.get(3)?,
                        created_at: row.get(4)?,
                        updated_at: row.get(5)?,
                    })
                },
            )
            .optional()?;
        Ok(comment)
    }

    pub fn put_scratchpad(&mut self, scratchpad: &Scratchpad) -> StoreResult<()> {
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "INSERT INTO scratchpads (id, project_id, name, content, revision, archived)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET
                project_id = excluded.project_id,
                name = excluded.name,
                content = excluded.content,
                revision = excluded.revision,
                archived = excluded.archived",
            params![
                scratchpad.id,
                scratchpad.project_id,
                scratchpad.name,
                scratchpad.content,
                scratchpad.revision,
                scratchpad.archived,
            ],
        )?;
        transaction.execute(
            "DELETE FROM scratchpad_tags WHERE scratchpad_id = ?1",
            [scratchpad.id],
        )?;
        for (position, tag) in scratchpad.tags.iter().enumerate() {
            transaction.execute(
                "INSERT INTO scratchpad_tags (scratchpad_id, tag, position)
                 VALUES (?1, ?2, ?3)",
                params![scratchpad.id, tag, position as i64],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn get_scratchpad(&self, id: i64) -> StoreResult<Option<Scratchpad>> {
        let mut scratchpad = self
            .connection
            .query_row(
                "SELECT id, project_id, name, content, revision, archived
                 FROM scratchpads WHERE id = ?1",
                [id],
                |row| {
                    Ok(Scratchpad {
                        id: row.get(0)?,
                        project_id: row.get(1)?,
                        name: row.get(2)?,
                        content: row.get(3)?,
                        revision: row.get(4)?,
                        tags: Vec::new(),
                        archived: row.get(5)?,
                    })
                },
            )
            .optional()?;

        if let Some(scratchpad) = &mut scratchpad {
            scratchpad.tags = query_strings(
                &self.connection,
                "SELECT tag FROM scratchpad_tags WHERE scratchpad_id = ?1 ORDER BY position",
                scratchpad.id,
            )?;
        }
        Ok(scratchpad)
    }

    pub fn put_project_lock(&self, lock: &ProjectLock) -> StoreResult<()> {
        self.connection.execute(
            "INSERT INTO locks (project_id, key, owner_actor, acquired_at, ttl)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(project_id, key) DO UPDATE SET
                owner_actor = excluded.owner_actor,
                acquired_at = excluded.acquired_at,
                ttl = excluded.ttl",
            params![
                lock.project_id,
                lock.key,
                lock.owner_actor,
                lock.acquired_at,
                lock.ttl_ms,
            ],
        )?;
        Ok(())
    }

    pub fn get_project_lock(
        &self,
        project_id: ProjectId,
        key: &str,
    ) -> StoreResult<Option<ProjectLock>> {
        let lock = self
            .connection
            .query_row(
                "SELECT project_id, key, owner_actor, acquired_at, ttl
                 FROM locks WHERE project_id = ?1 AND key = ?2",
                params![project_id, key],
                |row| {
                    Ok(ProjectLock {
                        project_id: row.get(0)?,
                        key: row.get(1)?,
                        owner_actor: row.get(2)?,
                        acquired_at: row.get(3)?,
                        ttl_ms: row.get(4)?,
                    })
                },
            )
            .optional()?;
        Ok(lock)
    }

    pub fn put_timer(&self, timer: &Timer) -> StoreResult<()> {
        let watch_list = to_json(&timer.watch_process_ids)?;
        self.connection.execute(
            "INSERT INTO timers (
                id, owner_actor, delivery_process_id, body, kind, watch_list, interval,
                loop, max_wait_deadline, paused, fired, fired_at, created_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
             ON CONFLICT(id) DO UPDATE SET
                owner_actor = excluded.owner_actor,
                delivery_process_id = excluded.delivery_process_id,
                body = excluded.body,
                kind = excluded.kind,
                watch_list = excluded.watch_list,
                interval = excluded.interval,
                loop = excluded.loop,
                max_wait_deadline = excluded.max_wait_deadline,
                paused = excluded.paused,
                fired = excluded.fired,
                fired_at = excluded.fired_at,
                created_at = excluded.created_at",
            params![
                timer.id,
                timer.owner_actor,
                timer.delivery_process_id,
                timer.body,
                timer.kind,
                watch_list,
                timer.interval_ms,
                timer.repeating,
                timer.max_wait_deadline,
                timer.paused,
                timer.fired,
                timer.fired_at,
                timer.created_at,
            ],
        )?;
        Ok(())
    }

    pub fn get_timer(&self, id: i64) -> StoreResult<Option<Timer>> {
        let timer = self
            .connection
            .query_row(
                "SELECT id, owner_actor, delivery_process_id, body, kind, watch_list,
                        interval, loop, max_wait_deadline, paused, fired, fired_at, created_at
                 FROM timers WHERE id = ?1",
                [id],
                |row| {
                    Ok(Timer {
                        id: row.get(0)?,
                        owner_actor: row.get(1)?,
                        delivery_process_id: row.get(2)?,
                        body: row.get(3)?,
                        kind: row.get(4)?,
                        watch_process_ids: json_from_row(row, 5)?,
                        interval_ms: row.get(6)?,
                        repeating: row.get(7)?,
                        max_wait_deadline: row.get(8)?,
                        paused: row.get(9)?,
                        fired: row.get(10)?,
                        fired_at: row.get(11)?,
                        created_at: row.get(12)?,
                    })
                },
            )
            .optional()?;
        Ok(timer)
    }

    pub fn put_actor(&self, actor: &Actor) -> StoreResult<()> {
        self.connection.execute(
            "INSERT INTO actors (
                id, session_id, process_id, selected_project_id, created_at, last_seen_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET
                session_id = excluded.session_id,
                process_id = excluded.process_id,
                selected_project_id = excluded.selected_project_id,
                created_at = excluded.created_at,
                last_seen_at = excluded.last_seen_at",
            params![
                actor.id,
                actor.session_id,
                actor.process_id,
                actor.selected_project_id,
                actor.created_at,
                actor.last_seen_at,
            ],
        )?;
        Ok(())
    }

    pub fn get_actor(&self, id: &str) -> StoreResult<Option<Actor>> {
        let actor = self
            .connection
            .query_row(
                "SELECT id, session_id, process_id, selected_project_id, created_at, last_seen_at
                 FROM actors WHERE id = ?1",
                [id],
                |row| {
                    Ok(Actor {
                        id: row.get(0)?,
                        session_id: row.get(1)?,
                        process_id: row.get(2)?,
                        selected_project_id: row.get(3)?,
                        created_at: row.get(4)?,
                        last_seen_at: row.get(5)?,
                    })
                },
            )
            .optional()?;
        Ok(actor)
    }

    pub fn get_actor_by_session_id(&self, session_id: &str) -> StoreResult<Option<Actor>> {
        let actor = self
            .connection
            .query_row(
                "SELECT id, session_id, process_id, selected_project_id, created_at, last_seen_at
                 FROM actors WHERE session_id = ?1",
                [session_id],
                |row| {
                    Ok(Actor {
                        id: row.get(0)?,
                        session_id: row.get(1)?,
                        process_id: row.get(2)?,
                        selected_project_id: row.get(3)?,
                        created_at: row.get(4)?,
                        last_seen_at: row.get(5)?,
                    })
                },
            )
            .optional()?;
        Ok(actor)
    }

    /// Exercise a temporary write/read/delete cycle on the active SQLite connection.
    pub fn smoke_test(&mut self) -> StoreResult<bool> {
        let transaction = self.connection.transaction()?;
        transaction.execute_batch(
            "CREATE TEMP TABLE IF NOT EXISTS mcp_smoke_test (
                id INTEGER PRIMARY KEY,
                value TEXT NOT NULL
             );
             DELETE FROM mcp_smoke_test;",
        )?;
        transaction.execute(
            "INSERT INTO mcp_smoke_test (id, value) VALUES (1, 'ok')",
            [],
        )?;
        let value: String =
            transaction.query_row("SELECT value FROM mcp_smoke_test WHERE id = 1", [], |row| {
                row.get(0)
            })?;
        transaction.execute("DELETE FROM mcp_smoke_test WHERE id = 1", [])?;
        transaction.execute_batch("DROP TABLE mcp_smoke_test;")?;
        transaction.commit()?;
        Ok(value == "ok")
    }
}

fn process_from_row(row: &Row<'_>) -> rusqlite::Result<Process> {
    Ok(Process {
        id: row.get(0)?,
        project_id: row.get(1)?,
        kind: row.get(2)?,
        name: row.get(3)?,
        command: row.get(4)?,
        working_dir: row.get(5)?,
        env: json_from_row(row, 6)?,
        auto_start: row.get(7)?,
        auto_restart: row.get(8)?,
        restart_when_changed: json_from_row(row, 9)?,
        source: row.get(10)?,
        trust_hash: row.get(11)?,
        status: row.get(12)?,
        pid: row.get(13)?,
        exit_code: row.get(14)?,
        exit_signal: row.get(15)?,
        exited_at: row.get(16)?,
        agent_tool_id: row.get(17)?,
    })
}

fn to_json(value: &impl Serialize) -> StoreResult<String> {
    Ok(serde_json::to_string(value)?)
}

fn json_from_row<T: DeserializeOwned>(row: &Row<'_>, index: usize) -> rusqlite::Result<T> {
    let json = row.get::<_, String>(index)?;
    serde_json::from_str(&json).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(index, Type::Text, Box::new(error))
    })
}

fn query_strings(connection: &Connection, sql: &str, id: i64) -> StoreResult<Vec<String>> {
    let mut statement = connection.prepare(sql)?;
    let strings = statement
        .query_map([id], |row| row.get(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(strings)
}
