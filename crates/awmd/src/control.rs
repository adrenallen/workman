//! JSON request dispatch for the authenticated WebSocket control channel.

use std::{path::Path, time::Duration};

use awm_core::{
    AgentToolId, Process, ProcessId, ProcessKind, ProcessSource, ProcessStatus, Project, ProjectId,
    Store,
};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    DEFAULT_PORT_WAIT, MAX_PORT_WAIT, ReadinessError, ReadinessService, RegistryError,
    SharedProcessRegistry,
};

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

#[derive(Debug, Deserialize)]
struct WorkingDirectoryParams {
    project_id: ProjectId,
    #[serde(default)]
    working_dir: String,
}

#[derive(Debug, Deserialize)]
struct SaveYmlCommandParams {
    project_id: ProjectId,
    name: String,
    command: String,
    #[serde(default)]
    working_dir: String,
    #[serde(default)]
    auto_start: bool,
    #[serde(default)]
    auto_restart: bool,
}

#[derive(Debug, Deserialize)]
struct RegisterProjectParams {
    path: String,
}

#[derive(Debug, Deserialize)]
struct RenameProjectParams {
    project_id: ProjectId,
    name: String,
}

#[derive(Debug, Deserialize)]
struct ProjectReorderParams {
    ordered_ids: Vec<ProjectId>,
}

#[derive(Debug, Deserialize)]
struct ProcessReorderParams {
    project_id: ProjectId,
    kind: ProcessKind,
    ordered_ids: Vec<ProcessId>,
}

#[derive(Debug, Serialize)]
struct ProjectSummary {
    #[serde(flatten)]
    project: Project,
    status: &'static str,
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
    #[serde(default)]
    submit: bool,
    /// Bypass the rendered-dialog guard for an intentional text response.
    #[serde(default)]
    force: bool,
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

#[derive(Debug, Deserialize)]
struct WaitForPortParams {
    process_id: ProcessId,
    timeout_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct TrustProcessParams {
    process_id: ProcessId,
    expected_hash: String,
}

#[derive(Debug, Deserialize)]
struct SpawnTerminalParams {
    project_id: ProjectId,
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AgentToolParams {
    tool: AgentToolInput,
}

#[derive(Debug, Deserialize)]
struct AgentToolInput {
    #[serde(default)]
    id: Option<AgentToolId>,
    name: String,
    command: String,
    tool_type: String,
    enabled: bool,
}

#[derive(Debug, Deserialize)]
struct AgentToolIdParams {
    agent_tool_id: AgentToolId,
}

#[derive(Debug, Deserialize)]
struct AgentToolConfigWriteParams {
    agent_tool_id: AgentToolId,
    confirm_write: bool,
    expected_preview_sha256: String,
}

#[derive(Debug, Deserialize)]
struct AgentToolDeepCheckParams {
    project_id: ProjectId,
    agent_tool_id: AgentToolId,
    #[serde(default)]
    timeout_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct SpawnAgentParams {
    project_id: ProjectId,
    agent_tool_id: AgentToolId,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    extra_args: Vec<String>,
    /// Automatically accept narrowly recognized first-run trust dialogs.
    #[serde(default = "default_true")]
    auto_acknowledge_dialogs: bool,
}

const fn default_true() -> bool {
    true
}

/// Dispatch a control request, retaining todo-211's JSON echo behavior for non-RPC frames.
pub(crate) async fn handle_text(
    text: &str,
    registry: &SharedProcessRegistry,
    mcp_url: &str,
) -> String {
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return text.to_owned();
    };
    let Ok(request) = serde_json::from_value::<ControlRequest>(value) else {
        return text.to_owned();
    };

