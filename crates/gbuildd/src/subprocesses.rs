//! Fresh descendant inspection and narrowly-scoped child signaling for control RPCs.

use gbuild_core::ProcessId;
use serde::Deserialize;
use serde_json::{Value, json};
use sysinfo::{Pid, Signal, System};

use crate::{ProcessRegistry, inspect_process_tree, inspect_process_tree_in};

type ControlResult = Result<Value, (&'static str, String)>;

#[derive(Debug, Deserialize)]
struct ProcessParams {
    process_id: ProcessId,
}

#[derive(Debug, Deserialize)]
struct KillParams {
    process_id: ProcessId,
    pid: u32,
    #[serde(default)]
    force: bool,
}

pub(crate) fn dispatch(
    method: &str,
    params: Value,
    registry: &mut ProcessRegistry,
) -> Option<ControlResult> {
    match method {
        "process.subprocesses" => Some(list(params, registry)),
        "process.kill_subprocess" => Some(kill(params, registry)),
        _ => None,
    }
}

fn list(params: Value, registry: &mut ProcessRegistry) -> ControlResult {
    let params: ProcessParams = parse(params)?;
    let root_pid = running_root_pid(registry, params.process_id)?;
    let subprocesses = inspect_process_tree(root_pid);
    Ok(json!({
        "process_id": params.process_id,
        "root_pid": root_pid,
        "subprocesses": subprocesses,
    }))
}

fn kill(params: Value, registry: &mut ProcessRegistry) -> ControlResult {
    let params: KillParams = parse(params)?;
    let root_pid = running_root_pid(registry, params.process_id)?;
    if params.pid == root_pid {
        return Err(not_descendant(params.pid, params.process_id));
    }

    // Validation and signaling intentionally share this immutable process table. This avoids
    // authorizing a PID from one snapshot and looking it up after it could have been reused.
    let system = System::new_all();
    let descendants = inspect_process_tree_in(&system, root_pid);
    if !descendants.iter().any(|child| child.pid == params.pid) {
        return Err(not_descendant(params.pid, params.process_id));
    }
    let Some(target) = system.process(Pid::from_u32(params.pid)) else {
        return Err(not_descendant(params.pid, params.process_id));
    };
    let (signal, signal_name) = if params.force {
        (Signal::Kill, "kill")
    } else {
        (Signal::Term, "term")
    };
    match target.kill_with(signal) {
        Some(true) => Ok(json!({
            "process_id": params.process_id,
            "pid": params.pid,
            "signal": signal_name,
            "delivered": true,
        })),
        Some(false) => Err((
            "subprocess_signal_failed",
            format!("failed to signal subprocess {}", params.pid),
        )),
        None => Err((
            "subprocess_signal_unsupported",
            format!("signal {signal_name} is not supported on this platform"),
        )),
    }
}

fn running_root_pid(
    registry: &mut ProcessRegistry,
    process_id: ProcessId,
) -> Result<u32, (&'static str, String)> {
    let process = registry
        .get(process_id)
        .map_err(|error| (error.code(), error.to_string()))?;
    process.pid.ok_or_else(|| {
        (
            "process_not_running",
            format!("process {process_id} is not running"),
        )
    })
}

fn parse<T: for<'de> Deserialize<'de>>(params: Value) -> Result<T, (&'static str, String)> {
    serde_json::from_value(params).map_err(|error| ("invalid_params", error.to_string()))
}

fn not_descendant(pid: u32, process_id: ProcessId) -> (&'static str, String) {
    (
        "subprocess_not_found",
        format!("PID {pid} is not a live descendant of process {process_id}"),
    )
}
