//! Todo graph, comments, and lease coordination over the SQLite store.

use std::{collections::HashSet, error::Error, fmt};

use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

use crate::{
    NotificationType, ProjectId, Store, StoreError, Todo, TodoComment, TodoCommentId, TodoId,
    TodoPriority, TodoStatus,
};

const DEFAULT_PAGE_SIZE: usize = 50;
const MAX_PAGE_SIZE: usize = 200;
pub const USER_ASSIGNEE: &str = "user";
pub const USER_MENTION: &str = "@user";

/// Hydrated todo data returned by the service and rich MCP responses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TodoView {
    pub id: TodoId,
    pub project_id: ProjectId,
    pub title: String,
    pub body: String,
    pub priority: TodoPriority,
    pub status: TodoStatus,
    pub completed: bool,
    pub sort_order: i64,
    pub assignee: Option<String>,
    pub locked_by: Option<String>,
    pub lock_expiry: Option<i64>,
    pub comment_count: usize,
    pub tags: Vec<String>,
    pub blocker_ids: Vec<TodoId>,
    pub is_blocked: bool,
    pub unresolved_blocker_count: usize,
}

/// Compact todo representation used by list responses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TodoSummary {
    pub id: TodoId,
    pub project_id: ProjectId,
    pub title: String,
    pub body_chars: usize,
    pub priority: TodoPriority,
    pub status: TodoStatus,
    pub completed: bool,
    pub sort_order: i64,
    pub assignee: Option<String>,
    pub locked_by: Option<String>,
    pub comment_count: usize,
    pub tags: Vec<String>,
    pub blocker_ids: Vec<TodoId>,
    pub is_blocked: bool,
    pub unresolved_blocker_count: usize,
}