    let id = request.id;
    let result = dispatch(&request.method, request.params, registry, mcp_url).await;
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
    mcp_url: &str,
) -> Result<Value, (&'static str, String)> {
    let readiness = ReadinessService::default();
    match method {
        "services.list" => {
            let params: ListParams = params_as(params)?;
            return readiness
                .services_list(registry, params.project_id)
                .await
                .map(json_value)
                .map_err(readiness_error);
        }
        "process.get_ports" | "process.ports" => {
            let params: ProcessIdParams = params_as(params)?;
            return readiness
                .get_process_ports(registry, params.process_id)
                .await
                .map(json_value)
                .map_err(readiness_error);
        }
        "process.wait_for_bound_port" => {
            let params: WaitForPortParams = params_as(params)?;
            let timeout = params
                .timeout_ms
                .map(Duration::from_millis)
                .unwrap_or(DEFAULT_PORT_WAIT)
                .min(MAX_PORT_WAIT);
            return readiness
                .wait_for_bound_port(registry, params.process_id, timeout)
                .await
                .map(json_value)
                .map_err(readiness_error);
        }
        "agent_tools.health" => {
            let tools = {
                let registry = registry.lock().await;
                crate::mcp::agent_spawning::load_agent_tools(&registry)
                    .map_err(|error| ("agent_tool_error", error))?
            };
            return Ok(json_value(
                crate::runtime_doctor::check_agent_tools(tools).await,
            ));
        }
        "agent_tools.configure_preview" => {
            let params: AgentToolIdParams = params_as(params)?;
            let tool = {
                let registry = registry.lock().await;
                crate::mcp::agent_spawning::load_agent_tool(&registry, params.agent_tool_id)
                    .map_err(|error| ("agent_tool_error", error))?
            };
            return crate::runtime_doctor::config_preview(&tool, mcp_url)
                .map(json_value)
                .map_err(|error| ("agent_config_error", error));
        }
        "agent_tools.configure" => {
            let params: AgentToolConfigWriteParams = params_as(params)?;
            let tool = {
                let registry = registry.lock().await;
                crate::mcp::agent_spawning::load_agent_tool(&registry, params.agent_tool_id)
                    .map_err(|error| ("agent_tool_error", error))?
            };
            return crate::runtime_doctor::apply_config(
                &tool,
                mcp_url,
                params.confirm_write,
                &params.expected_preview_sha256,
            )
            .map(json_value)
            .map_err(|error| ("agent_config_error", error));
        }
        "agent_tools.deep_check" => {
            let params: AgentToolDeepCheckParams = params_as(params)?;
            return crate::mcp::agent_spawning::deep_check_registered_agent(
                registry.clone(),
                params.project_id,
                params.agent_tool_id,
                mcp_url,
                params.timeout_ms,
                None,
            )
            .await
            .map(json_value)
            .map_err(|error| ("deep_check_failed", error));
        }
        _ => {}
    }

    let mut registry = registry.lock().await;
    if let Some(result) = crate::context_actions::dispatch(method, params.clone(), &mut registry) {
        return result;
    }
    if let Some(result) = crate::coordination::dispatch(method, params.clone(), registry.store()) {
        return result;
    }
    if let Some(result) = crate::subprocesses::dispatch(method, params.clone(), &mut registry) {
        return result;
    }
    match method {
        "projects.list" => {
            return project_result(list_projects(registry.store()));
        }
        "projects.register" => {
            let params: RegisterProjectParams = params_as(params)?;
            register_project(registry.store(), &params.path)?;
            return project_result(list_projects(registry.store()));
        }
        "projects.select" => {
            let params: ProjectParams = params_as(params)?;
            select_project(registry.store(), params.project_id)?;
            let _ = crate::lifecycle::auto_start_project(&mut registry, params.project_id);
            return project_result(list_projects(registry.store()));
        }
        "projects.rename" => {
            let params: RenameProjectParams = params_as(params)?;
            rename_project(registry.store(), params.project_id, &params.name)?;
            return project_result(list_projects(registry.store()));
        }
        "project.reorder" => {
            let params: ProjectReorderParams = params_as(params)?;
            registry
                .store_mut()
                .reorder_projects(&params.ordered_ids)
                .map_err(reorder_store_error)?;
            return project_result(list_projects(registry.store()));
        }
        "config.sync" => {
            let params: ProjectParams = params_as(params)?;
            let project = registry
                .store()
                .get_project(params.project_id)
                .map_err(project_store_error)?
                .ok_or(("project_not_found", "project not found".to_owned()))?;
            return crate::lifecycle::sync_project_config(&mut registry, &project)
                .map(|()| json!({ "project_id": params.project_id, "synced": true }))
                .map_err(|error| ("config_error", error.to_string()));
        }
        "config.status" => {
            let params: ProjectParams = params_as(params)?;
            let project = registry
                .store()
                .get_project(params.project_id)
                .map_err(project_store_error)?
                .ok_or(("project_not_found", "project not found".to_owned()))?;
            let root = Path::new(&project.path);
            let canonical = root.join(crate::AWM_CONFIG_FILE);
            let path = crate::project_config_path(root).unwrap_or(canonical);
            return Ok(json!({
                "project_id": params.project_id,
                "path": path,
                "exists": path.is_file(),
            }));
        }
        "config.validate_working_dir" => {
            let params: WorkingDirectoryParams = params_as(params)?;
            return crate::config::validate_project_working_dir(
                registry.store(),
                params.project_id,
                &params.working_dir,
            )
            .map(json_value)
            .map_err(config_error);
        }
        "config.command_save" => {
            let params: SaveYmlCommandParams = params_as(params)?;
            return crate::config::write_awm_yml_command(
                &mut registry,
                params.project_id,
                params.name,
                params.command,
                params.working_dir,
                params.auto_start,
                params.auto_restart,
            )
            .map(json_value)
            .map_err(config_error);
        }
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
            if params.submit
                && !params.force
                && let Some(dialog) = registry
                    .pending_dialog(params.process_id)
                    .map_err(registry_error)?
            {
                return Err((
                    "dialog_pending",
                    format!(
                        "{} is awaiting a dialog response; pass force=true to answer it intentionally.\n\n{}",
                        dialog.classification, dialog.rendered
                    ),
                ));
            }
            return (if params.submit {
                registry.submit_input(params.process_id, &data)
            } else {
                registry.send_input(params.process_id, &data)
            })
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

    match method {
        "agent_tools.list" => {
            return crate::mcp::agent_spawning::load_agent_tools(&registry)
                .map(json_value)
                .map_err(|error| ("agent_tool_error", error));
        }
        "agent_tools.save" => {
            let params: AgentToolParams = params_as(params)?;
            return crate::mcp::agent_spawning::save_agent_tool_from_settings(
                &registry,
                params.tool.id,
                params.tool.name,
                params.tool.command,
                params.tool.tool_type,
                params.tool.enabled,
            )
            .map(json_value)
            .map_err(|error| ("agent_tool_error", error));
        }
        "agent_tools.delete" => {
            let params: AgentToolIdParams = params_as(params)?;
            return crate::mcp::agent_spawning::delete_agent_tool(&registry, params.agent_tool_id)
                .map(|deleted| json!({ "agent_tool_id": params.agent_tool_id, "deleted": deleted }))
                .map_err(|error| ("agent_tool_error", error));
        }
        "agents.spawn" => {
            let params: SpawnAgentParams = params_as(params)?;
            let project = registry
                .store()
                .get_project(params.project_id)
                .map_err(project_store_error)?
                .ok_or(("project_not_found", "project not found".to_owned()))?;
            return crate::mcp::agent_spawning::spawn_registered_agent(
                &mut registry,
                &project,
                params.agent_tool_id,
                params.name,
                params.extra_args,
                mcp_url,
                params.auto_acknowledge_dialogs,
                None,
            )
            .map(json_value)
            .map_err(|error| ("spawn_failed", error));
        }
        _ => {}
    }

    let result = match method {
        "process.create" => registry.create(process_param(params)?).map(json_value),
        "process.update" => registry.update(process_param(params)?).map(json_value),
        "process.get" | "process.status" => {
            let params: ProcessIdParams = params_as(params)?;
            registry.get_status(params.process_id).map(json_value)
        }
        "process.list" => {
            let params: ListParams = params_as(params)?;
            registry.list_statuses(params.project_id).map(json_value)
        }
        "process.reorder" => {
            let params: ProcessReorderParams = params_as(params)?;
            registry
                .store_mut()
                .reorder_processes(params.project_id, params.kind, &params.ordered_ids)
                .map_err(reorder_store_error)?;
            registry
                .list_statuses(Some(params.project_id))
                .map(json_value)
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
        "process.trust_review" => {
            let params: ProcessIdParams = params_as(params)?;
            registry.trust_review(params.process_id).map(json_value)
        }
        "process.trust_yml" => {
            let params: TrustProcessParams = params_as(params)?;
            registry
                .trust_yml_process(params.process_id, &params.expected_hash)
                .map(json_value)
        }
        "process.spawn_terminal" => {
            let params: SpawnTerminalParams = params_as(params)?;
            spawn_terminal(&mut registry, params).map(json_value)
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

fn spawn_terminal(
    registry: &mut crate::ProcessRegistry,
    params: SpawnTerminalParams,
) -> Result<Process, RegistryError> {
    let project = registry
        .store()
        .get_project(params.project_id)?
        .ok_or(RegistryError::NotFound(params.project_id))?;
    let existing = registry.list(Some(params.project_id))?;
    let requested = params
        .name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty());
    let name = requested.map(str::to_owned).unwrap_or_else(|| {
        let mut suffix = 1;
        loop {
            let candidate = if suffix == 1 {
                "Terminal".to_owned()
            } else {
                format!("Terminal {suffix}")
            };
            if existing.iter().all(|process| process.name != candidate) {
                break candidate;
            }
            suffix += 1;
        }
    });
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_owned());
    let process = registry.create(Process {
        id: 0,
        project_id: params.project_id,
        kind: ProcessKind::Terminal,
        name,
        command: Some(shell),
        working_dir: project.path,
        env: Default::default(),
        auto_start: false,
        auto_restart: false,
        restart_when_changed: Vec::new(),
        source: ProcessSource::Local,
        trust_hash: None,
        status: ProcessStatus::Stopped,
        pid: None,
        exit_code: None,
        exit_signal: None,
        exited_at: None,
        agent_tool_id: None,
        spawned_by_process_id: None,
        sort_order: 0,
    })?;
    registry.start(process.id)
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

fn config_error(error: crate::ConfigError) -> (&'static str, String) {
    let code = match &error {
        crate::ConfigError::ProjectNotFound(_) => "project_not_found",
        crate::ConfigError::InvalidProcessName | crate::ConfigError::MissingCommand(_) => {
            "invalid_params"
        }
        crate::ConfigError::LocalNameConflict(_) => "process_name_conflict",
        crate::ConfigError::ParentTraversal { .. }
        | crate::ConfigError::WorkingDirectory { .. }
        | crate::ConfigError::NotDirectory { .. }
        | crate::ConfigError::OutsideProject { .. } => "invalid_working_directory",
        _ => "config_error",
    };
    (code, error.to_string())
}

fn readiness_error(error: ReadinessError) -> (&'static str, String) {
    (error.code(), error.to_string())
}

fn project_result(
    result: Result<Vec<ProjectSummary>, (&'static str, String)>,
) -> Result<Value, (&'static str, String)> {
    result.map(json_value)
}

fn register_project(store: &Store, path: &str) -> Result<(), (&'static str, String)> {
    let canonical = std::fs::canonicalize(path).map_err(|error| {
        (
            "invalid_project_path",
            format!("could not open project directory: {error}"),
        )
    })?;
    if !canonical.is_dir() {
        return Err((
            "invalid_project_path",
            "project path must be a directory".to_owned(),
        ));
    }
    let canonical = canonical.to_string_lossy().into_owned();
    let name = Path::new(&canonical)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("Project")
        .to_owned();
    let connection = store.connection();
    connection
        .execute(
            "INSERT INTO projects (path, name, sort_order)
             VALUES (?1, ?2, (SELECT COALESCE(MAX(sort_order), -1) + 1 FROM projects))
             ON CONFLICT(path) DO NOTHING",
            (&canonical, &name),
        )
        .map_err(project_store_error)?;
    connection
        .execute(
            "UPDATE projects SET selected = 1
             WHERE path = ?1 AND NOT EXISTS (
                SELECT 1 FROM projects WHERE selected = 1 AND path <> ?1
             )",
            [&canonical],
        )
        .map_err(project_store_error)?;
    Ok(())
}

fn select_project(store: &Store, project_id: ProjectId) -> Result<(), (&'static str, String)> {
    let exists = store
        .connection()
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM projects WHERE id = ?1)",
            [project_id],
            |row| row.get::<_, bool>(0),
        )
        .map_err(project_store_error)?;
    if !exists {
        return Err(("project_not_found", "project not found".to_owned()));
    }
    store
        .connection()
        .execute(
            "UPDATE projects SET selected = CASE WHEN id = ?1 THEN 1 ELSE 0 END",
            [project_id],
        )
        .map_err(project_store_error)?;
    Ok(())
}

fn rename_project(
    store: &Store,
    project_id: ProjectId,
    name: &str,
) -> Result<(), (&'static str, String)> {
    let name = name.trim();
    if name.is_empty() {
        return Err((
            "invalid_project_name",
            "project name cannot be empty".to_owned(),
        ));
    }
    let changed = store
        .connection()
        .execute(
            "UPDATE projects SET display_name = ?1 WHERE id = ?2",
            (name, project_id),
        )
        .map_err(project_store_error)?;
    if changed == 0 {
        return Err(("project_not_found", "project not found".to_owned()));
    }
    Ok(())
}

fn list_projects(store: &Store) -> Result<Vec<ProjectSummary>, (&'static str, String)> {
    let mut statement = store
        .connection()
        .prepare(
            "SELECT id, path, name, display_name, icon, selected, sort_order
             FROM projects
             ORDER BY sort_order, id",
        )
        .map_err(project_store_error)?;
    let projects = statement
        .query_map([], |row| {
            Ok(Project {
                id: row.get(0)?,
                path: row.get(1)?,
                name: row.get(2)?,
                display_name: row.get(3)?,
                icon: row.get(4)?,
                selected: row.get(5)?,
                sort_order: row.get(6)?,
            })
        })
        .map_err(project_store_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(project_store_error)?;

    projects
        .into_iter()
        .map(|project| {
            let processes = store
                .list_processes(Some(project.id))
                .map_err(|error| ("store_error", error.to_string()))?;
            let status = if processes
                .iter()
                .any(|process| process.status == ProcessStatus::Crashed)
            {
                "error"
            } else if processes.iter().any(|process| {
                matches!(
                    process.status,
                    ProcessStatus::Running | ProcessStatus::Starting
                )
            }) {
                "running"
            } else {
                "idle"
            };
            Ok(ProjectSummary { project, status })
        })
        .collect()
}

fn project_store_error(error: impl std::fmt::Display) -> (&'static str, String) {
    ("store_error", error.to_string())
}

fn reorder_store_error(error: impl std::fmt::Display) -> (&'static str, String) {
    ("invalid_reorder", error.to_string())
}
