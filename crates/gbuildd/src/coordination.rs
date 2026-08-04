//! Read-only coordination RPCs used by the desktop todo and scratchpad views.

use std::time::{SystemTime, UNIX_EPOCH};

use gbuild_core::{
    ProjectId, ScratchpadId, ScratchpadListQuery, ScratchpadReadMode, ScratchpadService,
    ScratchpadServiceError, Store, TodoId, TodoListQuery, TodoService, TodoServiceError, TodoSort,
};
use serde::Deserialize;
use serde_json::{Value, json};

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
struct ScratchpadParams {
    project_id: ProjectId,
    scratchpad_id: ScratchpadId,
}

/// Returns `None` when `method` belongs to another control module.
pub(crate) fn dispatch(method: &str, params: Value, store: &Store) -> Option<ControlResult> {
    match method {
        "coordination.snapshot" => Some(snapshot(params, store)),
        "coordination.todo" => Some(todo_detail(params, store)),
        "coordination.scratchpad" => Some(scratchpad_read(params, store)),
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

    Ok(json!({
        "project_id": params.project_id,
        "todos": todos.todos,
        "todo_total_count": todos.total_count,
        "scratchpads": scratchpads.scratchpads,
        "scratchpad_total_count": scratchpads.total_count,
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

fn params_as<T: for<'de> Deserialize<'de>>(params: Value) -> Result<T, (&'static str, String)> {
    serde_json::from_value(params).map_err(|error| ("invalid_params", error.to_string()))
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
