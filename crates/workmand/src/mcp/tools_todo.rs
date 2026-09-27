//! MCP todo tools backed by [`workman_core::TodoService`].

use axum::http::request::Parts;
use rmcp::{
    handler::server::{tool::Extension, wrapper::Parameters},
    model::CallToolResult,
    schemars, tool, tool_router,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use workman_core::{
    Actor, MergedTodoUpdate, NewTodo, ProjectId, Store, TodoCommentId, TodoId, TodoListQuery,
    TodoPriority, TodoService, TodoServiceError, TodoSort, TodoStatus, TodoView, USER_ASSIGNEE,
    UpdateTodo,
};

use super::{WorkmanMcp, failure, now_millis, scoped_project, success};

const DEFAULT_LEASE_TTL_SECONDS: i64 = 300;
const MAX_LEASE_TTL_SECONDS: i64 = 86_400;

/// Controls response detail: `slim` returns a compact receipt and `rich` returns the full record.
#[derive(Debug, Clone, Copy, Default, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
enum ResponseMode {
    #[default]
    Slim,
    Rich,
}

#[derive(Debug, Clone, Copy, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
enum TodoPriorityArg {
    High,
    Medium,
    Low,
}

impl From<TodoPriorityArg> for TodoPriority {
    fn from(priority: TodoPriorityArg) -> Self {
        match priority {
            TodoPriorityArg::High => Self::High,
            TodoPriorityArg::Medium => Self::Medium,
            TodoPriorityArg::Low => Self::Low,
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
enum TodoStatusArg {
    Open,
    InProgress,
    Backlog,
    Completed,
}

impl From<TodoStatusArg> for TodoStatus {
    fn from(status: TodoStatusArg) -> Self {
        match status {
            TodoStatusArg::Open => Self::Open,
            TodoStatusArg::InProgress => Self::InProgress,
            TodoStatusArg::Backlog => Self::Backlog,
            TodoStatusArg::Completed => Self::Completed,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
enum TodoSortArg {
    #[default]
    Priority,
    Newest,
    Oldest,
    TitleAsc,
    TitleDesc,
    Status,
}

impl From<TodoSortArg> for TodoSort {
    fn from(sort: TodoSortArg) -> Self {
        match sort {
            TodoSortArg::Priority => Self::Priority,
            TodoSortArg::Newest => Self::Newest,
            TodoSortArg::Oldest => Self::Oldest,
            TodoSortArg::TitleAsc => Self::TitleAsc,
            TodoSortArg::TitleDesc => Self::TitleDesc,
            TodoSortArg::Status => Self::Status,
        }
    }
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct TodoCreateArgs {
    #[serde(default)]
    project_id: Option<ProjectId>,
    title: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    priority: Option<TodoPriorityArg>,
    #[serde(default)]
    tags: Option<Vec<String>>,
    /// Assign the new todo to the human with `user`; omit for no assignment.
    #[serde(default)]
    assignee: Option<String>,
    #[serde(default)]
    /// `slim` returns a compact receipt; `rich` returns the full todo.
    response_mode: Option<ResponseMode>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct TodoGetArgs {
    #[serde(default)]
    project_id: Option<ProjectId>,
    todo_id: TodoId,
    #[serde(default)]
    include_comments: bool,
    #[serde(default)]
    comments_offset: Option<usize>,
    #[serde(default)]
    comments_limit: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct TodoUpdateArgs {
    #[serde(default)]
    project_id: Option<ProjectId>,
    todo_id: TodoId,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    priority: Option<TodoPriorityArg>,
    #[serde(default)]
    status: Option<TodoStatusArg>,
    #[serde(default)]
    tags: Option<Vec<String>>,
    /// Add tags without replacing existing tags.
    #[serde(default)]
    add_tags: Option<Vec<String>>,
    /// Remove tags without replacing other tags.
    #[serde(default)]
    remove_tags: Option<Vec<String>>,
    /// Replace the full blocker list.
    #[serde(default)]
    blocker_ids: Option<Vec<TodoId>>,
    /// Add blockers without replacing existing blockers.
    #[serde(default)]
    add_blocker_ids: Option<Vec<TodoId>>,
    /// Remove blockers without replacing other blockers.
    #[serde(default)]
    remove_blocker_ids: Option<Vec<TodoId>>,
    /// Assign to the human with `user`, or clear with `none`; omit to preserve.
    #[serde(default)]
    assignee: Option<String>,
    #[serde(default)]
    /// `slim` returns a compact receipt; `rich` returns the full todo.
    response_mode: Option<ResponseMode>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct TodoDeleteArgs {
    #[serde(default)]
    project_id: Option<ProjectId>,
    todo_id: TodoId,
}

#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
struct TodoListArgs {
    #[serde(default)]
    project_id: Option<ProjectId>,
    #[serde(default)]
    status: Option<TodoStatusArg>,
    #[serde(default)]
    completed: Option<bool>,
    #[serde(default)]
    is_blocked: Option<bool>,
    #[serde(default)]
    priority: Option<TodoPriorityArg>,
    /// Filter to todos assigned to the human with `user`.
    #[serde(default)]
    assignee: Option<String>,
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    tags: Option<Vec<String>>,
    #[serde(default)]
    sort: Option<TodoSortArg>,
    #[serde(default)]
    offset: Option<usize>,
    #[serde(default)]
    limit: Option<usize>,
    /// Include the project's distinct todo tags beside the page.
    #[serde(default)]
    include_tags: bool,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct TodoCommentCreateArgs {
    #[serde(default)]
    project_id: Option<ProjectId>,
    todo_id: TodoId,
    body: String,
    #[serde(default)]
    response_mode: Option<ResponseMode>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct TodoCommentUpdateArgs {
    #[serde(default)]
    project_id: Option<ProjectId>,
    comment_id: TodoCommentId,
    body: String,
    #[serde(default)]
    response_mode: Option<ResponseMode>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct TodoCommentDeleteArgs {
    #[serde(default)]
    project_id: Option<ProjectId>,
    comment_id: TodoCommentId,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct TodoLockArgs {
    #[serde(default)]
    project_id: Option<ProjectId>,
    todo_id: TodoId,
    #[serde(default)]
    lease_ttl_seconds: Option<i64>,
    #[serde(default)]
    response_mode: Option<ResponseMode>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct TodoWriteArgs {
    #[serde(default)]
    project_id: Option<ProjectId>,
    todo_id: TodoId,
    #[serde(default)]
    response_mode: Option<ResponseMode>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct TodoCompleteArgs {
    #[serde(default)]
    project_id: Option<ProjectId>,
    todo_id: TodoId,
    completed: bool,
    #[serde(default)]
    release_lock: Option<bool>,
    #[serde(default)]
    response_mode: Option<ResponseMode>,
}

#[derive(Debug, Serialize)]
struct TodoReceipt {
    project_id: ProjectId,
    todo_id: TodoId,
}

#[tool_router(router = todo_tool_router, vis = "pub(crate)")]
impl WorkmanMcp {
    #[tool(
        description = "Create a project-scoped todo; assignee=user assigns it to the human and notifies them"
    )]
    async fn todo_create(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<TodoCreateArgs>,
    ) -> CallToolResult {
        let priority = args.priority.unwrap_or(TodoPriorityArg::Medium).into();
        let mut registry = self.registry.lock().await;
        let (project, actor) = match scoped_project(&mut registry, &parts, args.project_id) {
            Ok(scoped) => scoped,
            Err(error) => return failure("project_scope_error", error),
        };
        let actor_label = actor_label(registry.store(), &actor);
        let service = TodoService::attributed(registry.store(), actor.id.clone());
        let created = match service.create(
            project.id,
            NewTodo {
                title: args.title,
                body: args.body.unwrap_or_default(),
                priority,
                tags: args.tags.unwrap_or_default(),
            },
            now_millis(),
        ) {
            Ok(todo) => todo,
            Err(error) => return todo_failure(error),
        };
        let todo = match args.assignee {
            Some(assignee) => match service.assign(
                project.id,
                created.id,
                Some(assignee),
                &actor_label,
                now_millis(),
            ) {
                Ok(todo) => todo,
                Err(error) => return todo_failure(error),
            },
            None => created,
        };
        todo_response(todo, args.response_mode)
    }

    #[tool(description = "Read one todo, optionally with a paginated comment page")]
    async fn todo_get(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<TodoGetArgs>,
    ) -> CallToolResult {
        let mut registry = self.registry.lock().await;
        let (project, _) = match scoped_project(&mut registry, &parts, args.project_id) {
            Ok(scoped) => scoped,
            Err(error) => return failure("project_scope_error", error),
        };
        let service = TodoService::new(registry.store());
        let now = now_millis();
        match service.get(project.id, args.todo_id, now) {
            Ok(Some(todo)) if args.include_comments => {
                match service.comment_list(
                    project.id,
                    args.todo_id,
                    args.comments_offset.unwrap_or(0),
                    args.comments_limit,
                    now,
                ) {
                    Ok(page) => success(json!({
                        "found": true,
                        "todo": todo,
                        "comments": page.comments,
                        "comments_total_count": page.total_count,
                        "comments_offset": page.offset,
                        "comments_limit": page.limit,
                        "comments_has_more": page.has_more,
                        "comments_next_offset": page.next_offset,
                    })),
                    Err(error) => todo_failure(error),
                }
            }
            Ok(Some(todo)) => success(json!({ "found": true, "todo": todo })),
            Ok(None) => success(json!({ "found": false, "todo": null })),
            Err(error) => todo_failure(error),
        }
    }

    #[tool(
        description = "Update todo fields, assignment, tags, or blockers; omitted fields are preserved"
    )]
    async fn todo_update(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<TodoUpdateArgs>,
    ) -> CallToolResult {
        let priority = args.priority.map(Into::into);
        let status = args.status.map(Into::into);
        let mut registry = self.registry.lock().await;
        let (project, actor) = match scoped_project(&mut registry, &parts, args.project_id) {
            Ok(scoped) => scoped,
            Err(error) => return failure("project_scope_error", error),
        };
        let actor_label = actor_label(registry.store(), &actor);
        let service = TodoService::attributed(registry.store(), actor.id.clone());
        match service.update_merged(
            project.id,
            args.todo_id,
            MergedTodoUpdate {
                fields: UpdateTodo {
                    title: args.title,
                    body: args.body,
                    priority,
                    status,
                    tags: args.tags,
                },
                assignee: args.assignee,
                add_tags: args.add_tags.unwrap_or_default(),
                remove_tags: args.remove_tags.unwrap_or_default(),
                blocker_ids: args.blocker_ids,
                add_blocker_ids: args.add_blocker_ids.unwrap_or_default(),
                remove_blocker_ids: args.remove_blocker_ids.unwrap_or_default(),
            },
            &actor_label,
            now_millis(),
        ) {
            Ok(todo) => todo_response(todo, args.response_mode),
            Err(error) => todo_failure(error),
        }
    }

    #[tool(description = "Delete a project-scoped todo item")]
    async fn todo_delete(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<TodoDeleteArgs>,
    ) -> CallToolResult {
        let mut registry = self.registry.lock().await;
        let (project, _) = match scoped_project(&mut registry, &parts, args.project_id) {
            Ok(scoped) => scoped,
            Err(error) => return failure("project_scope_error", error),
        };
        let service = TodoService::new(registry.store());
        match service.delete(project.id, args.todo_id, now_millis()) {
            Ok(affected_todo_ids) => success(json!({
                "project_id": project.id,
                "todo_id": args.todo_id,
                "affected_todo_ids": affected_todo_ids,
            })),
            Err(error) => todo_failure(error),
        }
    }

    #[tool(description = "List todo summaries with filters, sort, and pagination")]
    async fn todo_list(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<TodoListArgs>,
    ) -> CallToolResult {
        let status = args.status.map(Into::into);
        let priority = args.priority.map(Into::into);
        let assignee = match args.assignee.as_deref().map(parse_assignee).transpose() {
            Ok(assignee) => assignee,
            Err(error) => return todo_failure(error),
        };
        let sort = args.sort.unwrap_or_default().into();
        let mut registry = self.registry.lock().await;
        let (project, _) = match scoped_project(&mut registry, &parts, args.project_id) {
            Ok(scoped) => scoped,
            Err(error) => return failure("project_scope_error", error),
        };
        let service = TodoService::new(registry.store());
        let page = match service.list(
            project.id,
            TodoListQuery {
                status,
                completed: args.completed,
                is_blocked: args.is_blocked,
                priority,
                assignee,
                query: args.query,
                tags: args.tags.unwrap_or_default(),
                sort,
                offset: args.offset.unwrap_or(0),
                limit: args.limit,
            },
            now_millis(),
        ) {
            Ok(page) => page,
            Err(error) => return todo_failure(error),
        };
        if args.include_tags {
            match service.tags_list(project.id) {
                Ok(tags) => {
                    let mut result = serde_json::to_value(page).expect("todo page serializes");
                    result
                        .as_object_mut()
                        .expect("todo page is an object")
                        .insert("tags".into(), json!(tags));
                    success(result)
                }
                Err(error) => todo_failure(error),
            }
        } else {
            success(page)
        }
    }

    #[tool(description = "Add a todo comment; mention @user to notify the human")]
    async fn todo_comment_create(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<TodoCommentCreateArgs>,
    ) -> CallToolResult {
        let mut registry = self.registry.lock().await;
        let (project, actor) = match scoped_project(&mut registry, &parts, args.project_id) {
            Ok(scoped) => scoped,
            Err(error) => return failure("project_scope_error", error),
        };
        let actor_label = actor_label(registry.store(), &actor);
        let service = TodoService::attributed(registry.store(), actor.id.clone());
        match service.comment_create_as(
            project.id,
            args.todo_id,
            &actor.id,
            &actor_label,
            args.body,
            now_millis(),
        ) {
            Ok(comment) if matches!(args.response_mode, Some(ResponseMode::Rich)) => {
                success(comment)
            }
            Ok(comment) => success(json!({
                "project_id": project.id,
                "todo_id": args.todo_id,
                "comment_id": comment.id,
            })),
            Err(error) => todo_failure(error),
        }
    }

    #[tool(description = "Update a todo comment")]
    async fn todo_comment_update(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<TodoCommentUpdateArgs>,
    ) -> CallToolResult {
        let mut registry = self.registry.lock().await;
        let (project, _) = match scoped_project(&mut registry, &parts, args.project_id) {
            Ok(scoped) => scoped,
            Err(error) => return failure("project_scope_error", error),
        };
        let service = TodoService::new(registry.store());
        match service.comment_update(project.id, args.comment_id, args.body, now_millis()) {
            Ok(comment) if matches!(args.response_mode, Some(ResponseMode::Rich)) => {
                success(comment)
            }
            Ok(comment) => success(json!({
                "project_id": project.id,
                "todo_id": comment.todo_id,
                "comment_id": comment.id,
            })),
            Err(error) => todo_failure(error),
        }
    }

    #[tool(description = "Delete a todo comment")]
    async fn todo_comment_delete(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<TodoCommentDeleteArgs>,
    ) -> CallToolResult {
        let mut registry = self.registry.lock().await;
        let (project, _) = match scoped_project(&mut registry, &parts, args.project_id) {
            Ok(scoped) => scoped,
            Err(error) => return failure("project_scope_error", error),
        };
        match TodoService::new(registry.store()).comment_delete(
            project.id,
            args.comment_id,
            now_millis(),
        ) {
            Ok(todo_id) => success(json!({
                "project_id": project.id,
                "todo_id": todo_id,
                "comment_id": args.comment_id,
            })),
            Err(error) => todo_failure(error),
        }
    }

    #[tool(description = "Lock a todo for coordinated editing with a renewable lease")]
    async fn todo_lock(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<TodoLockArgs>,
    ) -> CallToolResult {
        let ttl_seconds = args.lease_ttl_seconds.unwrap_or(DEFAULT_LEASE_TTL_SECONDS);
        if !(1..=MAX_LEASE_TTL_SECONDS).contains(&ttl_seconds) {
            return todo_failure(TodoServiceError::InvalidInput(format!(
                "lease_ttl_seconds must be between 1 and {MAX_LEASE_TTL_SECONDS}"
            )));
        }
        let mut registry = self.registry.lock().await;
        let (project, actor) = match scoped_project(&mut registry, &parts, args.project_id) {
            Ok(scoped) => scoped,
            Err(error) => return failure("project_scope_error", error),
        };
        let service = TodoService::attributed(registry.store(), actor.id.clone());
        match service.lock(
            project.id,
            args.todo_id,
            &actor.id,
            ttl_seconds * 1_000,
            now_millis(),
        ) {
            Ok(todo) => todo_response(todo, args.response_mode),
            Err(error) => todo_failure(error),
        }
    }

    #[tool(
        description = "Release a todo lock owned by this MCP process; ownership survives MCP reconnects"
    )]
    async fn todo_unlock(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<TodoWriteArgs>,
    ) -> CallToolResult {
        let mut registry = self.registry.lock().await;
        let (project, actor) = match scoped_project(&mut registry, &parts, args.project_id) {
            Ok(scoped) => scoped,
            Err(error) => return failure("project_scope_error", error),
        };
        match TodoService::attributed(registry.store(), actor.id.clone()).unlock(
            project.id,
            args.todo_id,
            &actor.id,
            now_millis(),
        ) {
            Ok(todo) => todo_response(todo, args.response_mode),
            Err(error) => todo_failure(error),
        }
    }

    #[tool(
        description = "Mark a todo complete or incomplete and optionally release this MCP process's lock"
    )]
    async fn todo_complete(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<TodoCompleteArgs>,
    ) -> CallToolResult {
        let mut registry = self.registry.lock().await;
        let (project, actor) = match scoped_project(&mut registry, &parts, args.project_id) {
            Ok(scoped) => scoped,
            Err(error) => return failure("project_scope_error", error),
        };
        match TodoService::attributed(registry.store(), actor.id.clone()).complete(
            project.id,
            args.todo_id,
            &actor.id,
            args.completed,
            args.release_lock.unwrap_or(true),
            now_millis(),
        ) {
            Ok((todo, _)) if matches!(args.response_mode, Some(ResponseMode::Rich)) => {
                success(todo)
            }
            Ok((_, affected_todo_ids)) => success(json!({
                "project_id": project.id,
                "todo_id": args.todo_id,
                "completed": args.completed,
                "affected_todo_ids": affected_todo_ids,
            })),
            Err(error) => todo_failure(error),
        }
    }
}

fn todo_response(todo: TodoView, response_mode: Option<ResponseMode>) -> CallToolResult {
    if matches!(response_mode, Some(ResponseMode::Rich)) {
        success(todo)
    } else {
        success(TodoReceipt {
            project_id: todo.project_id,
            todo_id: todo.id,
        })
    }
}

fn parse_assignee(value: &str) -> Result<String, TodoServiceError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "user" | "@user" | "me" | "you" => Ok(USER_ASSIGNEE.into()),
        _ => Err(TodoServiceError::InvalidInput(
            "assignee filter must be user".into(),
        )),
    }
}

fn actor_label(store: &Store, actor: &Actor) -> String {
    store.actor_display_label(&actor.id)
}

fn todo_failure(error: TodoServiceError) -> CallToolResult {
    failure(error.code(), error.to_string())
}
