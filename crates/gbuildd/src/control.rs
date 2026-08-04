//! JSON request dispatch for the authenticated WebSocket control channel.

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use gbuild_core::{Process, ProcessId, ProjectId};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{RegistryError, SharedProcessRegistry};

#[derive(Debug, Deserialize)]
struct ControlRequest {
    #[serde(default)]
    id: Value,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Deserialize)]
struct ProcessIdParams {
    process_id: ProcessId,
}

#[derive(Debug, Default, Deserialize)]
struct ListParams {
    project_id: Option<ProjectId>,
}

#[derive(Debug, Deserialize)]
struct RenameParams {
    process_id: ProcessId,
    name: String,
}

#[derive(Debug, Deserialize)]
struct ProjectParams {
    project_id: ProjectId,
}

#[derive(Debug, Default, Deserialize)]
struct OutputParams {
    process_id: ProcessId,
    offset: Option<u64>,
    max_bytes: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct InputParams {
    process_id: ProcessId,
    data: String,
}

#[derive(Debug, Deserialize)]
struct ResizeParams {
    process_id: ProcessId,
    rows: u16,
    cols: u16,
    #[serde(default)]
    pixel_width: u16,
    #[serde(default)]
    pixel_height: u16,
}

/// Dispatch a control request, retaining todo-211's JSON echo behavior for non-RPC frames.
pub(crate) async fn handle_text(text: &str, registry: &SharedProcessRegistry) -> String {
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return text.to_owned();
    };
    let Ok(request) = serde_json::from_value::<ControlRequest>(value) else {
        return text.to_owned();
    };

    let id = request.id;
    let result = dispatch(&request.method, request.params, registry).await;
    match result {
        Ok(result) => json!({ "id": id, "ok": true, "result": result }).to_string(),
        Err((code, message)) => json!({
            "id": id,
            "ok": false,
            "error": { "code": code, "message": message }
        })
        .to_string(),
    }
}

async fn dispatch(
    method: &str,
    params: Value,
    registry: &SharedProcessRegistry,
) -> Result<Value, (&'static str, String)> {
    let mut registry = registry.lock().await;
    match method {
        "process.raw_output" => {
            let params: OutputParams = params_as(params)?;
            let mut chunk = registry
                .raw_output(
                    params.process_id,
                    params.offset,
                    params.max_bytes.unwrap_or(64 * 1024).clamp(1, 256 * 1024),
                )
                .map_err(registry_error)?;
            let data = BASE64.encode(&chunk.data);
            chunk.data.clear();
            return Ok(json!({
                "data": data,
                "start_offset": chunk.start_offset,
                "end_offset": chunk.end_offset,
                "total_bytes": chunk.total_bytes,
                "truncated": chunk.truncated,
                "status": chunk.status,
            }));
        }
        "process.rendered_output" => {
            let params: ProcessIdParams = params_as(params)?;
            return registry
                .rendered_output(params.process_id)
                .map(json_value)
                .map_err(registry_error);
        }
        "process.send_input" => {
            let params: InputParams = params_as(params)?;
            let data = BASE64
                .decode(params.data)
                .map_err(|error| ("invalid_params", error.to_string()))?;
            return registry
                .send_input(params.process_id, &data)
                .map(json_value)
                .map_err(registry_error);
        }
        "process.resize" => {
            let params: ResizeParams = params_as(params)?;
            return registry
                .resize(
                    params.process_id,
                    params.rows,
                    params.cols,
                    params.pixel_width,
                    params.pixel_height,
                )
                .map(json_value)
                .map_err(registry_error);
        }
        _ => {}
    }

    let result = match method {
        "process.create" => registry.create(process_param(params)?).map(json_value),
        "process.update" => registry.update(process_param(params)?).map(json_value),
        "process.get" => {
            let params: ProcessIdParams = params_as(params)?;
            registry.get(params.process_id).map(json_value)
        }
        "process.list" => {
            let params: ListParams = params_as(params)?;
            registry.list(params.project_id).map(json_value)
        }
        "process.start" => {
            let params: ProcessIdParams = params_as(params)?;
            registry.start(params.process_id).map(json_value)
        }
        "process.stop" => {
            let params: ProcessIdParams = params_as(params)?;
            registry.stop(params.process_id).map(json_value)
        }
        "process.restart" => {
            let params: ProcessIdParams = params_as(params)?;
            registry.restart(params.process_id).map(json_value)
        }
        "process.close" | "process.delete" => {
            let params: ProcessIdParams = params_as(params)?;
            registry.close(params.process_id).map(json_value)
        }
        "process.rename" => {
            let params: RenameParams = params_as(params)?;
            registry
                .rename(params.process_id, params.name)
                .map(json_value)
        }
        "process.select" => {
            let params: ProcessIdParams = params_as(params)?;
            registry.select(params.process_id).map(|process| {
                json!({
                    "selected_process_id": process.id,
                    "process": process,
                })
            })
        }
        "process.start_all_commands" => {
            let params: ProjectParams = params_as(params)?;
            Ok(json_value(registry.start_all_commands(params.project_id)))
        }
        "process.stop_all_commands" => {
            let params: ProjectParams = params_as(params)?;
            Ok(json_value(registry.stop_all_commands(params.project_id)))
        }
        "process.restart_all_commands" => {
            let params: ProjectParams = params_as(params)?;
            Ok(json_value(registry.restart_all_commands(params.project_id)))
        }
        _ => {
            return Err((
                "method_not_found",
                format!("unknown control method {method:?}"),
            ));
        }
    };
    result.map_err(registry_error)
}

fn process_param(params: Value) -> Result<Process, (&'static str, String)> {
    let value = params.get("process").cloned().unwrap_or(params);
    params_as(value)
}

fn params_as<T: for<'de> Deserialize<'de>>(params: Value) -> Result<T, (&'static str, String)> {
    serde_json::from_value(params).map_err(|error| ("invalid_params", error.to_string()))
}

fn json_value(value: impl serde::Serialize) -> Value {
    serde_json::to_value(value).expect("serializing process control result cannot fail")
}

fn registry_error(error: RegistryError) -> (&'static str, String) {
    (error.code(), error.to_string())
}
