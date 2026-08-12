//! Destructive desktop context-menu actions with server-side confirmation gates.

use serde::Deserialize;
use serde_json::Value;
use workman_core::ProcessId;

use crate::ProcessRegistry;

pub(crate) type ControlResult = Result<Value, (&'static str, String)>;

#[derive(Debug, Deserialize)]
struct ProcessParams {
    process_id: ProcessId,
    #[serde(default)]
    confirm_kill: bool,
}

/// Returns `None` when the method belongs to another control module.
pub(crate) fn dispatch(
    method: &str,
    params: Value,
    registry: &mut ProcessRegistry,
) -> Option<ControlResult> {
    match method {
        "process.kill" => Some(kill_process(params, registry)),
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
        .kill(params.process_id)
        .map(json_value)
        .map_err(registry_error)
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
