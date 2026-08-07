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
    Actor, NewTodo, ProjectId, Store, TodoCommentId, TodoId, TodoListQuery, TodoPriority,
    TodoService, TodoServiceError, TodoSort, TodoStatus, TodoView, USER_ASSIGNEE, UpdateTodo,
};

use super::{WorkmanMcp, failure, now_millis, scoped_project, success};

const DEFAULT_LEASE_TTL_SECONDS: i64 = 300;
const MAX_LEASE_TTL_SECONDS: i64 = 86_400;

#[derive(Debug, Clone, Copy, Default, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
enum ResponseMode {
    #[default]
    Slim,
    Rich,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct TodoCreateArgs {
    #[serde(default)]
    project_id: Option<ProjectId>,
    title: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    priority: Option<String>,
    #[serde(default)]
    tags: Option<Vec<String>>,
    /// Assign the new todo to the human with `user`; omit for no assignment.
    #[serde(default)]
    assignee: Option<String>,
    #[serde(default)]
    response_mode: Option<ResponseMode>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct TodoGetArgs {
    #[serde(default)]
    project_id: Option<ProjectId>,
    todo_id: TodoId,
    #[serde(default)]
    include_comments: bool,
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
    priority: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    tags: Option<Vec<String>>,
    /// Assign to the human with `user`, or clear with `none`; omit to preserve.
    #[serde(default)]
    assignee: Option<String>,
    #[serde(default)]
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
    status: Option<String>,
    #[serde(default)]
    completed: Option<bool>,
    #[serde(default)]
    is_blocked: Option<bool>,
    #[serde(default)]
    priority: Option<String>,
    /// Filter to todos assigned to the human with `user`.
    #[serde(default)]
    assignee: Option<String>,
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    tags: Option<Vec<String>>,
    #[serde(default)]
    sort: Option<String>,
    #[serde(default)]
    offset: Option<usize>,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct TodoTagArgs {
    #[serde(default)]
    project_id: Option<ProjectId>,
    todo_id: TodoId,
    tag: String,
    #[serde(default)]
    response_mode: Option<ResponseMode>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct TodoSetBlockersArgs {
    #[serde(default)]
    project_id: Option<ProjectId>,
    todo_id: TodoId,
    #[serde(default)]
    blocker_ids: Option<Vec<TodoId>>,
    #[serde(default)]
    response_mode: Option<ResponseMode>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct TodoBlockerArgs {
    #[serde(default)]
    project_id: Option<ProjectId>,
    todo_id: TodoId,
    blocker_id: TodoId,
    #[serde(default)]
    response_mode: Option<ResponseMode>,
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
struct TodoCommentListArgs {
    #[serde(default)]
    project_id: Option<ProjectId>,
    todo_id: TodoId,
    #[serde(default)]
    offset: Option<usize>,
    #[serde(default)]
    limit: Option<usize>,
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
struct TodoAssignArgs {
    #[serde(default)]
    project_id: Option<ProjectId>,
    todo_id: TodoId,
    /// Assign to the human with `user`; omit, use null, or use `none` to clear.
    #[serde(default)]
    assignee: Option<String>,
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

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct TodoTransferArgs {
    #[serde(default)]
    project_id: Option<ProjectId>,
    todo_id: TodoId,
    target_project_id: ProjectId,
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
        let priority = match parse_priority(args.priority.as_deref().unwrap_or("medium")) {
            Ok(priority) => priority,
            Err(error) => return todo_failure(error),
        };
        let mut registry = self.registry.lock().await;
        let (project, actor) = match scoped_project(&mut registry, &parts, args.project_id) {
            Ok(scoped) => scoped,
            Err(error) => return failure("project_scope_error", error),
        };
        let actor_label = match actor_label(registry.store(), &actor) {
            Ok(label) => label,
            Err(error) => return todo_failure(error),
        };
        let service = TodoService::new(registry.store());
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

    #[tool(
        description = "Assign a todo to the human with assignee=user, or omit assignee/use none to unassign"
    )]
    async fn todo_assign(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<TodoAssignArgs>,
    ) -> CallToolResult {
        let mut registry = self.registry.lock().await;
        let (project, actor) = match scoped_project(&mut registry, &parts, args.project_id) {
            Ok(scoped) => scoped,
            Err(error) => return failure("project_scope_error", error),
        };
        let actor_label = match actor_label(registry.store(), &actor) {
            Ok(label) => label,
            Err(error) => return todo_failure(error),
        };
        match TodoService::new(registry.store()).assign(
            project.id,
            args.todo_id,
            args.assignee,
            &actor_label,
            now_millis(),
        ) {
            Ok(todo) => todo_response(todo, args.response_mode),
            Err(error) => todo_failure(error),
        }
    }

    #[tool(description = "Read one todo and optionally include its comments")]
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
                let mut comments = Vec::new();
                let mut offset = 0;
                loop {
                    match service.comment_list(project.id, args.todo_id, offset, Some(200), now) {
                        Ok(page) => {
                            comments.extend(page.comments);
                            match page.next_offset {
                                Some(next_offset) => offset = next_offset,
                                None => break,
                            }
                        }
                        Err(error) => return todo_failure(error),
                    }
                }
                success(json!({ "found": true, "todo": todo, "comments": comments }))
            }
            Ok(Some(todo)) => success(json!({ "found": true, "todo": todo })),
            Ok(None) => success(json!({ "found": false, "todo": null })),
            Err(error) => todo_failure(error),
        }
    }

    #[tool(
        description = "Update todo fields; assignee=user assigns the human, assignee=none clears, omitted fields are preserved"
    )]
    async fn todo_update(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<TodoUpdateArgs>,
    ) -> CallToolResult {
        let priority = match args.priority.as_deref().map(parse_priority).transpose() {
            Ok(priority) => priority,
            Err(error) => return todo_failure(error),
        };
        let status = match args.status.as_deref().map(parse_status).transpose() {
            Ok(status) => status,
            Err(error) => return todo_failure(error),
        };
        let mut registry = self.registry.lock().await;
        let (project, actor) = match scoped_project(&mut registry, &parts, args.project_id) {
            Ok(scoped) => scoped,
            Err(error) => return failure("project_scope_error", error),
        };
        let actor_label = match actor_label(registry.store(), &actor) {
            Ok(label) => label,
            Err(error) => return todo_failure(error),
        };
        let service = TodoService::new(registry.store());
        let updated = match service.update(
            project.id,
            args.todo_id,
            UpdateTodo {
                title: args.title,
                body: args.body,
                priority,
                status,
                tags: args.tags,
            },
            now_millis(),
        ) {
            Ok(todo) => todo,
            Err(error) => return todo_failure(error),
        };
        let todo = match args.assignee {
            Some(assignee) => match service.assign(
                project.id,
                args.todo_id,
                Some(assignee),
                &actor_label,
                now_millis(),
            ) {
                Ok(todo) => todo,
                Err(error) => return todo_failure(error),
            },
            None => updated,
        };
        todo_response(todo, args.response_mode)
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
        let status = match args.status.as_deref().map(parse_status).transpose() {
            Ok(status) => status,
            Err(error) => return todo_failure(error),
        };
        let priority = match args.priority.as_deref().map(parse_priority).transpose() {
            Ok(priority) => priority,
            Err(error) => return todo_failure(error),
        };
        let assignee = match args.assignee.as_deref().map(parse_assignee).transpose() {
            Ok(assignee) => assignee,
            Err(error) => return todo_failure(error),
        };
        let sort = match parse_sort(args.sort.as_deref()) {
            Ok(sort) => sort,
            Err(error) => return todo_failure(error),
        };
        let mut registry = self.registry.lock().await;
        let (project, _) = match scoped_project(&mut registry, &parts, args.project_id) {
            Ok(scoped) => scoped,
            Err(error) => return failure("project_scope_error", error),
        };
        let service = TodoService::new(registry.store());
        match service.list(
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
            Ok(page) => success(page),
            Err(error) => todo_failure(error),
        }
    }

    #[tool(description = "List distinct todo tags in a project")]
    async fn todo_tags_list(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<super::ProjectScopeArgs>,
    ) -> CallToolResult {
        let mut registry = self.registry.lock().await;
        let (project, _) = match scoped_project(&mut registry, &parts, args.project_id) {
            Ok(scoped) => scoped,
            Err(error) => return failure("project_scope_error", error),
        };
        match TodoService::new(registry.store()).tags_list(project.id) {
            Ok(tags) => success(json!({ "tags": tags })),
            Err(error) => todo_failure(error),
        }
    }

    #[tool(description = "Add one tag without replacing other tags")]
    async fn todo_add_tag(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<TodoTagArgs>,
    ) -> CallToolResult {
        self.todo_tag_change(parts, args, true).await
    }

    #[tool(description = "Remove one tag without replacing other tags")]
    async fn todo_remove_tag(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<TodoTagArgs>,
    ) -> CallToolResult {
        self.todo_tag_change(parts, args, false).await
    }

    #[tool(description = "Replace a todo's full blocker list")]
    async fn todo_set_blockers(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<TodoSetBlockersArgs>,
    ) -> CallToolResult {
        let mut registry = self.registry.lock().await;
        let (project, _) = match scoped_project(&mut registry, &parts, args.project_id) {
            Ok(scoped) => scoped,
            Err(error) => return failure("project_scope_error", error),
        };
        let service = TodoService::new(registry.store());
        match service.set_blockers(
            project.id,
            args.todo_id,
            args.blocker_ids.unwrap_or_default(),
            now_millis(),
        ) {
            Ok(todo) => todo_response(todo, args.response_mode),
            Err(error) => todo_failure(error),
        }
    }

    #[tool(description = "Add one blocker without replacing other blockers")]
    async fn todo_add_blocker(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<TodoBlockerArgs>,
    ) -> CallToolResult {
        self.todo_blocker_change(parts, args, true).await
    }

    #[tool(description = "Remove one blocker without replacing other blockers")]
    async fn todo_remove_blocker(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<TodoBlockerArgs>,
    ) -> CallToolResult {
        self.todo_blocker_change(parts, args, false).await
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
        let actor_label = match actor_label(registry.store(), &actor) {
            Ok(label) => label,
            Err(error) => return todo_failure(error),
        };
        let service = TodoService::new(registry.store());
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

    #[tool(description = "List comments for a todo with optional pagination")]
    async fn todo_comment_list(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<TodoCommentListArgs>,
    ) -> CallToolResult {
        let mut registry = self.registry.lock().await;
        let (project, _) = match scoped_project(&mut registry, &parts, args.project_id) {
            Ok(scoped) => scoped,
            Err(error) => return failure("project_scope_error", error),
        };
        match TodoService::new(registry.store()).comment_list(
            project.id,
            args.todo_id,
            args.offset.unwrap_or(0),
            args.limit,
            now_millis(),
        ) {
            Ok(page) => success(page),
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
        let service = TodoService::new(registry.store());
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

    #[tool(description = "Release a todo lock owned by this MCP actor")]
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
        match TodoService::new(registry.store()).unlock(
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
        description = "Mark a todo complete or incomplete and optionally release this actor's lock"
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
        match TodoService::new(registry.store()).complete(
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

    #[tool(
        description = "Move a todo to another project while preserving comments and completion (cross-project transfer is unavailable to agent identities)"
    )]
    async fn todo_transfer(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<TodoTransferArgs>,
    ) -> CallToolResult {
        let mut registry = self.registry.lock().await;
        let (project, actor) = match scoped_project(&mut registry, &parts, args.project_id) {
            Ok(scoped) => scoped,
            Err(error) => return failure("project_scope_error", error),
        };
        if let Err(error) = super::enforce_project_access(&registry, &actor, args.target_project_id)
        {
            return failure("project_scope_error", error);
        }
        match TodoService::new(registry.store()).transfer(
            project.id,
            args.todo_id,
            args.target_project_id,
            now_millis(),
        ) {
            Ok((todo, _)) if matches!(args.response_mode, Some(ResponseMode::Rich)) => {
                success(todo)
            }
            Ok((_, affected_todo_ids)) => success(json!({
                "project_id": project.id,
                "todo_id": args.todo_id,
                "target_project_id": args.target_project_id,
                "affected_todo_ids": affected_todo_ids,
            })),
            Err(error) => todo_failure(error),
        }
    }

    async fn todo_tag_change(&self, parts: Parts, args: TodoTagArgs, add: bool) -> CallToolResult {
        let mut registry = self.registry.lock().await;
        let (project, _) = match scoped_project(&mut registry, &parts, args.project_id) {
            Ok(scoped) => scoped,
            Err(error) => return failure("project_scope_error", error),
        };
        let service = TodoService::new(registry.store());
        let result = if add {
            service.add_tag(project.id, args.todo_id, args.tag.clone(), now_millis())
        } else {
            service.remove_tag(project.id, args.todo_id, args.tag.clone(), now_millis())
        };
        match result {
            Ok(todo) if matches!(args.response_mode, Some(ResponseMode::Rich)) => success(todo),
            Ok(_) => success(json!({
                "project_id": project.id,
                "todo_id": args.todo_id,
                "tag": args.tag.trim(),
            })),
            Err(error) => todo_failure(error),
        }
    }

    async fn todo_blocker_change(
        &self,
        parts: Parts,
        args: TodoBlockerArgs,
        add: bool,
    ) -> CallToolResult {
        let mut registry = self.registry.lock().await;
        let (project, _) = match scoped_project(&mut registry, &parts, args.project_id) {
            Ok(scoped) => scoped,
            Err(error) => return failure("project_scope_error", error),
        };
        let service = TodoService::new(registry.store());
        let result = if add {
            service.add_blocker(project.id, args.todo_id, args.blocker_id, now_millis())
        } else {
            service.remove_blocker(project.id, args.todo_id, args.blocker_id, now_millis())
        };
        match result {
            Ok(todo) => todo_response(todo, args.response_mode),
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

fn parse_priority(value: &str) -> Result<TodoPriority, TodoServiceError> {
    value.parse().map_err(|_| {
        TodoServiceError::InvalidInput("priority must be one of high, medium, or low".into())
    })
}

fn parse_status(value: &str) -> Result<TodoStatus, TodoServiceError> {
    value.parse().map_err(|_| {
        TodoServiceError::InvalidInput(
            "status must be one of open, in_progress, backlog, or completed".into(),
        )
    })
}

fn parse_assignee(value: &str) -> Result<String, TodoServiceError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "user" | "@user" | "me" | "you" => Ok(USER_ASSIGNEE.into()),
        _ => Err(TodoServiceError::InvalidInput(
            "assignee filter must be user".into(),
        )),
    }
}

fn actor_label(store: &Store, actor: &Actor) -> Result<String, TodoServiceError> {
    let process = actor
        .process_id
        .map(|process_id| store.get_process(process_id))
        .transpose()?
        .flatten();
    Ok(process
        .map(|process| process.name)
        .unwrap_or_else(|| actor.id.clone()))
}

fn parse_sort(value: Option<&str>) -> Result<TodoSort, TodoServiceError> {
    match value.unwrap_or("priority") {
        "priority" | "priority_desc" => Ok(TodoSort::Priority),
        "newest" | "created_at_desc" | "updated_at_desc" => Ok(TodoSort::Newest),
        "oldest" | "created_at_asc" | "updated_at_asc" => Ok(TodoSort::Oldest),
        "title" | "title_asc" => Ok(TodoSort::TitleAsc),
        "title_desc" => Ok(TodoSort::TitleDesc),
        "status" | "status_asc" => Ok(TodoSort::Status),
        _ => Err(TodoServiceError::InvalidInput(
            "unsupported todo sort; use priority, newest, oldest, title_asc, title_desc, or status"
                .into(),
        )),
    }
}

fn todo_failure(error: TodoServiceError) -> CallToolResult {
    failure(error.code(), error.to_string())
}
