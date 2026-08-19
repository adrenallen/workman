//! SQLite connection setup, schema migration, and domain persistence.

use std::{
    collections::{HashMap, HashSet},
    error::Error,
    fmt,
    path::Path,
    time::Duration,
};

use rusqlite::{Connection, OptionalExtension, Row, TransactionBehavior, params, types::Type};
use serde::{Serialize, de::DeserializeOwned};

use crate::domain::{
    ActiveWorktreeRemoval, Actor, AgentLaunchMode, AgentSession, AgentTemplate, AgentTool, Process,
    ProcessId, ProcessKind, Profile, ProfileId, Project, ProjectId, ProjectLock, ProjectWorktree,
    QuickPrompt, Scratchpad, Timer, Todo, TodoBlocker, TodoComment, TodoId, WorktreeRepository,
    WorktreeRepositoryId,
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
    (
        6,
        "process_lineage",
        include_str!("../migrations/0006_process_lineage.sql"),
    ),
    (
        7,
        "sort_order",
        include_str!("../migrations/0007_sort_order.sql"),
    ),
    (
        8,
        "worktrees",
        include_str!("../migrations/0008_worktrees.sql"),
    ),
    (
        9,
        "todo_claims",
        include_str!("../migrations/0009_todo_claims.sql"),
    ),
    (
        10,
        "agent_notifications",
        include_str!("../migrations/0010_agent_notifications.sql"),
    ),
    (
        11,
        "notifications",
        include_str!("../migrations/0011_notifications.sql"),
    ),
    (
        12,
        "project_icon_color",
        include_str!("../migrations/0012_project_icon_color.sql"),
    ),
    (
        13,
        "agent_tool_sort_order",
        include_str!("../migrations/0013_agent_tool_sort_order.sql"),
    ),
    (
        14,
        "agent_tool_yolo_defaults",
        include_str!("../migrations/0014_agent_tool_yolo_defaults.sql"),
    ),
    (
        15,
        "agent_notification_rate_limit",
        include_str!("../migrations/0015_agent_notification_rate_limit.sql"),
    ),
    (
        16,
        "notification_needs_input",
        include_str!("../migrations/0016_notification_needs_input.sql"),
    ),
    (
        17,
        "todo_activity",
        include_str!("../migrations/0017_todo_activity.sql"),
    ),
    (
        18,
        "consumed_idle_watches",
        include_str!("../migrations/0018_consumed_idle_watches.sql"),
    ),
    (
        19,
        "human_assignment_mentions",
        include_str!("../migrations/0019_human_assignment_mentions.sql"),
    ),
    (
        20,
        "sidebar_sort_order",
        include_str!("../migrations/0020_sidebar_sort_order.sql"),
    ),
    (
        21,
        "agent_session_resume",
        include_str!("../migrations/0021_agent_session_resume.sql"),
    ),
    (
        22,
        "actor_attribution",
        include_str!("../migrations/0022_actor_attribution.sql"),
    ),
    (
        23,
        "unique_agent_sessions",
        include_str!("../migrations/0023_unique_agent_sessions.sql"),
    ),
    (
        24,
        "profiles",
        include_str!("../migrations/0024_profiles.sql"),
    ),
    (
        25,
        "project_folders",
        include_str!("../migrations/0025_project_folders.sql"),
    ),
    (
        26,
        "grok_agent_preset",
        include_str!("../migrations/0026_grok_agent_preset.sql"),
    ),
    (
        27,
        "process_ownership",
        include_str!("../migrations/0027_process_ownership.sql"),
    ),
    (
        28,
        "agent_templates",
        include_str!("../migrations/0028_agent_templates.sql"),
    ),
    (
        29,
        "quick_prompts",
        include_str!("../migrations/0029_quick_prompts.sql"),
    ),
    (
        30,
        "scratchpad_comments",
        include_str!("../migrations/0030_scratchpad_comments.sql"),
    ),
    (
        31,
        "active_worktree_removals",
        include_str!("../migrations/0031_active_worktree_removals.sql"),
    ),
];

/// Version of the newest migration compiled into this crate.
pub const LATEST_SCHEMA_VERSION: i64 = 31;

/// Errors produced while opening, migrating, or using the SQLite store.
#[derive(Debug)]
pub enum StoreError {
    Sqlite(rusqlite::Error),
    Json(serde_json::Error),
    SchemaTooNew { found: i64, supported: i64 },
    InvalidReorder(String),
    InvalidProfile(String),
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
            Self::InvalidReorder(message) | Self::InvalidProfile(message) => {
                formatter.write_str(message)
            }
        }
    }
}

