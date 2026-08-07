//! Destructive desktop context-menu actions with server-side confirmation gates.

use serde::Deserialize;
use serde_json::{Value, json};
use workman_core::{ProcessId, ProjectId};

use crate::ProcessRegistry;

pub(crate) type ControlResult = Result<Value, (&'static str, String)>;

#[derive(Debug, Deserialize)]
struct ProcessParams {
    process_id: ProcessId,
    #[serde(default)]
    confirm_kill: bool,
    #[serde(default)]
    cascade: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct RemoveProjectParams {
    project_id: ProjectId,
    #[serde(default)]
    confirm_remove: bool,
}

/// Returns `None` when the method belongs to another control module.
pub(crate) fn dispatch(
    method: &str,
    params: Value,
    registry: &mut ProcessRegistry,
) -> Option<ControlResult> {
    match method {
        "process.kill" => Some(kill_process(params, registry)),
        "projects.remove" => Some(remove_project(params, registry)),
        _ => None,
    }
}

fn kill_process(params: Value, registry: &mut ProcessRegistry) -> ControlResult {
    let params: ProcessParams = params_as(params)?;
    if !params.confirm_kill {
        return Err((
            "confirmation_required",
            "set confirm_kill=true to immediately kill this process tree".to_owned(),
        ));
    }
    registry
        .kill_with_descendants(params.process_id, params.cascade.unwrap_or(true))
        .map(json_value)
        .map_err(registry_error)
}

fn remove_project(params: Value, registry: &mut ProcessRegistry) -> ControlResult {
    let params: RemoveProjectParams = params_as(params)?;
    if !params.confirm_remove {
        return Err((
            "confirmation_required",
            "set confirm_remove=true to remove this project from workman".to_owned(),
        ));
    }

    let project = registry
        .store()
        .get_project(params.project_id)
        .map_err(store_error)?
        .ok_or(("project_not_found", "project not found".to_owned()))?;
    let processes = registry
        .list(Some(params.project_id))
        .map_err(registry_error)?;
    for process in processes {
        registry.close(process.id).map_err(registry_error)?;
    }
    let deleted = registry
        .store()
        .delete_project(params.project_id)
        .map_err(store_error)?;
    if !deleted {
        return Err(("project_not_found", "project not found".to_owned()));
    }

    let mut selected_project_id = None;
    if project.selected
        && let Some(mut next) = registry
            .store()
            .list_projects()
            .map_err(store_error)?
            .into_iter()
            .next()
    {
        next.selected = true;
        selected_project_id = Some(next.id);
        registry.store().put_project(&next).map_err(store_error)?;
    }

    Ok(json!({
        "project_id": params.project_id,
        "removed": true,
        "selected_project_id": selected_project_id,
        "files_removed": false,
    }))
}

fn params_as<T: for<'de> Deserialize<'de>>(params: Value) -> Result<T, (&'static str, String)> {
    serde_json::from_value(params).map_err(|error| ("invalid_params", error.to_string()))
}

fn json_value(value: impl serde::Serialize) -> Value {
    serde_json::to_value(value).expect("context action result must serialize")
}

fn registry_error(error: crate::RegistryError) -> (&'static str, String) {
    (error.code(), error.to_string())
}

fn store_error(error: workman_core::StoreError) -> (&'static str, String) {
    ("store_error", error.to_string())
}