impl From<TodoView> for TodoSummary {
    fn from(todo: TodoView) -> Self {
        Self {
            id: todo.id,
            project_id: todo.project_id,
            title: todo.title,
            body_chars: todo.body.chars().count(),
            priority: todo.priority,
            status: todo.status,
            completed: todo.completed,
            sort_order: todo.sort_order,
            assignee: todo.assignee,
            locked_by: todo.locked_by,
            comment_count: todo.comment_count,
            tags: todo.tags,
            blocker_ids: todo.blocker_ids,
            is_blocked: todo.is_blocked,
            unresolved_blocker_count: todo.unresolved_blocker_count,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TodoPage {
    pub todos: Vec<TodoSummary>,
    pub total_count: usize,
    pub offset: usize,
    pub limit: usize,
    pub has_more: bool,
    pub next_offset: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TodoCommentPage {
    pub comments: Vec<TodoComment>,
    pub total_count: usize,
    pub offset: usize,
    pub limit: usize,
    pub has_more: bool,
    pub next_offset: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TodoActivityKind {
    Created,
    Completed,
    Reopened,
    Locked,
    Unlocked,
}

impl TodoActivityKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Completed => "completed",
            Self::Reopened => "reopened",
            Self::Locked => "locked",
            Self::Unlocked => "unlocked",
        }
    }

    fn parse(value: &str) -> TodoServiceResult<Self> {
        match value {
            "created" => Ok(Self::Created),
            "completed" => Ok(Self::Completed),
            "reopened" => Ok(Self::Reopened),
            "locked" => Ok(Self::Locked),
            "unlocked" => Ok(Self::Unlocked),
            value => Err(TodoServiceError::Store(format!(
                "unknown todo activity kind {value}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TodoActivity {
    pub id: i64,
    pub todo_id: TodoId,
    pub actor: String,
    pub kind: TodoActivityKind,
    pub created_at: i64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TodoSort {
    /// High priority first, then newest IDs first.
    #[default]
    Priority,
    Newest,
    Oldest,
    TitleAsc,
    TitleDesc,
    Status,
}

#[derive(Debug, Clone, Default)]
pub struct TodoListQuery {
    pub status: Option<TodoStatus>,
    pub completed: Option<bool>,
    pub is_blocked: Option<bool>,
    pub priority: Option<TodoPriority>,
    pub assignee: Option<String>,
    pub query: Option<String>,
    pub tags: Vec<String>,
    pub sort: TodoSort,
    pub offset: usize,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct NewTodo {
    pub title: String,
    pub body: String,
    pub priority: TodoPriority,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct UpdateTodo {
    pub title: Option<String>,
    pub body: Option<String>,
    pub priority: Option<TodoPriority>,
    pub status: Option<TodoStatus>,
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TodoServiceError {
    Store(String),
    ProjectNotFound(ProjectId),
    TodoNotFound(TodoId),
    CommentNotFound(TodoCommentId),
    InvalidInput(String),
    BlockerCycle { todo_id: TodoId, blocker_id: TodoId },
    Locked { todo_id: TodoId, owner: String },
    LockNotOwned(TodoId),
}

impl TodoServiceError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Store(_) => "store_error",
            Self::ProjectNotFound(_) => "project_not_found",
            Self::TodoNotFound(_) => "todo_not_found",
            Self::CommentNotFound(_) => "todo_comment_not_found",
            Self::InvalidInput(_) => "invalid_todo_input",
            Self::BlockerCycle { .. } => "todo_blocker_cycle",
            Self::Locked { .. } => "todo_locked",
            Self::LockNotOwned(_) => "todo_lock_not_owned",
        }
    }
}

impl fmt::Display for TodoServiceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(message) | Self::InvalidInput(message) => formatter.write_str(message),
            Self::ProjectNotFound(id) => write!(formatter, "project {id} was not found"),
            Self::TodoNotFound(id) => write!(formatter, "todo {id} was not found in this project"),
            Self::CommentNotFound(id) => write!(formatter, "todo comment {id} was not found"),
            Self::BlockerCycle {
                todo_id,
                blocker_id,
            } => write!(
                formatter,
                "making todo {todo_id} depend on {blocker_id} would create a cycle"
            ),
            Self::Locked { todo_id, owner } => {
                write!(formatter, "todo {todo_id} is locked by {owner}")
            }
            Self::LockNotOwned(id) => write!(formatter, "this actor does not own todo {id}'s lock"),
        }
    }
}

impl Error for TodoServiceError {}

impl From<StoreError> for TodoServiceError {
    fn from(error: StoreError) -> Self {
        Self::Store(error.to_string())
    }
}

impl From<rusqlite::Error> for TodoServiceError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Store(error.to_string())
    }
}

pub type TodoServiceResult<T> = Result<T, TodoServiceError>;

/// Internal service shared by MCP and future control/UI adapters.
pub struct TodoService<'store> {
    store: &'store Store,
}

impl<'store> TodoService<'store> {
    pub fn new(store: &'store Store) -> Self {
        Self { store }
    }

    pub fn create(
        &self,
        project_id: ProjectId,
        todo: NewTodo,
        now_ms: i64,
    ) -> TodoServiceResult<TodoView> {
        self.require_project(project_id)?;
        validate_title(&todo.title)?;
        let tags = normalize_tags(todo.tags)?;
        let transaction = self.store.connection().unchecked_transaction()?;
        transaction.execute(
            "INSERT INTO todos (
                project_id, title, body, status, priority, completed, sort_order
             ) VALUES (
                ?1, ?2, ?3, 'open', ?4, 0,
                (SELECT COALESCE(MAX(sort_order), -1) + 1 FROM todos WHERE project_id = ?1)
             )",
            params![project_id, todo.title, todo.body, todo.priority],
        )?;
        let todo_id = transaction.last_insert_rowid();
        replace_tags(&transaction, todo_id, &tags)?;
        transaction.execute(
            "INSERT INTO todo_activity (todo_id, actor, kind, created_at)
             VALUES (?1, 'workman', 'created', ?2)",
            params![todo_id, now_ms],
        )?;
        transaction.commit()?;
        self.require_todo(project_id, todo_id, now_ms)
    }

    pub fn get(
        &self,
        project_id: ProjectId,
        todo_id: TodoId,
        now_ms: i64,
    ) -> TodoServiceResult<Option<TodoView>> {
        let Some(todo) = self.store.get_todo(todo_id)? else {
            return Ok(None);
        };
        if todo.project_id != project_id {
            return Ok(None);
        }
        self.hydrate(todo, now_ms).map(Some)
    }

    pub fn update(
        &self,
        project_id: ProjectId,
        todo_id: TodoId,
        update: UpdateTodo,
        now_ms: i64,
    ) -> TodoServiceResult<TodoView> {
        let current = self.require_todo(project_id, todo_id, now_ms)?;
        if let Some(title) = &update.title {
            validate_title(title)?;
        }
        let tags = update.tags.map(normalize_tags).transpose()?;
        let transaction = self.store.connection().unchecked_transaction()?;
        if let Some(title) = update.title {
            transaction.execute(
                "UPDATE todos SET title = ?1 WHERE id = ?2",
                params![title, todo_id],
            )?;
        }
        if let Some(body) = update.body {
            transaction.execute(
                "UPDATE todos SET body = ?1 WHERE id = ?2",
                params![body, todo_id],
            )?;
        }
        if let Some(priority) = update.priority {
            transaction.execute(
                "UPDATE todos SET priority = ?1 WHERE id = ?2",
                params![priority, todo_id],
            )?;
        }
        if let Some(status) = update.status {
            let completed = status == TodoStatus::Completed;
            transaction.execute(
                "UPDATE todos SET status = ?1, completed = ?2 WHERE id = ?3",
                params![status, completed, todo_id],
            )?;
            if status != current.status {
                let kind = if completed {
                    Some(TodoActivityKind::Completed)
                } else if current.status == TodoStatus::Completed {
                    Some(TodoActivityKind::Reopened)
                } else {
                    // Activity intentionally records lifecycle changes, not ordinary workflow-state edits.
                    None
                };
                if let Some(kind) = kind {
                    transaction.execute(
                        "INSERT INTO todo_activity (todo_id, actor, kind, created_at)
                         VALUES (?1, 'workman', ?2, ?3)",
                        params![todo_id, kind.as_str(), now_ms],
                    )?;
                }
            }
        }
        if let Some(tags) = tags {
            replace_tags(&transaction, todo_id, &tags)?;
        }
        transaction.commit()?;
        self.require_todo(project_id, todo_id, now_ms)
    }

    /// Assign a todo to the supported human principal, or clear its assignment.
    ///
    /// Repeating the same assignment is a no-op. Clearing and later reassigning creates a fresh
    /// notification edge. The nullable string column intentionally leaves room for agent
    /// principals without conflating assignment with an edit lock.
    pub fn assign(
        &self,
        project_id: ProjectId,
        todo_id: TodoId,
        assignee: Option<String>,
        actor: &str,
        now_ms: i64,
    ) -> TodoServiceResult<TodoView> {
        let current = self.require_todo(project_id, todo_id, now_ms)?;
        let assignee = normalize_assignee(assignee)?;
        if current.assignee == assignee {
            return Ok(current);
        }

        let actor = actor.trim();
        let actor = if actor.is_empty() { "Workman" } else { actor };
        let transaction = self.store.connection().unchecked_transaction()?;
        transaction.execute(
            "UPDATE todos SET assignee = ?1 WHERE id = ?2 AND project_id = ?3",
            params![assignee.as_deref(), todo_id, project_id],
        )?;
        if assignee.as_deref() == Some(USER_ASSIGNEE) {
            transaction.execute(
                "INSERT INTO notifications
                    (type, project_id, todo_id, body, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    NotificationType::TodoAssignedToYou.as_str(),
                    project_id,
                    todo_id,
                    format!("{actor} assigned ‘{}’ to you.", current.title),
                    now_ms,
                ],
            )?;
        }
        transaction.commit()?;
        self.require_todo(project_id, todo_id, now_ms)
    }

    pub fn delete(
        &self,
        project_id: ProjectId,
        todo_id: TodoId,
        now_ms: i64,
    ) -> TodoServiceResult<Vec<TodoId>> {
        self.require_todo(project_id, todo_id, now_ms)?;
        let affected = dependent_ids(self.store.connection(), todo_id)?;
        self.store
            .connection()
            .execute("DELETE FROM todos WHERE id = ?1", [todo_id])?;
        Ok(affected)
    }

    pub fn list(
        &self,
        project_id: ProjectId,
        query: TodoListQuery,
        now_ms: i64,
    ) -> TodoServiceResult<TodoPage> {
        self.require_project(project_id)?;
        let tags = normalize_tags(query.tags)?;
        let needle = query
            .query
            .map(|value| value.trim().to_lowercase())
            .filter(|value| !value.is_empty());
        let mut statement = self
            .store
            .connection()
            .prepare("SELECT id FROM todos WHERE project_id = ?1 ORDER BY id")?;
        let ids = statement
            .query_map([project_id], |row| row.get::<_, TodoId>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(statement);

        let mut todos = Vec::with_capacity(ids.len());
        for id in ids {
            let todo = self.require_todo(project_id, id, now_ms)?;
            let matches_query = match &needle {
                Some(needle) => self.todo_matches_query(&todo, needle)?,
                None => true,
            };
            if query.status.is_some_and(|status| todo.status != status)
                || query
                    .completed
                    .is_some_and(|completed| todo.completed != completed)
                || query
                    .is_blocked
                    .is_some_and(|is_blocked| todo.is_blocked != is_blocked)
                || query
                    .priority
                    .is_some_and(|priority| todo.priority != priority)
                || query
                    .assignee
                    .as_ref()
                    .is_some_and(|assignee| todo.assignee.as_ref() != Some(assignee))
                || (!tags.is_empty() && !tags.iter().any(|tag| todo.tags.contains(tag)))
                || !matches_query
            {
                continue;
            }
            todos.push(todo);
        }
        sort_todos(&mut todos, query.sort);

        let total_count = todos.len();
        let limit = query
            .limit
            .unwrap_or(DEFAULT_PAGE_SIZE)
            .clamp(1, MAX_PAGE_SIZE);
        let offset = query.offset.min(total_count);
        let end = offset.saturating_add(limit).min(total_count);
        let has_more = end < total_count;
        let todos = todos[offset..end]
            .iter()
            .cloned()
            .map(TodoSummary::from)
            .collect();
        Ok(TodoPage {
            todos,
            total_count,
            offset,
            limit,
            has_more,
            next_offset: has_more.then_some(end),
        })
    }

    /// Replace the manual order of open todos while preserving completed-row slots.
    pub fn reorder(
        &self,
        project_id: ProjectId,
        ordered_ids: &[TodoId],
        now_ms: i64,
    ) -> TodoServiceResult<Vec<TodoSummary>> {
        self.require_project(project_id)?;
        let mut statement = self.store.connection().prepare(
            "SELECT id, sort_order FROM todos
             WHERE project_id = ?1 AND completed = 0
             ORDER BY sort_order, id",
        )?;
        let current = statement
            .query_map([project_id], |row| {
                Ok((row.get::<_, TodoId>(0)?, row.get::<_, i64>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(statement);
        validate_reorder_ids(
            "todo",
            &current.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            ordered_ids,
        )?;

        let transaction = self.store.connection().unchecked_transaction()?;
        for ((_, sort_order), todo_id) in current.iter().zip(ordered_ids) {
            transaction.execute(
                "UPDATE todos SET sort_order = ?1
                 WHERE id = ?2 AND project_id = ?3 AND completed = 0",
                params![sort_order, todo_id, project_id],
            )?;
        }
        transaction.commit()?;

        ordered_ids
            .iter()
            .map(|id| {
                self.require_todo(project_id, *id, now_ms)
                    .map(TodoSummary::from)
            })
            .collect()
    }

    pub fn tags_list(&self, project_id: ProjectId) -> TodoServiceResult<Vec<String>> {
        self.require_project(project_id)?;
        let mut statement = self.store.connection().prepare(
            "SELECT tt.tag, COUNT(*) AS uses, MIN(t.id) AS first_id
             FROM todo_tags AS tt
             JOIN todos AS t ON t.id = tt.todo_id
             WHERE t.project_id = ?1
             GROUP BY tt.tag
             ORDER BY uses DESC, first_id, tt.tag",
        )?;
        let tags = statement
            .query_map([project_id], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(tags)
    }

    pub fn add_tag(
        &self,
        project_id: ProjectId,
        todo_id: TodoId,
        tag: String,
        now_ms: i64,
    ) -> TodoServiceResult<TodoView> {
        self.require_todo(project_id, todo_id, now_ms)?;
        let tag = normalize_tag(tag)?;
        self.store.connection().execute(
            "INSERT INTO todo_tags (todo_id, tag, position)
             SELECT ?1, ?2, COALESCE(MAX(position), -1) + 1 FROM todo_tags WHERE todo_id = ?1
             ON CONFLICT(todo_id, tag) DO NOTHING",
            params![todo_id, tag],
        )?;
        self.require_todo(project_id, todo_id, now_ms)
    }

    pub fn remove_tag(
        &self,
        project_id: ProjectId,
        todo_id: TodoId,
        tag: String,
        now_ms: i64,
    ) -> TodoServiceResult<TodoView> {
        self.require_todo(project_id, todo_id, now_ms)?;
        let tag = normalize_tag(tag)?;
        let transaction = self.store.connection().unchecked_transaction()?;
        transaction.execute(
            "DELETE FROM todo_tags WHERE todo_id = ?1 AND tag = ?2",
            params![todo_id, tag],
        )?;
        compact_tag_positions(&transaction, todo_id)?;
        transaction.commit()?;
        self.require_todo(project_id, todo_id, now_ms)
    }

    pub fn set_blockers(
        &self,
        project_id: ProjectId,
        todo_id: TodoId,
        blocker_ids: Vec<TodoId>,
        now_ms: i64,
    ) -> TodoServiceResult<TodoView> {
        self.require_todo(project_id, todo_id, now_ms)?;
        let blocker_ids = deduplicate_ids(blocker_ids);
        for blocker_id in &blocker_ids {
            self.validate_blocker(project_id, todo_id, *blocker_id, now_ms)?;
        }
        let transaction = self.store.connection().unchecked_transaction()?;
        transaction.execute("DELETE FROM todo_blockers WHERE todo_id = ?1", [todo_id])?;
        for blocker_id in blocker_ids {
            transaction.execute(
                "INSERT INTO todo_blockers (todo_id, blocked_by_todo_id) VALUES (?1, ?2)",
                params![todo_id, blocker_id],
            )?;
        }
        transaction.commit()?;
        self.require_todo(project_id, todo_id, now_ms)
    }

    pub fn add_blocker(
        &self,
        project_id: ProjectId,
        todo_id: TodoId,
        blocker_id: TodoId,
        now_ms: i64,
    ) -> TodoServiceResult<TodoView> {
        self.require_todo(project_id, todo_id, now_ms)?;
        self.validate_blocker(project_id, todo_id, blocker_id, now_ms)?;
        self.store.connection().execute(
            "INSERT INTO todo_blockers (todo_id, blocked_by_todo_id) VALUES (?1, ?2)
             ON CONFLICT(todo_id, blocked_by_todo_id) DO NOTHING",
            params![todo_id, blocker_id],
        )?;
        self.require_todo(project_id, todo_id, now_ms)
    }

    pub fn remove_blocker(
        &self,
        project_id: ProjectId,
        todo_id: TodoId,
        blocker_id: TodoId,
        now_ms: i64,
    ) -> TodoServiceResult<TodoView> {
        self.require_todo(project_id, todo_id, now_ms)?;
        self.store.connection().execute(
            "DELETE FROM todo_blockers WHERE todo_id = ?1 AND blocked_by_todo_id = ?2",
            params![todo_id, blocker_id],
        )?;
        self.require_todo(project_id, todo_id, now_ms)
    }

    pub fn comment_create(
        &self,
        project_id: ProjectId,
        todo_id: TodoId,
        actor: &str,
        body: String,
        now_ms: i64,
    ) -> TodoServiceResult<TodoComment> {
        self.comment_create_as(project_id, todo_id, actor, actor, body, now_ms)
    }

    /// Create a comment while retaining the durable actor id and using a human-readable actor
    /// label for a possible mention notification.
    pub fn comment_create_as(
        &self,
        project_id: ProjectId,
        todo_id: TodoId,
        actor: &str,
        actor_label: &str,
        body: String,
        now_ms: i64,
    ) -> TodoServiceResult<TodoComment> {
        let todo = self.require_todo(project_id, todo_id, now_ms)?;
        validate_body("comment", &body)?;
        let mentions_user = mentions_user(&body);
        let transaction = self.store.connection().unchecked_transaction()?;
        transaction.execute(
            "INSERT INTO todo_comments (todo_id, actor, body, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?4)",
            params![todo_id, actor, body, now_ms],
        )?;
        let comment_id = transaction.last_insert_rowid();
        if mentions_user {
            let actor_label = actor_label.trim();
            let actor_label = if actor_label.is_empty() {
                "Workman"
            } else {
                actor_label
            };
            transaction.execute(
                "INSERT INTO notifications
                    (type, project_id, todo_id, comment_id, body, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    NotificationType::MentionedInComment.as_str(),
                    project_id,
                    todo_id,
                    comment_id,
                    format!("{actor_label} mentioned you on ‘{}’.", todo.title),
                    now_ms,
                ],
            )?;
        }
        transaction.commit()?;
        self.require_comment(project_id, comment_id, now_ms)
    }

    pub fn comment_update(
        &self,
        project_id: ProjectId,
        comment_id: TodoCommentId,
        body: String,
        now_ms: i64,
    ) -> TodoServiceResult<TodoComment> {
        validate_body("comment", &body)?;
        self.require_comment(project_id, comment_id, now_ms)?;
        self.store.connection().execute(
            "UPDATE todo_comments SET body = ?1, updated_at = ?2 WHERE id = ?3",
            params![body, now_ms, comment_id],
        )?;
        self.require_comment(project_id, comment_id, now_ms)
    }

    pub fn comment_delete(
        &self,
        project_id: ProjectId,
        comment_id: TodoCommentId,
        now_ms: i64,
    ) -> TodoServiceResult<TodoId> {
        let comment = self.require_comment(project_id, comment_id, now_ms)?;
        self.store
            .connection()
            .execute("DELETE FROM todo_comments WHERE id = ?1", [comment_id])?;
        Ok(comment.todo_id)
    }

    pub fn comment_list(
        &self,
        project_id: ProjectId,
        todo_id: TodoId,
        offset: usize,
        limit: Option<usize>,
        now_ms: i64,
    ) -> TodoServiceResult<TodoCommentPage> {
        self.require_todo(project_id, todo_id, now_ms)?;
        let total_count = self.store.connection().query_row(
            "SELECT COUNT(*) FROM todo_comments WHERE todo_id = ?1",
            [todo_id],
            |row| row.get::<_, usize>(0),
        )?;
        let limit = limit.unwrap_or(DEFAULT_PAGE_SIZE).clamp(1, MAX_PAGE_SIZE);
        let offset = offset.min(total_count);
        let mut statement = self.store.connection().prepare(
            "SELECT id, todo_id, actor, body, created_at, updated_at
             FROM todo_comments WHERE todo_id = ?1
             ORDER BY created_at, id LIMIT ?2 OFFSET ?3",
        )?;
        let comments = statement
            .query_map(params![todo_id, limit, offset], comment_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let end = offset + comments.len();
        let has_more = end < total_count;
        Ok(TodoCommentPage {
            comments,
            total_count,
            offset,
            limit,
            has_more,
            next_offset: has_more.then_some(end),
        })
    }

    pub fn lock(
        &self,
        project_id: ProjectId,
        todo_id: TodoId,
        actor: &str,
        lease_ttl_ms: i64,
        now_ms: i64,
    ) -> TodoServiceResult<TodoView> {
        if actor.is_empty() {
            return Err(TodoServiceError::InvalidInput(
                "lock actor must not be empty".into(),
            ));
        }
        if lease_ttl_ms <= 0 {
            return Err(TodoServiceError::InvalidInput(
                "lease duration must be positive".into(),
            ));
        }
        let current = self.require_todo(project_id, todo_id, now_ms)?;
        let new_claim = current.locked_by.as_deref() != Some(actor);
        let expiry = now_ms
            .checked_add(lease_ttl_ms)
            .ok_or_else(|| TodoServiceError::InvalidInput("lease duration is too large".into()))?;
        let changed = self.store.connection().execute(
            "UPDATE todos SET
                 lock_acquired_at = CASE
                   WHEN lock_actor = ?1 AND lock_expiry > ?5
                     THEN COALESCE(lock_acquired_at, ?5)
                   ELSE ?5
                 END,
                 lock_actor = ?1,
                 lock_expiry = ?2
             WHERE id = ?3 AND project_id = ?4
               AND (lock_actor IS NULL OR lock_expiry IS NULL OR lock_expiry <= ?5 OR lock_actor = ?1)",
            params![actor, expiry, todo_id, project_id, now_ms],
        )?;
        if changed == 0 {
            let todo = self.require_todo(project_id, todo_id, now_ms)?;
            return Err(TodoServiceError::Locked {
                todo_id,
                owner: todo.locked_by.unwrap_or_else(|| "another actor".into()),
            });
        }
        if new_claim {
            self.record_activity(todo_id, actor, TodoActivityKind::Locked, now_ms)?;
        }
        self.require_todo(project_id, todo_id, now_ms)
    }

    pub fn unlock(
        &self,
        project_id: ProjectId,
        todo_id: TodoId,
        actor: &str,
        now_ms: i64,
    ) -> TodoServiceResult<TodoView> {
        self.require_todo(project_id, todo_id, now_ms)?;
        let changed = self.store.connection().execute(
            "UPDATE todos SET lock_actor = NULL, lock_expiry = NULL, lock_acquired_at = NULL
             WHERE id = ?1 AND project_id = ?2 AND lock_actor = ?3",
            params![todo_id, project_id, actor],
        )?;
        if changed == 0 {
            return Err(TodoServiceError::LockNotOwned(todo_id));
        }
        self.record_activity(todo_id, actor, TodoActivityKind::Unlocked, now_ms)?;
        self.require_todo(project_id, todo_id, now_ms)
    }

    pub fn complete(
        &self,
        project_id: ProjectId,
        todo_id: TodoId,
        actor: &str,
        completed: bool,
        release_lock: bool,
        now_ms: i64,
    ) -> TodoServiceResult<(TodoView, Vec<TodoId>)> {
        let current = self.require_todo(project_id, todo_id, now_ms)?;
        let status = if completed {
            TodoStatus::Completed
        } else if current.status == TodoStatus::Completed {
            TodoStatus::Open
        } else {
            current.status
        };
        self.store.connection().execute(
            "UPDATE todos SET completed = ?1, status = ?2,
                 lock_actor = CASE WHEN ?3 AND lock_actor = ?4 THEN NULL ELSE lock_actor END,
                 lock_expiry = CASE WHEN ?3 AND lock_actor = ?4 THEN NULL ELSE lock_expiry END,
                 lock_acquired_at = CASE
                   WHEN ?3 AND lock_actor = ?4 THEN NULL ELSE lock_acquired_at
                 END
             WHERE id = ?5 AND project_id = ?6",
            params![completed, status, release_lock, actor, todo_id, project_id],
        )?;
        if current.completed != completed {
            self.record_activity(
                todo_id,
                actor,
                if completed {
                    TodoActivityKind::Completed
                } else {
                    TodoActivityKind::Reopened
                },
                now_ms,
            )?;
        }
        let affected = dependent_ids(self.store.connection(), todo_id)?;
        Ok((self.require_todo(project_id, todo_id, now_ms)?, affected))
    }

    pub fn transfer(
        &self,
        project_id: ProjectId,
        todo_id: TodoId,
        target_project_id: ProjectId,
        now_ms: i64,
    ) -> TodoServiceResult<(TodoView, Vec<TodoId>)> {
        self.require_todo(project_id, todo_id, now_ms)?;
        self.require_project(target_project_id)?;
        if project_id == target_project_id {
            return Err(TodoServiceError::InvalidInput(
                "target project must differ from the source project".into(),
            ));
        }
        let affected = dependent_ids(self.store.connection(), todo_id)?;
        let transaction = self.store.connection().unchecked_transaction()?;
        transaction.execute(
            "DELETE FROM todo_blockers WHERE todo_id = ?1 OR blocked_by_todo_id = ?1",
            [todo_id],
        )?;
        transaction.execute(
            "UPDATE todos SET project_id = ?1, lock_actor = NULL, lock_expiry = NULL,
                 lock_acquired_at = NULL,
                 sort_order = (SELECT COALESCE(MAX(sort_order), -1) + 1
                               FROM todos WHERE project_id = ?1)
             WHERE id = ?2",
            params![target_project_id, todo_id],
        )?;
        transaction.commit()?;
        Ok((
            self.require_todo(target_project_id, todo_id, now_ms)?,
            affected,
        ))
    }

    pub fn activity_list(
        &self,
        project_id: ProjectId,
        todo_id: TodoId,
        now_ms: i64,
    ) -> TodoServiceResult<Vec<TodoActivity>> {
        self.require_todo(project_id, todo_id, now_ms)?;
        let mut statement = self.store.connection().prepare(
            "SELECT id, todo_id, actor, kind, created_at
             FROM todo_activity WHERE todo_id = ?1 ORDER BY created_at, id",
        )?;
        let rows = statement.query_map([todo_id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, TodoId>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })?;
        let mut activity = Vec::new();
        for row in rows {
            let (id, todo_id, actor, kind, created_at) = row?;
            activity.push(TodoActivity {
                id,
                todo_id,
                actor,
                kind: TodoActivityKind::parse(&kind)?,
                created_at,
            });
        }
        Ok(activity)
    }

    fn record_activity(
        &self,
        todo_id: TodoId,
        actor: &str,
        kind: TodoActivityKind,
        now_ms: i64,
    ) -> TodoServiceResult<()> {
        self.store.connection().execute(
            "INSERT INTO todo_activity (todo_id, actor, kind, created_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![todo_id, actor, kind.as_str(), now_ms],
        )?;
        Ok(())
    }

    fn require_project(&self, project_id: ProjectId) -> TodoServiceResult<()> {
        if self.store.get_project(project_id)?.is_none() {
            return Err(TodoServiceError::ProjectNotFound(project_id));
        }
        Ok(())
    }

    fn require_todo(
        &self,
        project_id: ProjectId,
        todo_id: TodoId,
        now_ms: i64,
    ) -> TodoServiceResult<TodoView> {
        self.get(project_id, todo_id, now_ms)?
            .ok_or(TodoServiceError::TodoNotFound(todo_id))
    }

    fn require_comment(
        &self,
        project_id: ProjectId,
        comment_id: TodoCommentId,
        now_ms: i64,
    ) -> TodoServiceResult<TodoComment> {
        let comment = self
            .store
            .get_todo_comment(comment_id)?
            .ok_or(TodoServiceError::CommentNotFound(comment_id))?;
        self.require_todo(project_id, comment.todo_id, now_ms)?;
        Ok(comment)
    }

    fn hydrate(&self, mut todo: Todo, now_ms: i64) -> TodoServiceResult<TodoView> {
        if todo.lock_expiry.is_some_and(|expiry| expiry <= now_ms) {
            self.store.connection().execute(
                "UPDATE todos SET lock_actor = NULL, lock_expiry = NULL, lock_acquired_at = NULL
                 WHERE id = ?1 AND lock_expiry <= ?2",
                params![todo.id, now_ms],
            )?;
            todo.lock_actor = None;
            todo.lock_expiry = None;
        }
        let blocker_ids = query_ids(
            self.store.connection(),
            "SELECT blocked_by_todo_id FROM todo_blockers WHERE todo_id = ?1 ORDER BY blocked_by_todo_id",
            todo.id,
        )?;
        let unresolved_blocker_count = self.store.connection().query_row(
            "SELECT COUNT(*) FROM todo_blockers AS edge
             JOIN todos AS blocker ON blocker.id = edge.blocked_by_todo_id
             WHERE edge.todo_id = ?1 AND blocker.completed = 0",
            [todo.id],
            |row| row.get::<_, usize>(0),
        )?;
        let comment_count = self.store.connection().query_row(
            "SELECT COUNT(*) FROM todo_comments WHERE todo_id = ?1",
            [todo.id],
            |row| row.get::<_, usize>(0),
        )?;
        let (assignee, sort_order) = self.store.connection().query_row(
            "SELECT assignee, sort_order FROM todos WHERE id = ?1",
            [todo.id],
            |row| Ok((row.get::<_, Option<String>>(0)?, row.get::<_, i64>(1)?)),
        )?;
        Ok(TodoView {
            id: todo.id,
            project_id: todo.project_id,
            title: todo.title,
            body: todo.body,
            priority: todo.priority,
            status: todo.status,
            completed: todo.completed,
            sort_order,
            assignee,
            locked_by: todo.lock_actor,
            lock_expiry: todo.lock_expiry,
            comment_count,
            tags: todo.tags,
            blocker_ids,
            is_blocked: unresolved_blocker_count > 0,
            unresolved_blocker_count,
        })
    }

    fn todo_matches_query(&self, todo: &TodoView, needle: &str) -> TodoServiceResult<bool> {
        if todo.title.to_lowercase().contains(needle) || todo.body.to_lowercase().contains(needle) {
            return Ok(true);
        }
        let found = self.store.connection().query_row(
            "SELECT EXISTS(
                SELECT 1 FROM todo_comments
                WHERE todo_id = ?1 AND instr(lower(body), ?2) > 0
             )",
            params![todo.id, needle],
            |row| row.get(0),
        )?;
        Ok(found)
    }

    fn validate_blocker(
        &self,
        project_id: ProjectId,
        todo_id: TodoId,
        blocker_id: TodoId,
        now_ms: i64,
    ) -> TodoServiceResult<()> {
        if todo_id == blocker_id {
            return Err(TodoServiceError::BlockerCycle {
                todo_id,
                blocker_id,
            });
        }
        self.require_todo(project_id, blocker_id, now_ms)?;
        let creates_cycle = self.store.connection().query_row(
            "WITH RECURSIVE dependencies(id) AS (
                SELECT blocked_by_todo_id FROM todo_blockers WHERE todo_id = ?1
                UNION
                SELECT edge.blocked_by_todo_id
                FROM todo_blockers AS edge
                JOIN dependencies ON edge.todo_id = dependencies.id
             )
             SELECT EXISTS(SELECT 1 FROM dependencies WHERE id = ?2)",
            params![blocker_id, todo_id],
            |row| row.get::<_, bool>(0),
        )?;
        if creates_cycle {
            return Err(TodoServiceError::BlockerCycle {
                todo_id,
                blocker_id,
            });
        }
        Ok(())
    }
}

fn validate_title(title: &str) -> TodoServiceResult<()> {
    validate_body("todo title", title)
}

fn normalize_assignee(assignee: Option<String>) -> TodoServiceResult<Option<String>> {
    let Some(assignee) = assignee else {
        return Ok(None);
    };
    match assignee.trim().to_ascii_lowercase().as_str() {
        "" | "none" | "unassigned" => Ok(None),
        "user" | "@user" | "me" | "you" => Ok(Some(USER_ASSIGNEE.into())),
        _ => Err(TodoServiceError::InvalidInput(
            "assignee must be user (or none to clear); agent assignees are not supported yet"
                .into(),
        )),
    }
}

fn mentions_user(body: &str) -> bool {
    let lowercase = body.to_ascii_lowercase();
    let bytes = lowercase.as_bytes();
    lowercase
        .match_indices(USER_MENTION)
        .any(|(start, mention)| {
            let end = start + mention.len();
            let before_is_boundary = start == 0 || !is_mention_word_byte(bytes[start - 1]);
            let after_is_boundary = end == bytes.len() || !is_mention_word_byte(bytes[end]);
            before_is_boundary && after_is_boundary
        })
}

fn is_mention_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'@')
}

fn validate_body(field: &str, value: &str) -> TodoServiceResult<()> {
    if value.trim().is_empty() {
        return Err(TodoServiceError::InvalidInput(format!(
            "{field} must not be empty"
        )));
    }
    Ok(())
}

fn normalize_tag(tag: String) -> TodoServiceResult<String> {
    let tag = tag.trim();
    if tag.is_empty() {
        return Err(TodoServiceError::InvalidInput(
            "todo tags must not be empty".into(),
        ));
    }
    Ok(tag.to_owned())
}

fn normalize_tags(tags: Vec<String>) -> TodoServiceResult<Vec<String>> {
    let mut seen = HashSet::new();
    let mut normalized = Vec::with_capacity(tags.len());
    for tag in tags {
        let tag = normalize_tag(tag)?;
        if seen.insert(tag.clone()) {
            normalized.push(tag);
        }
    }
    Ok(normalized)
}

fn deduplicate_ids(ids: Vec<TodoId>) -> Vec<TodoId> {
    let mut seen = HashSet::new();
    ids.into_iter().filter(|id| seen.insert(*id)).collect()
}

fn validate_reorder_ids(
    label: &str,
    current_ids: &[i64],
    ordered_ids: &[i64],
) -> TodoServiceResult<()> {
    let unique = ordered_ids.iter().copied().collect::<HashSet<_>>();
    let current = current_ids.iter().copied().collect::<HashSet<_>>();
    if ordered_ids.len() != current_ids.len()
        || unique.len() != ordered_ids.len()
        || unique != current
    {
        return Err(StoreError::InvalidReorder(format!(
            "{label} reorder must contain every scoped ID exactly once"
        ))
        .into());
    }
    Ok(())
}

fn replace_tags(connection: &Connection, todo_id: TodoId, tags: &[String]) -> rusqlite::Result<()> {
    connection.execute("DELETE FROM todo_tags WHERE todo_id = ?1", [todo_id])?;
    for (position, tag) in tags.iter().enumerate() {
        connection.execute(
            "INSERT INTO todo_tags (todo_id, tag, position) VALUES (?1, ?2, ?3)",
            params![todo_id, tag, position],
        )?;
    }
    Ok(())
}

fn compact_tag_positions(connection: &Connection, todo_id: TodoId) -> rusqlite::Result<()> {
    let tags = {
        let mut statement =
            connection.prepare("SELECT tag FROM todo_tags WHERE todo_id = ?1 ORDER BY position")?;
        statement
            .query_map([todo_id], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    connection.execute("DELETE FROM todo_tags WHERE todo_id = ?1", [todo_id])?;
    for (position, tag) in tags.iter().enumerate() {
        connection.execute(
            "INSERT INTO todo_tags (todo_id, tag, position) VALUES (?1, ?2, ?3)",
            params![todo_id, tag, position],
        )?;
    }
    Ok(())
}

fn dependent_ids(connection: &Connection, todo_id: TodoId) -> rusqlite::Result<Vec<TodoId>> {
    query_ids(
        connection,
        "SELECT todo_id FROM todo_blockers WHERE blocked_by_todo_id = ?1 ORDER BY todo_id",
        todo_id,
    )
}

fn query_ids(connection: &Connection, sql: &str, id: TodoId) -> rusqlite::Result<Vec<TodoId>> {
    let mut statement = connection.prepare(sql)?;
    statement
        .query_map([id], |row| row.get(0))?
        .collect::<rusqlite::Result<Vec<_>>>()
}

fn comment_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<TodoComment> {
    Ok(TodoComment {
        id: row.get(0)?,
        todo_id: row.get(1)?,
        actor: row.get(2)?,
        body: row.get(3)?,
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
    })
}

fn sort_todos(todos: &mut [TodoView], sort: TodoSort) {
    todos.sort_by(|left, right| match sort {
        TodoSort::Priority => priority_rank(left.priority)
            .cmp(&priority_rank(right.priority))
            .then_with(|| right.id.cmp(&left.id)),
        TodoSort::Newest => right.id.cmp(&left.id),
        TodoSort::Oldest => left.id.cmp(&right.id),
        TodoSort::TitleAsc => left
            .title
            .to_lowercase()
            .cmp(&right.title.to_lowercase())
            .then_with(|| left.id.cmp(&right.id)),
        TodoSort::TitleDesc => right
            .title
            .to_lowercase()
            .cmp(&left.title.to_lowercase())
            .then_with(|| right.id.cmp(&left.id)),
        TodoSort::Status => status_rank(left.status)
            .cmp(&status_rank(right.status))
            .then_with(|| priority_rank(left.priority).cmp(&priority_rank(right.priority)))
            .then_with(|| right.id.cmp(&left.id)),
    });
}

fn priority_rank(priority: TodoPriority) -> u8 {
    match priority {
        TodoPriority::High => 0,
        TodoPriority::Medium => 1,
        TodoPriority::Low => 2,
    }
}

fn status_rank(status: TodoStatus) -> u8 {
    match status {
        TodoStatus::InProgress => 0,
        TodoStatus::Open => 1,
        TodoStatus::Backlog => 2,
        TodoStatus::Completed => 3,
    }
}
