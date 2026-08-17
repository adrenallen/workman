//! Coordination RPCs used by the desktop todo and scratchpad views.

use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use serde_json::{Value, json};
use workman_core::{
    NewScratchpadComment, NewTodo, ProjectId, ScratchpadCommentId, ScratchpadId,
    ScratchpadListQuery, ScratchpadReadMode, ScratchpadService, ScratchpadServiceError, Store,
    TodoId, TodoListQuery, TodoPriority, TodoService, TodoServiceError, TodoSort, TodoStatus,
    UpdateTodo,
};

pub(crate) type ControlResult = Result<Value, (&'static str, String)>;

#[derive(Debug, Deserialize)]
struct ProjectParams {
    project_id: ProjectId,
}

#[derive(Debug, Deserialize)]
struct ReorderParams {
    project_id: ProjectId,
    ordered_ids: Vec<i64>,
}

#[derive(Debug, Deserialize)]
struct TodoParams {
    project_id: ProjectId,
    todo_id: TodoId,
}

#[derive(Debug, Deserialize)]
struct SetTodoBlockersParams {
    project_id: ProjectId,
    todo_id: TodoId,
    #[serde(default)]
    blocker_ids: Vec<TodoId>,
}

#[derive(Debug, Deserialize)]
struct TodoBlockerParams {
    project_id: ProjectId,
    todo_id: TodoId,
    blocker_id: TodoId,
}

#[derive(Debug, Deserialize)]
struct CreateTodoParams {
    project_id: ProjectId,
    title: String,
    #[serde(default)]
    body: String,
    #[serde(default = "medium_priority")]
    priority: TodoPriority,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    blocker_ids: Vec<TodoId>,
}

#[derive(Debug, Deserialize)]
struct CompleteTodoParams {
    project_id: ProjectId,
    todo_id: TodoId,
    #[serde(default = "default_true")]
    completed: bool,
}

#[derive(Debug, Deserialize)]
struct UpdateTodoParams {
    project_id: ProjectId,
    todo_id: TodoId,
    title: Option<String>,
    body: Option<String>,
    priority: Option<TodoPriority>,
    status: Option<TodoStatus>,
    tags: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct TransferTodoParams {
    project_id: ProjectId,
    todo_id: TodoId,
    target_project_id: ProjectId,
}

#[derive(Debug, Deserialize)]
struct CommentTodoParams {
    project_id: ProjectId,
    todo_id: TodoId,
    body: String,
}

#[derive(Debug, Deserialize)]
struct ScratchpadParams {
    project_id: ProjectId,
    scratchpad_id: ScratchpadId,
    #[serde(default)]
    include_comments: bool,
}

#[derive(Debug, Deserialize)]
struct ScratchpadCommentsParams {
    project_id: ProjectId,
    scratchpad_id: ScratchpadId,
    #[serde(default)]
    include_resolved: bool,
}

#[derive(Debug, Deserialize)]
struct CreateScratchpadCommentParams {
    project_id: ProjectId,
    scratchpad_id: ScratchpadId,
    body: String,
    #[serde(default)]
    quote: Option<String>,
    #[serde(default)]
    anchor_start: Option<usize>,
    #[serde(default)]
    anchor_end: Option<usize>,
    #[serde(default)]
    anchor_prefix: Option<String>,
    #[serde(default)]
    anchor_suffix: Option<String>,
    #[serde(default)]
    allow_unanchored: bool,
    #[serde(default)]
    expected_revision: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct UpdateScratchpadCommentParams {
    project_id: ProjectId,
    comment_id: ScratchpadCommentId,
    body: String,
}

#[derive(Debug, Deserialize)]
struct ResolveScratchpadCommentParams {
    project_id: ProjectId,
    comment_id: ScratchpadCommentId,
    #[serde(default = "default_true")]
    resolved: bool,
}

#[derive(Debug, Deserialize)]
struct DeleteScratchpadCommentParams {
    project_id: ProjectId,
    comment_id: ScratchpadCommentId,
}

#[derive(Debug, Deserialize)]
struct CreateScratchpadParams {
    project_id: ProjectId,
    name: String,
    #[serde(default)]
    content: String,
    #[serde(default)]
    tags: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct UpdateScratchpadParams {
    project_id: ProjectId,
    scratchpad_id: ScratchpadId,
    content: String,
    expected_revision: i64,
}

#[derive(Debug, Deserialize)]
struct RenameScratchpadParams {
    project_id: ProjectId,
    scratchpad_id: ScratchpadId,
    name: String,
    expected_revision: i64,
}

#[derive(Debug, Deserialize)]
struct SetScratchpadTagsParams {
    project_id: ProjectId,
    scratchpad_id: ScratchpadId,
    tags: Vec<String>,
    expected_revision: i64,
}

#[derive(Debug, Deserialize)]
struct ScratchpadRevisionParams {
    project_id: ProjectId,
    scratchpad_id: ScratchpadId,
    expected_revision: i64,
}

/// Returns `None` when `method` belongs to another control module.
pub(crate) fn dispatch(method: &str, params: Value, store: &Store) -> Option<ControlResult> {
    match method {
        "coordination.snapshot" => Some(snapshot(params, store)),
        "coordination.todo" => Some(todo_detail(params, store)),
        "coordination.todo_create" => Some(todo_create(params, store)),
        "coordination.todo_update" => Some(todo_update(params, store)),
        "coordination.todo_complete" => Some(todo_complete(params, store)),
        "coordination.todo_comment" => Some(todo_comment(params, store)),
        "coordination.todo_lock" => Some(todo_lock(params, store)),
        "coordination.todo_unlock" => Some(todo_unlock(params, store)),
        "coordination.todo_delete" => Some(todo_delete(params, store)),
        "coordination.todo_transfer" => Some(todo_transfer(params, store)),
        "coordination.todo_reorder" => Some(todo_reorder(params, store)),
        "coordination.todo_set_blockers" => Some(todo_set_blockers(params, store)),
        "coordination.todo_add_blocker" => Some(todo_add_blocker(params, store)),
        "coordination.todo_remove_blocker" => Some(todo_remove_blocker(params, store)),
        "coordination.scratchpad" => Some(scratchpad_read(params, store)),
        "coordination.scratchpad_comments" => Some(scratchpad_comments(params, store)),
        "coordination.scratchpad_comment_create" => Some(scratchpad_comment_create(params, store)),
        "coordination.scratchpad_comment_update" => Some(scratchpad_comment_update(params, store)),
        "coordination.scratchpad_comment_resolve" => {
            Some(scratchpad_comment_resolve(params, store))
        }
        "coordination.scratchpad_comment_delete" => Some(scratchpad_comment_delete(params, store)),
        "coordination.scratchpad_create" => Some(scratchpad_create(params, store)),
        "coordination.scratchpad_update" => Some(scratchpad_update(params, store)),
        "coordination.scratchpad_rename" => Some(scratchpad_rename(params, store)),
        "coordination.scratchpad_set_tags" => Some(scratchpad_set_tags(params, store)),
        "coordination.scratchpad_archive" => Some(scratchpad_archive(params, store)),
        "coordination.scratchpad_delete" => Some(scratchpad_delete(params, store)),
        "coordination.scratchpad_reorder" => Some(scratchpad_reorder(params, store)),
        _ => None,
    }
}

fn snapshot(params: Value, store: &Store) -> ControlResult {
    let params: ProjectParams = params_as(params)?;
    let todos = TodoService::new(store)
        .list(
            params.project_id,
            TodoListQuery {
                sort: TodoSort::Status,
                limit: Some(200),
                ..TodoListQuery::default()
            },
            now_millis(),
        )
        .map_err(todo_error)?;
    let scratchpads = ScratchpadService::new(store)
        .list(
            params.project_id,
            ScratchpadListQuery {
                limit: Some(200),
                ..ScratchpadListQuery::default()
            },
        )
        .map_err(scratchpad_error)?;
    let archived_scratchpads = ScratchpadService::new(store)
        .list(
            params.project_id,
            ScratchpadListQuery {
                archived: true,
                limit: Some(200),
                ..ScratchpadListQuery::default()
            },
        )
        .map_err(scratchpad_error)?;

    Ok(json!({
        "project_id": params.project_id,
        "todos": todos.todos,
        "todo_total_count": todos.total_count,
        "scratchpads": scratchpads.scratchpads,
        "scratchpad_total_count": scratchpads.total_count,
        "archived_scratchpads": archived_scratchpads.scratchpads,
        "archived_scratchpad_total_count": archived_scratchpads.total_count,
    }))
}

fn todo_detail(params: Value, store: &Store) -> ControlResult {
    let params: TodoParams = params_as(params)?;
    let service = TodoService::new(store);
    let now = now_millis();
    let todo = service
        .get(params.project_id, params.todo_id, now)
        .map_err(todo_error)?
        .ok_or((
            "todo_not_found",
            "todo not found in this project".to_owned(),
        ))?;
    let comments = service
        .comment_list(params.project_id, params.todo_id, 0, Some(200), now)
        .map_err(todo_error)?;
    let activity = service
        .activity_list(params.project_id, params.todo_id, now)
        .map_err(todo_error)?;

    Ok(json!({
        "todo": todo,
        "comments": comments.comments,
        "comment_total_count": comments.total_count,
        "activity": activity,
    }))
}

fn todo_create(params: Value, store: &Store) -> ControlResult {
    let params: CreateTodoParams = params_as(params)?;
    let service = TodoService::attributed(store, "user");
    let todo = service
        .create(
            params.project_id,
            NewTodo {
                title: params.title,
                body: params.body,
                priority: params.priority,
                tags: params.tags,
            },
            now_millis(),
        )
        .map_err(todo_error)?;
    if params.blocker_ids.is_empty() {
        Ok(json_value(todo))
    } else {
        service
            .set_blockers(params.project_id, todo.id, params.blocker_ids, now_millis())
            .map(json_value)
            .map_err(todo_error)
    }
}

fn todo_complete(params: Value, store: &Store) -> ControlResult {
    let params: CompleteTodoParams = params_as(params)?;
    let (todo, affected_todo_ids) = TodoService::attributed(store, "user")
        .complete(
            params.project_id,
            params.todo_id,
            "user",
            params.completed,
            false,
            now_millis(),
        )
        .map_err(todo_error)?;
    Ok(json!({
        "todo": todo,
        "affected_todo_ids": affected_todo_ids,
    }))
}

fn todo_update(params: Value, store: &Store) -> ControlResult {
    let params: UpdateTodoParams = params_as(params)?;
    TodoService::attributed(store, "user")
        .update(
            params.project_id,
            params.todo_id,
            UpdateTodo {
                title: params.title,
                body: params.body,
                priority: params.priority,
                status: params.status,
                tags: params.tags,
            },
            now_millis(),
        )
        .map(json_value)
        .map_err(todo_error)
}

fn todo_comment(params: Value, store: &Store) -> ControlResult {
    let params: CommentTodoParams = params_as(params)?;
    TodoService::attributed(store, "user")
        .comment_create(
            params.project_id,
            params.todo_id,
            "user",
            params.body,
            now_millis(),
        )
        .map(json_value)
        .map_err(todo_error)
}

fn todo_lock(params: Value, store: &Store) -> ControlResult {
    let params: TodoParams = params_as(params)?;
    TodoService::attributed(store, "user")
        .lock(
            params.project_id,
            params.todo_id,
            "user",
            20 * 60 * 1_000,
            now_millis(),
        )
        .map(json_value)
        .map_err(todo_error)
}

fn todo_unlock(params: Value, store: &Store) -> ControlResult {
    let params: TodoParams = params_as(params)?;
    TodoService::attributed(store, "user")
        .unlock(params.project_id, params.todo_id, "user", now_millis())
        .map(json_value)
        .map_err(todo_error)
}

fn todo_delete(params: Value, store: &Store) -> ControlResult {
    let params: TodoParams = params_as(params)?;
    let affected_todo_ids = TodoService::new(store)
        .delete(params.project_id, params.todo_id, now_millis())
        .map_err(todo_error)?;
    Ok(json!({
        "todo_id": params.todo_id,
        "affected_todo_ids": affected_todo_ids,
    }))
}

fn todo_transfer(params: Value, store: &Store) -> ControlResult {
    let params: TransferTodoParams = params_as(params)?;
    let (todo, affected_todo_ids) = TodoService::new(store)
        .transfer(
            params.project_id,
            params.todo_id,
            params.target_project_id,
            now_millis(),
        )
        .map_err(todo_error)?;
    Ok(json!({
        "todo": todo,
        "affected_todo_ids": affected_todo_ids,
    }))
}

fn todo_reorder(params: Value, store: &Store) -> ControlResult {
    let params: ReorderParams = params_as(params)?;
    TodoService::new(store)
        .reorder(params.project_id, &params.ordered_ids, now_millis())
        .map_err(todo_error)?;
    snapshot(json!({ "project_id": params.project_id }), store)
}

fn todo_set_blockers(params: Value, store: &Store) -> ControlResult {
    let params: SetTodoBlockersParams = params_as(params)?;
    TodoService::new(store)
        .set_blockers(
            params.project_id,
            params.todo_id,
            params.blocker_ids,
            now_millis(),
        )
        .map(json_value)
        .map_err(todo_error)
}

fn todo_add_blocker(params: Value, store: &Store) -> ControlResult {
    let params: TodoBlockerParams = params_as(params)?;
    TodoService::new(store)
        .add_blocker(
            params.project_id,
            params.todo_id,
            params.blocker_id,
            now_millis(),
        )
        .map(json_value)
        .map_err(todo_error)
}

fn todo_remove_blocker(params: Value, store: &Store) -> ControlResult {
    let params: TodoBlockerParams = params_as(params)?;
    TodoService::new(store)
        .remove_blocker(
            params.project_id,
            params.todo_id,
            params.blocker_id,
            now_millis(),
        )
        .map(json_value)
        .map_err(todo_error)
}

fn scratchpad_read(params: Value, store: &Store) -> ControlResult {
    let params: ScratchpadParams = params_as(params)?;
    let read = ScratchpadService::new(store)
        .read(
            params.project_id,
            params.scratchpad_id,
            ScratchpadReadMode::Full,
            None,
            0,
            None,
        )
        .map_err(scratchpad_error)?;
    let mut response = json!({
        "scratchpad": read.scratchpad,
        "total_lines": read.total_lines,
    });
    if params.include_comments {
        let comments = ScratchpadService::attributed(store, "user")
            .comment_list(params.project_id, params.scratchpad_id, true)
            .map_err(scratchpad_error)?;
        response["comments"] = json!(comments.comments);
        response["comment_total_count"] = json!(comments.total_count);
        response["unresolved_comment_count"] = json!(comments.unresolved_count);
        response["comments_revision"] = json!(comments.comments_revision);
    }
    Ok(response)
}

fn scratchpad_comments(params: Value, store: &Store) -> ControlResult {
    let params: ScratchpadCommentsParams = params_as(params)?;
    ScratchpadService::attributed(store, "user")
        .comment_list(
            params.project_id,
            params.scratchpad_id,
            params.include_resolved,
        )
        .map(json_value)
        .map_err(scratchpad_error)
}

fn scratchpad_comment_create(params: Value, store: &Store) -> ControlResult {
    let params: CreateScratchpadCommentParams = params_as(params)?;
    ScratchpadService::attributed(store, "user")
        .comment_create(
            params.project_id,
            params.scratchpad_id,
            NewScratchpadComment {
                body: params.body,
                quote: params.quote,
                anchor_start: params.anchor_start,
                anchor_end: params.anchor_end,
                anchor_prefix: params.anchor_prefix,
                anchor_suffix: params.anchor_suffix,
                allow_unanchored: params.allow_unanchored,
                expected_revision: params.expected_revision,
            },
            now_millis(),
        )
        .map(json_value)
        .map_err(scratchpad_error)
}

fn scratchpad_comment_update(params: Value, store: &Store) -> ControlResult {
    let params: UpdateScratchpadCommentParams = params_as(params)?;
    ScratchpadService::attributed(store, "user")
        .comment_update(
            params.project_id,
            params.comment_id,
            params.body,
            now_millis(),
        )
        .map(json_value)
        .map_err(scratchpad_error)
}

fn scratchpad_comment_resolve(params: Value, store: &Store) -> ControlResult {
    let params: ResolveScratchpadCommentParams = params_as(params)?;
    ScratchpadService::attributed(store, "user")
        .comment_set_resolved(
            params.project_id,
            params.comment_id,
            params.resolved,
            now_millis(),
        )
        .map(json_value)
        .map_err(scratchpad_error)
}

fn scratchpad_comment_delete(params: Value, store: &Store) -> ControlResult {
    let params: DeleteScratchpadCommentParams = params_as(params)?;
    ScratchpadService::attributed(store, "user")
        .comment_delete(params.project_id, params.comment_id)
        .map(|scratchpad_id| {
            json!({
                "project_id": params.project_id,
                "scratchpad_id": scratchpad_id,
                "comment_id": params.comment_id,
                "deleted": true,
            })
        })
        .map_err(scratchpad_error)
}

fn scratchpad_create(params: Value, store: &Store) -> ControlResult {
    let params: CreateScratchpadParams = params_as(params)?;
    ScratchpadService::attributed(store, "user")
        .write(
            params.project_id,
            None,
            params.name,
            params.content,
            Some(params.tags),
            None,
        )
        .map(|(scratchpad, _)| json_value(scratchpad))
        .map_err(scratchpad_error)
}

fn scratchpad_reorder(params: Value, store: &Store) -> ControlResult {
    let params: ReorderParams = params_as(params)?;
    ScratchpadService::new(store)
        .reorder(params.project_id, &params.ordered_ids)
        .map_err(scratchpad_error)?;
    snapshot(json!({ "project_id": params.project_id }), store)
}

fn scratchpad_update(params: Value, store: &Store) -> ControlResult {
    let params: UpdateScratchpadParams = params_as(params)?;
    let service = ScratchpadService::attributed(store, "user");
    let current = service
        .read(
            params.project_id,
            params.scratchpad_id,
            ScratchpadReadMode::Full,
            None,
            0,
            None,
        )
        .map_err(scratchpad_error)?;
    service
        .write(
            params.project_id,
            Some(params.scratchpad_id),
            current.scratchpad.name,
            params.content,
            None,
            Some(params.expected_revision),
        )
        .map_err(scratchpad_error)?;
    let read = service
        .read(
            params.project_id,
            params.scratchpad_id,
            ScratchpadReadMode::Full,
            None,
            0,
            None,
        )
        .map_err(scratchpad_error)?;
    let comments = service
        .comment_list(params.project_id, params.scratchpad_id, true)
        .map_err(scratchpad_error)?;
    Ok(json!({
        "scratchpad": read.scratchpad,
        "total_lines": read.total_lines,
        "comments": comments.comments,
        "comment_total_count": comments.total_count,
        "unresolved_comment_count": comments.unresolved_count,
        "comments_revision": comments.comments_revision,
    }))
}

fn scratchpad_rename(params: Value, store: &Store) -> ControlResult {
    let params: RenameScratchpadParams = params_as(params)?;
    ScratchpadService::attributed(store, "user")
        .rename(
            params.project_id,
            params.scratchpad_id,
            params.name,
            params.expected_revision,
        )
        .map(json_value)
        .map_err(scratchpad_error)
}

fn scratchpad_set_tags(params: Value, store: &Store) -> ControlResult {
    let params: SetScratchpadTagsParams = params_as(params)?;
    let service = ScratchpadService::attributed(store, "user");
    let current = service
        .read(
            params.project_id,
            params.scratchpad_id,
            ScratchpadReadMode::Full,
            None,
            0,
            None,
        )
        .map_err(scratchpad_error)?;
    service
        .write(
            params.project_id,
            Some(params.scratchpad_id),
            current.scratchpad.name,
            current.scratchpad.content,
            Some(params.tags),
            Some(params.expected_revision),
        )
        .map(|(scratchpad, _)| json_value(scratchpad))
        .map_err(scratchpad_error)
}

fn scratchpad_archive(params: Value, store: &Store) -> ControlResult {
    let params: ScratchpadRevisionParams = params_as(params)?;
    ScratchpadService::attributed(store, "user")
        .archive(
            params.project_id,
            params.scratchpad_id,
            Some(params.expected_revision),
        )
        .map(json_value)
        .map_err(scratchpad_error)
}

fn scratchpad_delete(params: Value, store: &Store) -> ControlResult {
    let params: ScratchpadRevisionParams = params_as(params)?;
    ScratchpadService::new(store)
        .delete(
            params.project_id,
            params.scratchpad_id,
            params.expected_revision,
        )
        .map(|()| {
            json!({
                "scratchpad_id": params.scratchpad_id,
                "deleted": true,
            })
        })
        .map_err(scratchpad_error)
}

fn params_as<T: for<'de> Deserialize<'de>>(params: Value) -> Result<T, (&'static str, String)> {
    serde_json::from_value(params).map_err(|error| ("invalid_params", error.to_string()))
}

fn json_value<T: serde::Serialize>(value: T) -> Value {
    serde_json::to_value(value).expect("coordination response must serialize")
}

const fn medium_priority() -> TodoPriority {
    TodoPriority::Medium
}

const fn default_true() -> bool {
    true
}

fn todo_error(error: TodoServiceError) -> (&'static str, String) {
    (error.code(), error.to_string())
}

fn scratchpad_error(error: ScratchpadServiceError) -> (&'static str, String) {
    (error.code(), error.to_string())
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(i64::MAX)
}
