//! Coordination RPCs used by the desktop todo and scratchpad views.

use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use serde_json::{Value, json};
use workman_core::{
    NewTodo, ProjectId, ScratchpadId, ScratchpadListQuery, ScratchpadReadMode, ScratchpadService,
    ScratchpadServiceError, Store, TodoId, TodoListQuery, TodoPriority, TodoService,
    TodoServiceError, TodoSort,
};

pub(crate) type ControlResult = Result<Value, (&'static str, String)>;

#[derive(Debug, Deserialize)]
struct ProjectParams {
    project_id: ProjectId,
}

#[derive(Debug, Deserialize)]
struct TodoParams {
    project_id: ProjectId,
    todo_id: TodoId,
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
}

#[derive(Debug, Deserialize)]
struct CompleteTodoParams {
    project_id: ProjectId,
    todo_id: TodoId,
    #[serde(default = "default_true")]
    completed: bool,
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
        "coordination.todo_complete" => Some(todo_complete(params, store)),
        "coordination.todo_comment" => Some(todo_comment(params, store)),
        "coordination.scratchpad" => Some(scratchpad_read(params, store)),
        "coordination.scratchpad_create" => Some(scratchpad_create(params, store)),
        "coordination.scratchpad_update" => Some(scratchpad_update(params, store)),
        "coordination.scratchpad_rename" => Some(scratchpad_rename(params, store)),
        "coordination.scratchpad_archive" => Some(scratchpad_archive(params, store)),
        "coordination.scratchpad_delete" => Some(scratchpad_delete(params, store)),
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

    Ok(json!({
        "todo": todo,
        "comments": comments.comments,
        "comment_total_count": comments.total_count,
    }))
}

fn todo_create(params: Value, store: &Store) -> ControlResult {
    let params: CreateTodoParams = params_as(params)?;
    TodoService::new(store)
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
        .map(json_value)
        .map_err(todo_error)
}

fn todo_complete(params: Value, store: &Store) -> ControlResult {
    let params: CompleteTodoParams = params_as(params)?;
    let (todo, affected_todo_ids) = TodoService::new(store)
        .complete(
            params.project_id,
            params.todo_id,
            "desktop-ui",
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

fn todo_comment(params: Value, store: &Store) -> ControlResult {
    let params: CommentTodoParams = params_as(params)?;
    TodoService::new(store)
        .comment_create(
            params.project_id,
            params.todo_id,
            "desktop-ui",
            params.body,
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
    Ok(json!({
        "scratchpad": read.scratchpad,
        "total_lines": read.total_lines,
    }))
}

fn scratchpad_create(params: Value, store: &Store) -> ControlResult {
    let params: CreateScratchpadParams = params_as(params)?;
    ScratchpadService::new(store)
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

fn scratchpad_update(params: Value, store: &Store) -> ControlResult {
    let params: UpdateScratchpadParams = params_as(params)?;
    let service = ScratchpadService::new(store);
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
    Ok(json!({
        "scratchpad": read.scratchpad,
        "total_lines": read.total_lines,
    }))
}

fn scratchpad_rename(params: Value, store: &Store) -> ControlResult {
    let params: RenameScratchpadParams = params_as(params)?;
    ScratchpadService::new(store)
        .rename(
            params.project_id,
            params.scratchpad_id,
            params.name,
            params.expected_revision,
        )
        .map(json_value)
        .map_err(scratchpad_error)
}

fn scratchpad_archive(params: Value, store: &Store) -> ControlResult {
    let params: ScratchpadRevisionParams = params_as(params)?;
    ScratchpadService::new(store)
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