impl Error for StoreError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Sqlite(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::SchemaTooNew { .. } | Self::InvalidReorder(_) | Self::InvalidProfile(_) => None,
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

/// The durable workman state store.
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

    /// Adopt a caller-created connection, apply workman's pragmas, and run migrations.
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

    /// Return the active profile. Every migrated database has exactly one.
    pub fn active_profile_id(&self) -> StoreResult<ProfileId> {
        Ok(self
            .connection
            .query_row("SELECT id FROM profiles WHERE active = 1", [], |row| {
                row.get(0)
            })?)
    }

    pub fn list_profiles(&self) -> StoreResult<Vec<Profile>> {
        let mut statement = self.connection.prepare(
            "SELECT p.id, p.name, p.active,
                    COUNT(DISTINCT pp.project_id), COUNT(DISTINCT a.id), p.created_at
             FROM profiles AS p
             LEFT JOIN profile_projects AS pp ON pp.profile_id = p.id
             LEFT JOIN agent_tools AS a ON a.profile_id = p.id
             GROUP BY p.id
             ORDER BY p.active DESC, p.created_at, p.id",
        )?;
        Ok(statement
            .query_map([], profile_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn get_profile(&self, profile_id: ProfileId) -> StoreResult<Option<Profile>> {
        Ok(self
            .connection
            .query_row(
                "SELECT p.id, p.name, p.active,
                        COUNT(DISTINCT pp.project_id), COUNT(DISTINCT a.id), p.created_at
                 FROM profiles AS p
                 LEFT JOIN profile_projects AS pp ON pp.profile_id = p.id
                 LEFT JOIN agent_tools AS a ON a.profile_id = p.id
                 WHERE p.id = ?1
                 GROUP BY p.id",
                [profile_id],
                profile_from_row,
            )
            .optional()?)
    }

    pub fn list_profile_projects(&self, profile_id: ProfileId) -> StoreResult<Vec<Project>> {
        let mut statement = self.connection.prepare(
            "SELECT pr.id, pr.path, pr.name, pr.display_name, pr.icon,
                    pp.selected, pp.sort_order
             FROM projects AS pr
             JOIN profile_projects AS pp ON pp.project_id = pr.id
             WHERE pp.profile_id = ?1
             ORDER BY pp.sort_order, pr.id",
        )?;
        Ok(statement
            .query_map([profile_id], project_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn list_profile_agent_tools(&self, profile_id: ProfileId) -> StoreResult<Vec<AgentTool>> {
        let mut statement = self.connection.prepare(
            "SELECT id, COALESCE(display_name, name), command, tool_type, enabled, source,
                    resume_args, continue_args
             FROM agent_tools WHERE profile_id = ?1 ORDER BY sort_order, id",
        )?;
        Ok(statement
            .query_map([profile_id], |row| {
                Ok(AgentTool {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    command: row.get(2)?,
                    tool_type: row.get(3)?,
                    enabled: row.get(4)?,
                    source: row.get(5)?,
                    resume_args: row.get(6)?,
                    continue_args: row.get(7)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn list_profile_agent_templates(
        &self,
        profile_id: ProfileId,
    ) -> StoreResult<Vec<AgentTemplate>> {
        let mut statement = self.connection.prepare(
            "SELECT id, profile_id, name, agent_tool_id, extra_args, prompt, sort_order,
                    created_at, updated_at
             FROM agent_templates WHERE profile_id = ?1 ORDER BY sort_order, id",
        )?;
        Ok(statement
            .query_map([profile_id], agent_template_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn profile_terminal_shell(&self, profile_id: ProfileId) -> StoreResult<Option<String>> {
        Ok(self
            .connection
            .query_row(
                "SELECT terminal_shell FROM profiles WHERE id = ?1",
                [profile_id],
                |row| row.get(0),
            )
            .optional()?
            .flatten())
    }

    /// Import a fully validated inactive profile. Project paths are canonicalized by the caller.
    /// Returned tool IDs follow `tools` order for custom-icon installation.
    pub fn import_profile(
        &self,
        name: &str,
        terminal_shell: Option<&str>,
        projects: &[(String, bool)],
        tools: &[AgentTool],
    ) -> StoreResult<(Profile, Vec<i64>)> {
        let name = normalized_profile_name(name)?;
        if projects.iter().filter(|(_, selected)| *selected).count() > 1 {
            return Err(StoreError::InvalidProfile(
                "an imported profile may select at most one project".into(),
            ));
        }
        let profile_id: ProfileId = self.connection.query_row(
            "SELECT COALESCE(MAX(id), 0) + 1 FROM profiles",
            [],
            |row| row.get(0),
        )?;
        let transaction = self.connection.unchecked_transaction()?;
        transaction.execute(
            "INSERT INTO profiles (
                id, name, active, terminal_shell, legacy_config_imported, created_at
             ) VALUES (?1, ?2, 0, ?3, 1, unixepoch())",
            params![profile_id, name, terminal_shell],
        )?;

        let mut next_project_id: i64 =
            transaction.query_row("SELECT COALESCE(MAX(id), 0) + 1 FROM projects", [], |row| {
                row.get(0)
            })?;
        for (position, (path, selected)) in projects.iter().enumerate() {
            let project_id = transaction
                .query_row("SELECT id FROM projects WHERE path = ?1", [path], |row| {
                    row.get::<_, i64>(0)
                })
                .optional()?
                .unwrap_or_else(|| {
                    let id = next_project_id;
                    next_project_id += 1;
                    id
                });
            transaction.execute(
                "INSERT INTO projects (id, path, name, selected, sort_order)
                 VALUES (?1, ?2, ?3, 0, 0)
                 ON CONFLICT(path) DO NOTHING",
                params![project_id, path, project_name_from_path(path)],
            )?;
            transaction.execute(
                "INSERT INTO profile_projects (profile_id, project_id, sort_order, selected)
                 VALUES (?1, ?2, ?3, ?4)",
                params![profile_id, project_id, position as i64, selected],
            )?;
        }

        let first_tool_id: i64 = transaction.query_row(
            "SELECT COALESCE(MAX(id), 0) + 1 FROM agent_tools",
            [],
            |row| row.get(0),
        )?;
        let mut tool_ids = Vec::with_capacity(tools.len());
        for (position, tool) in tools.iter().enumerate() {
            let id = first_tool_id + position as i64;
            transaction.execute(
                "INSERT INTO agent_tools (
                    id, name, display_name, command, tool_type, enabled, source, sort_order,
                    resume_args, continue_args, profile_id
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    id,
                    profile_agent_storage_name(profile_id, id),
                    tool.name,
                    tool.command,
                    tool.tool_type,
                    tool.enabled,
                    tool.source,
                    position as i64,
                    tool.resume_args,
                    tool.continue_args,
                    profile_id,
                ],
            )?;
            tool_ids.push(id);
        }
        transaction.commit()?;
        let profile = self
            .get_profile(profile_id)?
            .ok_or_else(|| StoreError::InvalidProfile("imported profile was not found".into()))?;
        Ok((profile, tool_ids))
    }

    /// Create an inactive profile. `copy_active` snapshots membership, shell, agent tools,
    /// templates, and quick prompts.
    /// The returned ID pairs map source agent-tool IDs to their independent copies so callers
    /// can clone custom icon files without putting filesystem paths in the database.
    pub fn create_profile(
        &self,
        name: &str,
        copy_active: bool,
    ) -> StoreResult<(Profile, Vec<(i64, i64)>)> {
        let name = normalized_profile_name(name)?;
        let source_profile_id = self.active_profile_id()?;
        let profile_id = self.connection.query_row(
            "SELECT COALESCE(MAX(id), 0) + 1 FROM profiles",
            [],
            |row| row.get(0),
        )?;
        let transaction = self.connection.unchecked_transaction()?;
        let shell: Option<String> = if copy_active {
            transaction.query_row(
                "SELECT terminal_shell FROM profiles WHERE id = ?1",
                [source_profile_id],
                |row| row.get(0),
            )?
        } else {
            None
        };
        transaction.execute(
            "INSERT INTO profiles (
                id, name, active, terminal_shell, legacy_config_imported, created_at
             ) VALUES (?1, ?2, 0, ?3, 1, unixepoch())",
            params![profile_id, name, shell],
        )?;
        if copy_active {
            transaction.execute(
                "INSERT INTO profile_projects (
                    profile_id, project_id, sort_order, selected, folder_id
                 )
                 SELECT ?1, project_id, sort_order, selected, NULL
                 FROM profile_projects WHERE profile_id = ?2",
                params![profile_id, source_profile_id],
            )?;

            let folders = {
                let mut statement = transaction.prepare(
                    "SELECT id, name, collapsed, sort_order
                     FROM project_folders WHERE profile_id = ?1 ORDER BY sort_order, id",
                )?;
                statement
                    .query_map([source_profile_id], |row| {
                        Ok((
                            row.get::<_, i64>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, bool>(2)?,
                            row.get::<_, i64>(3)?,
                        ))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?
            };
            let first_folder_id: i64 = transaction.query_row(
                "SELECT COALESCE(MAX(id), 0) + 1 FROM project_folders",
                [],
                |row| row.get(0),
            )?;
            for (offset, (source_folder_id, folder_name, collapsed, sort_order)) in
                folders.into_iter().enumerate()
            {
                let target_folder_id = first_folder_id + offset as i64;
                transaction.execute(
                    "INSERT INTO project_folders (
                        id, profile_id, name, collapsed, sort_order
                     ) VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![
                        target_folder_id,
                        profile_id,
                        folder_name,
                        collapsed,
                        sort_order
                    ],
                )?;
                transaction.execute(
                    "UPDATE profile_projects SET folder_id = ?1
                     WHERE profile_id = ?2
                       AND project_id IN (
                           SELECT project_id FROM profile_projects
                           WHERE profile_id = ?4 AND folder_id = ?3
                       )",
                    params![
                        target_folder_id,
                        profile_id,
                        source_folder_id,
                        source_profile_id
                    ],
                )?;
            }
        }

        let mut icon_pairs = Vec::new();
        if copy_active {
            let tools = {
                let mut statement = transaction.prepare(
                    "SELECT id, COALESCE(display_name, name), command, tool_type, enabled, source,
                            sort_order, resume_args, continue_args
                     FROM agent_tools WHERE profile_id = ?1 ORDER BY sort_order, id",
                )?;
                statement
                    .query_map([source_profile_id], |row| {
                        Ok((
                            row.get::<_, i64>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, bool>(4)?,
                            row.get::<_, String>(5)?,
                            row.get::<_, i64>(6)?,
                            row.get::<_, Option<String>>(7)?,
                            row.get::<_, Option<String>>(8)?,
                        ))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?
            };
            let first_id: i64 = transaction.query_row(
                "SELECT COALESCE(MAX(id), 0) + 1 FROM agent_tools",
                [],
                |row| row.get(0),
            )?;
            for (
                position,
                (
                    old_id,
                    display_name,
                    command,
                    tool_type,
                    enabled,
                    source,
                    sort_order,
                    resume_args,
                    continue_args,
                ),
            ) in tools.into_iter().enumerate()
            {
                let next_id = first_id + position as i64;
                let storage_name = profile_agent_storage_name(profile_id, next_id);
                transaction.execute(
                    "INSERT INTO agent_tools (
                        id, name, display_name, command, tool_type, enabled, source, sort_order,
                        resume_args, continue_args, profile_id
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                    params![
                        next_id,
                        storage_name,
                        display_name,
                        command,
                        tool_type,
                        enabled,
                        source,
                        sort_order,
                        resume_args,
                        continue_args,
                        profile_id,
                    ],
                )?;
                icon_pairs.push((old_id, next_id));
            }

            let tool_id_map = icon_pairs.iter().copied().collect::<HashMap<_, _>>();
            let templates = {
                let mut statement = transaction.prepare(
                    "SELECT name, agent_tool_id, extra_args, prompt, sort_order
                     FROM agent_templates WHERE profile_id = ?1 ORDER BY sort_order, id",
                )?;
                statement
                    .query_map([source_profile_id], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, i64>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, i64>(4)?,
                        ))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?
            };
            let first_template_id: i64 = transaction.query_row(
                "SELECT COALESCE(MAX(id), 0) + 1 FROM agent_templates",
                [],
                |row| row.get(0),
            )?;
            for (position, (name, source_tool_id, extra_args, prompt, sort_order)) in
                templates.into_iter().enumerate()
            {
                let copied_tool_id = tool_id_map.get(&source_tool_id).ok_or_else(|| {
                    StoreError::InvalidProfile(format!(
                        "agent template references uncopied tool {source_tool_id}"
                    ))
                })?;
                transaction.execute(
                    "INSERT INTO agent_templates (
                        id, profile_id, name, agent_tool_id, extra_args, prompt, sort_order,
                        created_at, updated_at
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, unixepoch(), unixepoch())",
                    params![
                        first_template_id + position as i64,
                        profile_id,
                        name,
                        copied_tool_id,
                        extra_args,
                        prompt,
                        sort_order,
                    ],
                )?;
            }

            let prompts = {
                let mut statement = transaction.prepare(
                    "SELECT name, body, sort_order
                     FROM quick_prompts WHERE profile_id = ?1 ORDER BY sort_order, id",
                )?;
                statement
                    .query_map([source_profile_id], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, i64>(2)?,
                        ))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?
            };
            let first_prompt_id: i64 = transaction.query_row(
                "SELECT COALESCE(MAX(id), 0) + 1 FROM quick_prompts",
                [],
                |row| row.get(0),
            )?;
            for (position, (name, body, sort_order)) in prompts.into_iter().enumerate() {
                transaction.execute(
                    "INSERT INTO quick_prompts (
                        id, profile_id, name, body, sort_order, created_at, updated_at
                     ) VALUES (?1, ?2, ?3, ?4, ?5, unixepoch(), unixepoch())",
                    params![
                        first_prompt_id + position as i64,
                        profile_id,
                        name,
                        body,
                        sort_order,
                    ],
                )?;
            }
        }
        transaction.commit()?;
        let profile = self
            .get_profile(profile_id)?
            .ok_or_else(|| StoreError::InvalidProfile("created profile was not found".into()))?;
        Ok((profile, icon_pairs))
    }

    pub fn rename_profile(&self, profile_id: ProfileId, name: &str) -> StoreResult<Profile> {
        let name = normalized_profile_name(name)?;
        if self.connection.execute(
            "UPDATE profiles SET name = ?1 WHERE id = ?2",
            params![name, profile_id],
        )? == 0
        {
            return Err(StoreError::InvalidProfile(format!(
                "profile {profile_id} was not found"
            )));
        }
        self.get_profile(profile_id)?
            .ok_or_else(|| StoreError::InvalidProfile("renamed profile was not found".into()))
    }

    /// Delete an inactive profile and return its agent-tool IDs for icon cleanup.
    pub fn delete_profile(&self, profile_id: ProfileId) -> StoreResult<Vec<i64>> {
        let profile = self.get_profile(profile_id)?.ok_or_else(|| {
            StoreError::InvalidProfile(format!("profile {profile_id} was not found"))
        })?;
        if profile.active {
            return Err(StoreError::InvalidProfile(
                "switch away before deleting the active profile".into(),
            ));
        }
        let tool_ids = {
            let mut statement = self
                .connection
                .prepare("SELECT id FROM agent_tools WHERE profile_id = ?1 ORDER BY id")?;
            statement
                .query_map([profile_id], |row| row.get(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        self.connection
            .execute("DELETE FROM profiles WHERE id = ?1", [profile_id])?;
        Ok(tool_ids)
    }

    pub fn switch_profile(&self, profile_id: ProfileId) -> StoreResult<Profile> {
        if self.get_profile(profile_id)?.is_none() {
            return Err(StoreError::InvalidProfile(format!(
                "profile {profile_id} was not found"
            )));
        }
        let transaction = self.connection.unchecked_transaction()?;
        transaction.execute("UPDATE profiles SET active = 0 WHERE active = 1", [])?;
        transaction.execute("UPDATE profiles SET active = 1 WHERE id = ?1", [profile_id])?;
        transaction.commit()?;
        self.get_profile(profile_id)?
            .ok_or_else(|| StoreError::InvalidProfile("switched profile was not found".into()))
    }

    pub fn active_profile_terminal_shell(&self) -> StoreResult<Option<String>> {
        Ok(self.connection.query_row(
            "SELECT terminal_shell FROM profiles WHERE active = 1",
            [],
            |row| row.get(0),
        )?)
    }

    pub fn set_active_profile_terminal_shell(&self, shell: Option<&str>) -> StoreResult<()> {
        self.connection.execute(
            "UPDATE profiles SET terminal_shell = ?1 WHERE active = 1",
            [shell],
        )?;
        Ok(())
    }

    pub fn active_profile_needs_legacy_config_import(&self) -> StoreResult<bool> {
        Ok(self.connection.query_row(
            "SELECT legacy_config_imported = 0 FROM profiles WHERE active = 1",
            [],
            |row| row.get(0),
        )?)
    }

    pub fn mark_active_profile_legacy_config_imported(&self) -> StoreResult<()> {
        self.connection.execute(
            "UPDATE profiles SET legacy_config_imported = 1 WHERE active = 1",
            [],
        )?;
        Ok(())
    }

    pub fn is_project_in_active_profile(&self, project_id: ProjectId) -> StoreResult<bool> {
        Ok(self.connection.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM profile_projects AS pp
                JOIN profiles AS p ON p.id = pp.profile_id AND p.active = 1
                WHERE pp.project_id = ?1
             )",
            [project_id],
            |row| row.get(0),
        )?)
    }

    pub fn attach_project_to_active_profile(&self, project_id: ProjectId) -> StoreResult<()> {
        let profile_id = self.active_profile_id()?;
        let sort_order =
            crate::project_folders::next_project_top_level_order(&self.connection, profile_id)?;
        self.connection.execute(
            "INSERT INTO profile_projects (profile_id, project_id, sort_order, selected)
             SELECT ?1, ?2, ?3,
                    CASE WHEN COUNT(*) = 0 THEN 1 ELSE 0 END
             FROM profile_projects WHERE profile_id = ?1
             ON CONFLICT(profile_id, project_id) DO NOTHING",
            params![profile_id, project_id, sort_order],
        )?;
        Ok(())
    }

    pub fn select_project_in_active_profile(&self, project_id: ProjectId) -> StoreResult<bool> {
        if !self.is_project_in_active_profile(project_id)? {
            return Ok(false);
        }
        let profile_id = self.active_profile_id()?;
        let transaction = self.connection.unchecked_transaction()?;
        transaction.execute(
            "UPDATE profile_projects SET selected = 0 WHERE profile_id = ?1",
            [profile_id],
        )?;
        transaction.execute(
            "UPDATE profile_projects SET selected = 1
             WHERE profile_id = ?1 AND project_id = ?2",
            params![profile_id, project_id],
        )?;
        transaction.commit()?;
        Ok(true)
    }

    pub fn put_project(&self, project: &Project) -> StoreResult<()> {
        let transaction = self.connection.unchecked_transaction()?;
        transaction.execute(
            "INSERT INTO projects (id, path, name, display_name, icon, selected, sort_order)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(id) DO UPDATE SET
                path = excluded.path,
                name = excluded.name,
                display_name = excluded.display_name,
                icon = COALESCE(excluded.icon, projects.icon),
                selected = excluded.selected,
                sort_order = excluded.sort_order",
            params![
                project.id,
                project.path,
                project.name,
                project.display_name,
                project.icon,
                project.selected,
                project.sort_order,
            ],
        )?;
        let profile_id: ProfileId =
            transaction.query_row("SELECT id FROM profiles WHERE active = 1", [], |row| {
                row.get(0)
            })?;
        let sort_order =
            crate::project_folders::next_project_top_level_order(&transaction, profile_id)?;
        transaction.execute(
            "INSERT INTO profile_projects (profile_id, project_id, sort_order, selected)
             SELECT ?1, ?2, ?3,
                    CASE WHEN COUNT(*) = 0 THEN 1 ELSE 0 END
             FROM profile_projects WHERE profile_id = ?1
             ON CONFLICT(profile_id, project_id) DO NOTHING",
            params![profile_id, project.id, sort_order],
        )?;
        if project.selected {
            transaction.execute(
                "UPDATE profile_projects SET selected = CASE WHEN project_id = ?2 THEN 1 ELSE 0 END
                 WHERE profile_id = ?1",
                params![profile_id, project.id],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn get_project(&self, id: ProjectId) -> StoreResult<Option<Project>> {
        let project = self
            .connection
            .query_row(
                "SELECT pr.id, pr.path, pr.name, pr.display_name, pr.icon,
                        pp.selected, pp.sort_order
                 FROM projects AS pr
                 JOIN profile_projects AS pp ON pp.project_id = pr.id
                 JOIN profiles AS p ON p.id = pp.profile_id AND p.active = 1
                 WHERE pr.id = ?1",
                [id],
                |row| {
                    Ok(Project {
                        id: row.get(0)?,
                        path: row.get(1)?,
                        name: row.get(2)?,
                        display_name: row.get(3)?,
                        icon: row.get(4)?,
                        selected: row.get(5)?,
                        sort_order: row.get(6)?,
                    })
                },
            )
            .optional()?;
        Ok(project)
    }

    /// Read a canonical project without applying active-profile visibility.
    pub fn get_project_any(&self, id: ProjectId) -> StoreResult<Option<Project>> {
        Ok(self
            .connection
            .query_row(
                "SELECT id, path, name, display_name, icon, selected, sort_order
                 FROM projects WHERE id = ?1",
                [id],
                project_from_row,
            )
            .optional()?)
    }

    pub fn get_project_by_path_any(&self, path: &str) -> StoreResult<Option<Project>> {
        Ok(self
            .connection
            .query_row(
                "SELECT id, path, name, display_name, icon, selected, sort_order
                 FROM projects WHERE path = ?1",
                [path],
                project_from_row,
            )
            .optional()?)
    }

    pub fn list_projects(&self) -> StoreResult<Vec<Project>> {
        let mut statement = self.connection.prepare(
            "SELECT pr.id, pr.path, pr.name, pr.display_name, pr.icon,
                    pp.selected, pp.sort_order
             FROM projects AS pr
             JOIN profile_projects AS pp ON pp.project_id = pr.id
             JOIN profiles AS p ON p.id = pp.profile_id AND p.active = 1
             ORDER BY pp.sort_order, pr.id",
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
                    sort_order: row.get(6)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(projects)
    }

    /// List canonical projects across every profile.
    ///
    /// Destructive path validation uses this broader view so deleting from one
    /// profile cannot silently remove a directory containing a project that is
    /// only registered in another profile.
    pub fn list_all_projects(&self) -> StoreResult<Vec<Project>> {
        let mut statement = self.connection.prepare(
            "SELECT id, path, name, display_name, icon, selected, sort_order
             FROM projects
             ORDER BY id",
        )?;
        let projects = statement
            .query_map([], project_from_row)?
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

    pub fn next_project_sort_order(&self) -> StoreResult<i64> {
        crate::project_folders::next_project_top_level_order(
            &self.connection,
            self.active_profile_id()?,
        )
    }

    /// Replace the complete project order in one transaction.
    pub fn reorder_projects(&mut self, ordered_ids: &[ProjectId]) -> StoreResult<Vec<Project>> {
        if !self.list_project_folders()?.is_empty() {
            return Err(StoreError::InvalidReorder(
                "project.reorder cannot flatten a project rail that contains folders".to_owned(),
            ));
        }
        let current = self
            .list_projects()?
            .into_iter()
            .map(|project| project.id)
            .collect::<Vec<_>>();
        validate_reorder_ids("project", &current, ordered_ids)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        for (sort_order, project_id) in ordered_ids.iter().enumerate() {
            transaction.execute(
                "UPDATE profile_projects SET sort_order = ?1
                 WHERE profile_id = (SELECT id FROM profiles WHERE active = 1)
                   AND project_id = ?2",
                params![sort_order as i64, project_id],
            )?;
        }
        transaction.commit()?;
        self.list_projects()
    }

    pub fn delete_project(&self, id: ProjectId) -> StoreResult<bool> {
        Ok(self.connection.execute(
            "DELETE FROM profile_projects
             WHERE profile_id = (SELECT id FROM profiles WHERE active = 1)
               AND project_id = ?1",
            [id],
        )? > 0)
    }

    /// Permanently remove a canonical project and every profile membership.
    ///
    /// Registration-only removal should use [`Self::delete_project`] so other
    /// profiles keep their project set. Destructive worktree removal must use
    /// this method because the path no longer exists for any profile.
    pub fn delete_project_everywhere(&self, id: ProjectId) -> StoreResult<bool> {
        Ok(self
            .connection
            .execute("DELETE FROM projects WHERE id = ?1", [id])?
            > 0)
    }

    /// Persist the intentionally small restart journal for an active removal.
    pub fn put_active_worktree_removal(&self, removal: &ActiveWorktreeRemoval) -> StoreResult<()> {
        self.connection.execute(
            "INSERT INTO active_worktree_removals
                (project_id, phase, delete_from_disk, updated_at)
             VALUES (?1, ?2, ?3, unixepoch())
             ON CONFLICT(project_id) DO UPDATE SET
                phase = excluded.phase,
                delete_from_disk = excluded.delete_from_disk,
                updated_at = excluded.updated_at",
            params![removal.project_id, removal.phase, removal.delete_from_disk,],
        )?;
        Ok(())
    }

    pub fn update_active_worktree_removal_phase(
        &self,
        project_id: ProjectId,
        phase: &str,
    ) -> StoreResult<bool> {
        Ok(self.connection.execute(
            "UPDATE active_worktree_removals
             SET phase = ?2, updated_at = unixepoch()
             WHERE project_id = ?1",
            params![project_id, phase],
        )? > 0)
    }

    pub fn list_active_worktree_removals(&self) -> StoreResult<Vec<ActiveWorktreeRemoval>> {
        let mut statement = self.connection.prepare(
            "SELECT project_id, phase, delete_from_disk
             FROM active_worktree_removals
             ORDER BY updated_at, project_id",
        )?;
        Ok(statement
            .query_map([], |row| {
                Ok(ActiveWorktreeRemoval {
                    project_id: row.get(0)?,
                    phase: row.get(1)?,
                    delete_from_disk: row.get(2)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn delete_active_worktree_removal(&self, project_id: ProjectId) -> StoreResult<bool> {
        Ok(self.connection.execute(
            "DELETE FROM active_worktree_removals WHERE project_id = ?1",
            [project_id],
        )? > 0)
    }

    pub fn put_worktree_repository(&self, repository: &WorktreeRepository) -> StoreResult<()> {
        self.connection.execute(
            "INSERT INTO worktree_repositories (id, root_path, name, managed_root)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(id) DO UPDATE SET
                root_path = excluded.root_path,
                name = excluded.name,
                managed_root = excluded.managed_root",
            params![
                repository.id,
                repository.root_path,
                repository.name,
                repository.managed_root,
            ],
        )?;
        Ok(())
    }

    pub fn next_worktree_repository_id(&self) -> StoreResult<WorktreeRepositoryId> {
        Ok(self.connection.query_row(
            "SELECT COALESCE(MAX(id), 0) + 1 FROM worktree_repositories",
            [],
            |row| row.get(0),
        )?)
    }

    pub fn get_worktree_repository(
        &self,
        id: WorktreeRepositoryId,
    ) -> StoreResult<Option<WorktreeRepository>> {
        Ok(self
            .connection
            .query_row(
                "SELECT id, root_path, name, managed_root
                 FROM worktree_repositories WHERE id = ?1",
                [id],
                worktree_repository_from_row,
            )
            .optional()?)
    }

    pub fn get_worktree_repository_by_root(
        &self,
        root_path: &str,
    ) -> StoreResult<Option<WorktreeRepository>> {
        Ok(self
            .connection
            .query_row(
                "SELECT id, root_path, name, managed_root
                 FROM worktree_repositories WHERE root_path = ?1",
                [root_path],
                worktree_repository_from_row,
            )
            .optional()?)
    }

    pub fn list_worktree_repositories(&self) -> StoreResult<Vec<WorktreeRepository>> {
        let mut statement = self.connection.prepare(
            "SELECT id, root_path, name, managed_root
             FROM worktree_repositories ORDER BY id",
        )?;
        Ok(statement
            .query_map([], worktree_repository_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn put_project_worktree(&self, link: &ProjectWorktree) -> StoreResult<()> {
        self.connection.execute(
            "INSERT INTO project_worktrees
                (project_id, repository_id, parent_project_id, branch, managed)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(project_id) DO UPDATE SET
                repository_id = excluded.repository_id,
                parent_project_id = excluded.parent_project_id,
                branch = excluded.branch,
                managed = excluded.managed",
            params![
                link.project_id,
                link.repository_id,
                link.parent_project_id,
                link.branch,
                link.managed,
            ],
        )?;
        Ok(())
    }

    /// Persist a project, its active-profile membership, and its Git-worktree
    /// link as one registration boundary.
    pub fn put_project_with_worktree(
        &self,
        project: &Project,
        link: &ProjectWorktree,
    ) -> StoreResult<()> {
        let transaction = self.connection.unchecked_transaction()?;
        transaction.execute(
            "INSERT INTO projects (id, path, name, display_name, icon, selected, sort_order)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(id) DO UPDATE SET
                path = excluded.path,
                name = excluded.name,
                display_name = excluded.display_name,
                icon = COALESCE(excluded.icon, projects.icon),
                selected = excluded.selected,
                sort_order = excluded.sort_order",
            params![
                project.id,
                project.path,
                project.name,
                project.display_name,
                project.icon,
                project.selected,
                project.sort_order,
            ],
        )?;
        let profile_id: ProfileId =
            transaction.query_row("SELECT id FROM profiles WHERE active = 1", [], |row| {
                row.get(0)
            })?;
        let sort_order =
            crate::project_folders::next_project_top_level_order(&transaction, profile_id)?;
        transaction.execute(
            "INSERT INTO profile_projects (profile_id, project_id, sort_order, selected)
             SELECT ?1, ?2, ?3,
                    CASE WHEN COUNT(*) = 0 THEN 1 ELSE 0 END
             FROM profile_projects WHERE profile_id = ?1
             ON CONFLICT(profile_id, project_id) DO NOTHING",
            params![profile_id, project.id, sort_order],
        )?;
        transaction.execute(
            "INSERT INTO project_worktrees
                (project_id, repository_id, parent_project_id, branch, managed)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(project_id) DO UPDATE SET
                repository_id = excluded.repository_id,
                parent_project_id = excluded.parent_project_id,
                branch = excluded.branch,
                managed = excluded.managed",
            params![
                link.project_id,
                link.repository_id,
                link.parent_project_id,
                link.branch,
                link.managed,
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn get_project_worktree(
        &self,
        project_id: ProjectId,
    ) -> StoreResult<Option<ProjectWorktree>> {
        Ok(self
            .connection
            .query_row(
                "SELECT project_id, repository_id, parent_project_id, branch, managed
                 FROM project_worktrees WHERE project_id = ?1",
                [project_id],
                project_worktree_from_row,
            )
            .optional()?)
    }

    pub fn list_project_worktrees(
        &self,
        repository_id: WorktreeRepositoryId,
    ) -> StoreResult<Vec<ProjectWorktree>> {
        let mut statement = self.connection.prepare(
            "SELECT project_id, repository_id, parent_project_id, branch, managed
             FROM project_worktrees WHERE repository_id = ?1 ORDER BY project_id",
        )?;
        Ok(statement
            .query_map([repository_id], project_worktree_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn set_worktree_preference(
        &self,
        repository_id: WorktreeRepositoryId,
        key: &str,
        value: Option<&str>,
    ) -> StoreResult<()> {
        if let Some(value) = value {
            self.connection.execute(
                "INSERT INTO worktree_preferences (repository_id, key, value)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(repository_id, key) DO UPDATE SET value = excluded.value",
                params![repository_id, key, value],
            )?;
        } else {
            self.connection.execute(
                "DELETE FROM worktree_preferences WHERE repository_id = ?1 AND key = ?2",
                params![repository_id, key],
            )?;
        }
        Ok(())
    }

    pub fn worktree_preferences(
        &self,
        repository_id: WorktreeRepositoryId,
    ) -> StoreResult<std::collections::BTreeMap<String, String>> {
        let mut statement = self.connection.prepare(
            "SELECT key, value FROM worktree_preferences
             WHERE repository_id = ?1 ORDER BY key",
        )?;
        Ok(statement
            .query_map([repository_id], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<std::collections::BTreeMap<_, _>>>()?)
    }

    pub fn put_agent_tool(&self, tool: &AgentTool) -> StoreResult<()> {
        let profile_id = self.active_profile_id()?;
        let storage_name = self
            .connection
            .query_row(
                "SELECT name FROM agent_tools WHERE id = ?1 AND profile_id = ?2",
                params![tool.id, profile_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .unwrap_or_else(|| profile_agent_storage_name(profile_id, tool.id));
        self.connection.execute(
            "INSERT INTO agent_tools (
                id, name, display_name, command, tool_type, enabled, source, sort_order,
                resume_args, continue_args, profile_id
             )
             VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7,
                COALESCE((SELECT MAX(sort_order) + 1 FROM agent_tools WHERE profile_id = ?10), 0),
                ?8, ?9, ?10
             )
             ON CONFLICT(id) DO UPDATE SET
                display_name = excluded.display_name,
                command = excluded.command,
                tool_type = excluded.tool_type,
                enabled = excluded.enabled,
                source = excluded.source,
                resume_args = excluded.resume_args,
                continue_args = excluded.continue_args
             WHERE agent_tools.profile_id = excluded.profile_id",
            params![
                tool.id,
                storage_name,
                tool.name,
                tool.command,
                tool.tool_type,
                tool.enabled,
                tool.source,
                tool.resume_args,
                tool.continue_args,
                profile_id,
            ],
        )?;
        Ok(())
    }

    pub fn get_agent_tool(&self, id: i64) -> StoreResult<Option<AgentTool>> {
        let tool = self
            .connection
            .query_row(
                "SELECT a.id, COALESCE(a.display_name, a.name), a.command, a.tool_type,
                        a.enabled, a.source,
                        resume_args, continue_args
                 FROM agent_tools AS a
                 JOIN profiles AS p ON p.id = a.profile_id AND p.active = 1
                 WHERE a.id = ?1",
                [id],
                |row| {
                    Ok(AgentTool {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        command: row.get(2)?,
                        tool_type: row.get(3)?,
                        enabled: row.get(4)?,
                        source: row.get(5)?,
                        resume_args: row.get(6)?,
                        continue_args: row.get(7)?,
                    })
                },
            )
            .optional()?;
        Ok(tool)
    }

    pub fn list_agent_tools(&self) -> StoreResult<Vec<AgentTool>> {
        let mut statement = self.connection.prepare(
            "SELECT a.id, COALESCE(a.display_name, a.name), a.command, a.tool_type,
                    a.enabled, a.source, a.resume_args, a.continue_args
             FROM agent_tools AS a
             JOIN profiles AS p ON p.id = a.profile_id AND p.active = 1
             ORDER BY a.sort_order, a.id",
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
                    resume_args: row.get(6)?,
                    continue_args: row.get(7)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(tools)
    }

    pub fn reorder_agent_tools(&self, ordered_ids: &[i64]) -> StoreResult<()> {
        let existing = self
            .list_agent_tools()?
            .into_iter()
            .map(|tool| tool.id)
            .collect::<HashSet<_>>();
        let requested = ordered_ids.iter().copied().collect::<HashSet<_>>();
        if requested.len() != ordered_ids.len() || requested != existing {
            return Err(StoreError::InvalidReorder(
                "agent tool order must contain every registered tool exactly once".to_owned(),
            ));
        }
        let transaction = self.connection.unchecked_transaction()?;
        for (position, id) in ordered_ids.iter().enumerate() {
            transaction.execute(
                "UPDATE agent_tools SET sort_order = ?1
                 WHERE id = ?2
                   AND profile_id = (SELECT id FROM profiles WHERE active = 1)",
                params![position as i64, id],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn next_agent_tool_id(&self) -> StoreResult<i64> {
        Ok(self.connection.query_row(
            "SELECT COALESCE(MAX(id), 0) + 1 FROM agent_tools",
            [],
            |row| row.get(0),
        )?)
    }

    pub fn delete_agent_tool(&self, id: i64) -> StoreResult<bool> {
        Ok(self.connection.execute(
            "DELETE FROM agent_tools
             WHERE id = ?1
               AND profile_id = (SELECT id FROM profiles WHERE active = 1)",
            [id],
        )? > 0)
    }

    pub fn list_profile_quick_prompts(
        &self,
        profile_id: ProfileId,
    ) -> StoreResult<Vec<QuickPrompt>> {
        let mut statement = self.connection.prepare(
            "SELECT id, name, body, sort_order, created_at, updated_at
             FROM quick_prompts WHERE profile_id = ?1 ORDER BY sort_order, id",
        )?;
        Ok(statement
            .query_map([profile_id], quick_prompt_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn list_quick_prompts(&self) -> StoreResult<Vec<QuickPrompt>> {
        self.list_profile_quick_prompts(self.active_profile_id()?)
    }

    pub fn get_quick_prompt(&self, id: i64) -> StoreResult<Option<QuickPrompt>> {
        Ok(self
            .connection
            .query_row(
                "SELECT q.id, q.name, q.body, q.sort_order, q.created_at, q.updated_at
                 FROM quick_prompts AS q
                 JOIN profiles AS p ON p.id = q.profile_id AND p.active = 1
                 WHERE q.id = ?1",
                [id],
                quick_prompt_from_row,
            )
            .optional()?)
    }

    pub fn list_agent_templates(&self) -> StoreResult<Vec<AgentTemplate>> {
        self.list_profile_agent_templates(self.active_profile_id()?)
    }

    pub fn get_agent_template(&self, id: i64) -> StoreResult<Option<AgentTemplate>> {
        Ok(self
            .connection
            .query_row(
                "SELECT t.id, t.profile_id, t.name, t.agent_tool_id, t.extra_args, t.prompt,
                        t.sort_order, t.created_at, t.updated_at
                 FROM agent_templates AS t
                 JOIN profiles AS p ON p.id = t.profile_id AND p.active = 1
                WHERE t.id = ?1",
                [id],
                agent_template_from_row,
            )
            .optional()?)
    }

    pub fn put_quick_prompt(&self, prompt: &QuickPrompt) -> StoreResult<()> {
        let profile_id = self.active_profile_id()?;
        self.connection.execute(
            "INSERT INTO quick_prompts (
                id, profile_id, name, body, sort_order, created_at, updated_at
             ) VALUES (
                ?1, ?4, ?2, ?3,
                COALESCE((SELECT MAX(sort_order) + 1 FROM quick_prompts WHERE profile_id = ?4), 0),
                unixepoch(), unixepoch()
             )
             ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                body = excluded.body,
                updated_at = unixepoch()
             WHERE quick_prompts.profile_id = excluded.profile_id",
            params![prompt.id, prompt.name, prompt.body, profile_id],
        )?;
        Ok(())
    }

    pub fn put_agent_template(&self, template: &AgentTemplate) -> StoreResult<()> {
        let tool_profile_id = self
            .connection
            .query_row(
                "SELECT profile_id FROM agent_tools WHERE id = ?1",
                [template.agent_tool_id],
                |row| row.get::<_, Option<ProfileId>>(0),
            )
            .optional()?
            .flatten();
        if tool_profile_id != Some(template.profile_id) {
            return Err(StoreError::InvalidProfile(format!(
                "agent tool {} does not belong to profile {}",
                template.agent_tool_id, template.profile_id
            )));
        }
        self.connection.execute(
            "INSERT INTO agent_templates (
                id, profile_id, name, agent_tool_id, extra_args, prompt, sort_order,
                created_at, updated_at
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6,
                COALESCE((SELECT MAX(sort_order) + 1 FROM agent_templates WHERE profile_id = ?2), 0),
                unixepoch(), unixepoch()
             )
             ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                agent_tool_id = excluded.agent_tool_id,
                extra_args = excluded.extra_args,
                prompt = excluded.prompt,
                updated_at = unixepoch()
             WHERE agent_templates.profile_id = excluded.profile_id",
            params![
                template.id,
                template.profile_id,
                template.name,
                template.agent_tool_id,
                to_json(&template.extra_args)?,
                template.prompt,
            ],
        )?;
        Ok(())
    }

    pub fn reorder_quick_prompts(&self, ordered_ids: &[i64]) -> StoreResult<()> {
        let existing = self
            .list_quick_prompts()?
            .into_iter()
            .map(|prompt| prompt.id)
            .collect::<HashSet<_>>();
        let requested = ordered_ids.iter().copied().collect::<HashSet<_>>();
        if requested.len() != ordered_ids.len() || requested != existing {
            return Err(StoreError::InvalidReorder(
                "quick prompt order must contain every saved prompt exactly once".to_owned(),
            ));
        }
        let transaction = self.connection.unchecked_transaction()?;
        for (position, id) in ordered_ids.iter().enumerate() {
            transaction.execute(
                "UPDATE quick_prompts SET sort_order = ?1, updated_at = unixepoch()
                 WHERE id = ?2
                   AND profile_id = (SELECT id FROM profiles WHERE active = 1)",
                params![position as i64, id],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn next_quick_prompt_id(&self) -> StoreResult<i64> {
        Ok(self.connection.query_row(
            "SELECT COALESCE(MAX(id), 0) + 1 FROM quick_prompts",
            [],
            |row| row.get(0),
        )?)
    }

    pub fn delete_quick_prompt(&self, id: i64) -> StoreResult<bool> {
        Ok(self.connection.execute(
            "DELETE FROM quick_prompts
             WHERE id = ?1
               AND profile_id = (SELECT id FROM profiles WHERE active = 1)",
            [id],
        )? > 0)
    }

    pub fn next_agent_template_id(&self) -> StoreResult<i64> {
        Ok(self.connection.query_row(
            "SELECT COALESCE(MAX(id), 0) + 1 FROM agent_templates",
            [],
            |row| row.get(0),
        )?)
    }

    pub fn reorder_agent_templates(&self, ordered_ids: &[i64]) -> StoreResult<()> {
        let profile_id = self.active_profile_id()?;
        let existing = self
            .list_profile_agent_templates(profile_id)?
            .into_iter()
            .map(|template| template.id)
            .collect::<HashSet<_>>();
        let requested = ordered_ids.iter().copied().collect::<HashSet<_>>();
        if requested.len() != ordered_ids.len() || requested != existing {
            return Err(StoreError::InvalidReorder(
                "agent template order must contain every template exactly once".to_owned(),
            ));
        }
        let transaction = self.connection.unchecked_transaction()?;
        for (position, id) in ordered_ids.iter().enumerate() {
            transaction.execute(
                "UPDATE agent_templates SET sort_order = ?1, updated_at = unixepoch()
                 WHERE id = ?2 AND profile_id = ?3",
                params![position as i64, id, profile_id],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn delete_agent_template(&self, id: i64) -> StoreResult<bool> {
        Ok(self.connection.execute(
            "DELETE FROM agent_templates
             WHERE id = ?1
               AND profile_id = (SELECT id FROM profiles WHERE active = 1)",
            [id],
        )? > 0)
    }

    /// Persist the strategy used for the latest agent launch while retaining any captured ID.
    pub fn set_agent_launch_mode(
        &self,
        process_id: ProcessId,
        launch_mode: AgentLaunchMode,
        launched_at: i64,
    ) -> StoreResult<()> {
        self.connection.execute(
            "INSERT INTO process_agent_sessions (
                process_id, session_id, launch_mode, launched_at, captured_at
             ) VALUES (?1, NULL, ?2, ?3, NULL)
             ON CONFLICT(process_id) DO UPDATE SET
                launch_mode = excluded.launch_mode,
                launched_at = excluded.launched_at",
            params![process_id, launch_mode, launched_at],
        )?;
        Ok(())
    }

    /// Attach a passively discovered CLI conversation ID to an agent process.
    pub fn set_agent_session_id(
        &self,
        process_id: ProcessId,
        session_id: &str,
        captured_at: i64,
    ) -> StoreResult<bool> {
        let updated = self.connection.execute(
            "UPDATE process_agent_sessions
             SET session_id = ?2, captured_at = ?3
             WHERE process_id = ?1
               AND NOT EXISTS (
                   SELECT 1 FROM process_agent_sessions AS claimed
                   WHERE claimed.session_id = ?2
                     AND claimed.process_id <> ?1
               )",
            params![process_id, session_id, captured_at],
        )?;
        Ok(updated > 0)
    }

    pub fn get_agent_session(&self, process_id: ProcessId) -> StoreResult<Option<AgentSession>> {
        Ok(self
            .connection
            .query_row(
                "SELECT process_id, session_id, launch_mode, launched_at, captured_at
                 FROM process_agent_sessions WHERE process_id = ?1",
                [process_id],
                |row| {
                    Ok(AgentSession {
                        process_id: row.get(0)?,
                        session_id: row.get(1)?,
                        launch_mode: row.get(2)?,
                        launched_at: row.get(3)?,
                        captured_at: row.get(4)?,
                    })
                },
            )
            .optional()?)
    }

    pub fn put_process(&self, process: &Process) -> StoreResult<()> {
        let env = to_json(&process.env)?;
        let restart_when_changed = to_json(&process.restart_when_changed)?;
        self.connection.execute(
            "INSERT INTO processes (
                id, project_id, kind, name, command, working_dir, env, auto_start,
                auto_restart, restart_when_changed, source, trust_hash, status, pid,
                exit_code, exit_signal, exited_at, agent_tool_id, spawned_by_process_id, sort_order
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
                ?15, ?16, ?17, ?18, ?19, ?20
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
                agent_tool_id = excluded.agent_tool_id,
                spawned_by_process_id = excluded.spawned_by_process_id,
                sort_order = excluded.sort_order",
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
                process.spawned_by_process_id,
                process.sort_order,
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
                        exit_code, exit_signal, exited_at, agent_tool_id, spawned_by_process_id,
                        sort_order
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
                        exit_code, exit_signal, exited_at, agent_tool_id, spawned_by_process_id,
                        sort_order
                 FROM processes WHERE project_id = ?1
                 ORDER BY CASE kind WHEN 'agent' THEN 0 WHEN 'terminal' THEN 1 ELSE 2 END,
                          sort_order, id",
                Some(project_id),
            ),
            None => (
                "SELECT id, project_id, kind, name, command, working_dir, env, auto_start,
                        auto_restart, restart_when_changed, source, trust_hash, status, pid,
                        exit_code, exit_signal, exited_at, agent_tool_id, spawned_by_process_id,
                        sort_order
                 FROM processes
                 ORDER BY project_id,
                          CASE kind WHEN 'agent' THEN 0 WHEN 'terminal' THEN 1 ELSE 2 END,
                          sort_order, id",
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

    pub fn list_active_profile_processes(&self) -> StoreResult<Vec<Process>> {
        let mut statement = self.connection.prepare(
            "SELECT process.id, process.project_id, process.kind, process.name, process.command,
                    process.working_dir, process.env, process.auto_start, process.auto_restart,
                    process.restart_when_changed, process.source, process.trust_hash,
                    process.status, process.pid, process.exit_code, process.exit_signal,
                    process.exited_at, process.agent_tool_id, process.spawned_by_process_id,
                    process.sort_order
             FROM processes AS process
             JOIN profile_projects AS pp ON pp.project_id = process.project_id
             JOIN profiles AS profile ON profile.id = pp.profile_id AND profile.active = 1
             ORDER BY pp.sort_order,
                      CASE process.kind WHEN 'agent' THEN 0 WHEN 'terminal' THEN 1 ELSE 2 END,
                      process.sort_order, process.id",
        )?;
        Ok(statement
            .query_map([], process_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?)
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

    pub fn next_process_sort_order(
        &self,
        project_id: ProjectId,
        kind: ProcessKind,
    ) -> StoreResult<i64> {
        Ok(self.connection.query_row(
            "SELECT COALESCE(MAX(sort_order), -1) + 1
             FROM processes WHERE project_id = ?1 AND kind = ?2",
            params![project_id, kind],
            |row| row.get(0),
        )?)
    }

    /// Replace the complete order for one project/kind group in one transaction.
    pub fn reorder_processes(
        &mut self,
        project_id: ProjectId,
        kind: ProcessKind,
        ordered_ids: &[ProcessId],
    ) -> StoreResult<Vec<Process>> {
        let current = self
            .list_processes(Some(project_id))?
            .into_iter()
            .filter(|process| process.kind == kind)
            .map(|process| process.id)
            .collect::<Vec<_>>();
        validate_reorder_ids("process", &current, ordered_ids)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        for (sort_order, process_id) in ordered_ids.iter().enumerate() {
            transaction.execute(
                "UPDATE processes SET sort_order = ?1
                 WHERE id = ?2 AND project_id = ?3 AND kind = ?4",
                params![sort_order as i64, process_id, project_id, kind],
            )?;
        }
        transaction.commit()?;
        Ok(self
            .list_processes(Some(project_id))?
            .into_iter()
            .filter(|process| process.kind == kind)
            .collect())
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
                        p.agent_tool_id, p.spawned_by_process_id, p.sort_order
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
                id, project_id, title, body, status, priority, completed, lock_actor,
                lock_process_id, lock_expiry
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(id) DO UPDATE SET
                project_id = excluded.project_id,
                title = excluded.title,
                body = excluded.body,
                status = excluded.status,
                priority = excluded.priority,
                completed = excluded.completed,
                lock_actor = excluded.lock_actor,
                lock_process_id = excluded.lock_process_id,
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
                todo.lock_process_id,
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
                        lock_actor, lock_process_id, lock_expiry
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
                        lock_process_id: row.get(8)?,
                        lock_expiry: row.get(9)?,
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
            "INSERT INTO scratchpads (
                id, project_id, name, content, revision, archived, created_by, updated_by
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(id) DO UPDATE SET
                project_id = excluded.project_id,
                name = excluded.name,
                content = excluded.content,
                revision = excluded.revision,
                archived = excluded.archived,
                created_by = excluded.created_by,
                updated_by = excluded.updated_by",
            params![
                scratchpad.id,
                scratchpad.project_id,
                scratchpad.name,
                scratchpad.content,
                scratchpad.revision,
                scratchpad.archived,
                scratchpad.created_by,
                scratchpad.updated_by,
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
                "SELECT id, project_id, name, content, revision, archived, created_by, updated_by
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
                        created_by: row.get(6)?,
                        updated_by: row.get(7)?,
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
            "INSERT INTO locks (project_id, key, owner_actor, owner_process_id, acquired_at, ttl)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(project_id, key) DO UPDATE SET
                owner_actor = excluded.owner_actor,
                owner_process_id = excluded.owner_process_id,
                acquired_at = excluded.acquired_at,
                ttl = excluded.ttl",
            params![
                lock.project_id,
                lock.key,
                lock.owner_actor,
                lock.owner_process_id,
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
                "SELECT project_id, key, owner_actor, owner_process_id, acquired_at, ttl
                 FROM locks WHERE project_id = ?1 AND key = ?2",
                params![project_id, key],
                |row| {
                    Ok(ProjectLock {
                        project_id: row.get(0)?,
                        key: row.get(1)?,
                        owner_actor: row.get(2)?,
                        owner_process_id: row.get(3)?,
                        acquired_at: row.get(4)?,
                        ttl_ms: row.get(5)?,
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
                id, owner_actor, owner_process_id, delivery_process_id, body, kind, watch_list,
                interval, loop, max_wait_deadline, paused, fired, fired_at, created_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
             ON CONFLICT(id) DO UPDATE SET
                owner_actor = excluded.owner_actor,
                owner_process_id = excluded.owner_process_id,
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
                timer.owner_process_id,
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
                "SELECT id, owner_actor, owner_process_id, delivery_process_id, body, kind,
                        watch_list, interval, loop, max_wait_deadline, paused, fired, fired_at, created_at
                 FROM timers WHERE id = ?1",
                [id],
                |row| {
                    Ok(Timer {
                        id: row.get(0)?,
                        owner_actor: row.get(1)?,
                        owner_process_id: row.get(2)?,
                        delivery_process_id: row.get(3)?,
                        body: row.get(4)?,
                        kind: row.get(5)?,
                        watch_process_ids: json_from_row(row, 6)?,
                        interval_ms: row.get(7)?,
                        repeating: row.get(8)?,
                        max_wait_deadline: row.get(9)?,
                        paused: row.get(10)?,
                        fired: row.get(11)?,
                        fired_at: row.get(12)?,
                        created_at: row.get(13)?,
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

    /// Resolve actor attribution through the actor's current process name.
    /// Process-less MCP sessions deliberately receive a neutral label rather than exposing IDs.
    pub fn actor_display_label(&self, actor_id: &str) -> String {
        if matches!(actor_id, "desktop-ui" | "workman" | "user") {
            return "user".into();
        }
        if !actor_id.starts_with("mcp-") {
            return actor_id.to_string();
        }

        let actor = self.get_actor(actor_id).ok().flatten();
        let process = actor
            .and_then(|actor| actor.process_id)
            .and_then(|process_id| self.get_process(process_id).ok().flatten());
        if let Some(process) = process {
            return process.name;
        }

        "session".into()
    }

    /// Resolve durable ownership through a process ID before falling back to actor attribution.
    pub fn ownership_display_label(&self, actor_id: &str, process_id: Option<ProcessId>) -> String {
        process_id
            .and_then(|process_id| self.get_process(process_id).ok().flatten())
            .map(|process| process.name)
            .unwrap_or_else(|| self.actor_display_label(actor_id))
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
        spawned_by_process_id: row.get(18)?,
        sort_order: row.get(19)?,
    })
}

fn agent_template_from_row(row: &Row<'_>) -> rusqlite::Result<AgentTemplate> {
    Ok(AgentTemplate {
        id: row.get(0)?,
        profile_id: row.get(1)?,
        name: row.get(2)?,
        agent_tool_id: row.get(3)?,
        extra_args: json_from_row(row, 4)?,
        prompt: row.get(5)?,
        sort_order: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

fn profile_from_row(row: &Row<'_>) -> rusqlite::Result<Profile> {
    let project_count: i64 = row.get(3)?;
    let agent_tool_count: i64 = row.get(4)?;
    Ok(Profile {
        id: row.get(0)?,
        name: row.get(1)?,
        active: row.get(2)?,
        project_count: usize::try_from(project_count).unwrap_or(usize::MAX),
        agent_tool_count: usize::try_from(agent_tool_count).unwrap_or(usize::MAX),
        created_at: row.get(5)?,
    })
}

fn quick_prompt_from_row(row: &Row<'_>) -> rusqlite::Result<QuickPrompt> {
    Ok(QuickPrompt {
        id: row.get(0)?,
        name: row.get(1)?,
        body: row.get(2)?,
        sort_order: row.get(3)?,
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
    })
}

fn project_from_row(row: &Row<'_>) -> rusqlite::Result<Project> {
    Ok(Project {
        id: row.get(0)?,
        path: row.get(1)?,
        name: row.get(2)?,
        display_name: row.get(3)?,
        icon: row.get(4)?,
        selected: row.get(5)?,
        sort_order: row.get(6)?,
    })
}

fn normalized_profile_name(name: &str) -> StoreResult<String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(StoreError::InvalidProfile(
            "profile name cannot be empty".into(),
        ));
    }
    if name.chars().count() > 80 || name.chars().any(char::is_control) {
        return Err(StoreError::InvalidProfile(
            "profile name must be at most 80 visible characters".into(),
        ));
    }
    Ok(name.to_owned())
}

fn profile_agent_storage_name(profile_id: ProfileId, agent_tool_id: i64) -> String {
    format!("profile-{profile_id}-tool-{agent_tool_id}")
}

fn project_name_from_path(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("Project")
        .to_owned()
}

fn worktree_repository_from_row(row: &Row<'_>) -> rusqlite::Result<WorktreeRepository> {
    Ok(WorktreeRepository {
        id: row.get(0)?,
        root_path: row.get(1)?,
        name: row.get(2)?,
        managed_root: row.get(3)?,
    })
}

fn project_worktree_from_row(row: &Row<'_>) -> rusqlite::Result<ProjectWorktree> {
    Ok(ProjectWorktree {
        project_id: row.get(0)?,
        repository_id: row.get(1)?,
        parent_project_id: row.get(2)?,
        branch: row.get(3)?,
        managed: row.get(4)?,
    })
}

fn validate_reorder_ids(label: &str, current_ids: &[i64], ordered_ids: &[i64]) -> StoreResult<()> {
    let unique = ordered_ids.iter().copied().collect::<HashSet<_>>();
    let current = current_ids.iter().copied().collect::<HashSet<_>>();
    if ordered_ids.len() != current_ids.len()
        || unique.len() != ordered_ids.len()
        || unique != current
    {
        return Err(StoreError::InvalidReorder(format!(
            "{label} reorder must contain every scoped ID exactly once"
        )));
    }
    Ok(())
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
