//! JSON request dispatch for the authenticated WebSocket control channel.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::Duration,
};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use workman_core::{
    AgentTemplateId, AgentToolId, Process, ProcessId, ProcessKind, ProcessSource, ProcessStatus,
    Project, ProjectFolder, ProjectId, ProjectLayoutEntry, QuickPrompt, Store, TimerId,
};

use crate::{
    DEFAULT_PORT_WAIT, MAX_PORT_WAIT, ReadinessError, ReadinessService, RegistryError,
    SharedProcessRegistry,
    timer_events::{TimerLifecycleEvent, TimerLifecycleHub, TimerLifecycleKind},
    timers::{TimerEdit, TimerError, TimerService},
};

pub(crate) mod agent_icons;
mod project_icons;
mod terminal_theme;

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

#[derive(Debug, Deserialize)]
struct TimerProjectParams {
    project_id: ProjectId,
}

#[derive(Debug, Deserialize)]
struct TimerTargetParams {
    project_id: ProjectId,
    timer_id: TimerId,
}

#[derive(Debug, Deserialize)]
struct TimerUpdateParams {
    project_id: ProjectId,
    timer_id: TimerId,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    due_at: Option<i64>,
    #[serde(default)]
    delay_ms: Option<i64>,
    #[serde(default)]
    interval_ms: Option<i64>,
    #[serde(default)]
    paused: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
struct NotificationsListParams {
    read: Option<bool>,
    limit: Option<usize>,
}

#[derive(Debug, Default, Deserialize)]
struct NotificationsMarkReadParams {
    notification_id: Option<i64>,
    #[serde(default)]
    all: bool,
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
struct ProfileParams {
    profile_id: i64,
}

#[derive(Debug, Deserialize)]
struct ProfileCreateParams {
    name: String,
    #[serde(default = "default_true")]
    copy_current: bool,
}

#[derive(Debug, Deserialize)]
struct ProfileRenameParams {
    profile_id: i64,
    name: String,
}

#[derive(Debug, Deserialize)]
struct ProfileSwitchParams {
    profile_id: i64,
    #[serde(default)]
    confirm_stop_running: bool,
}

#[derive(Debug, Deserialize)]
struct ProfileDeleteParams {
    profile_id: i64,
    #[serde(default)]
    confirm_delete: bool,
}

#[derive(Debug, Deserialize)]
struct ProfileExportParams {
    profile_id: i64,
    path: String,
}

#[derive(Debug, Deserialize)]
struct ProfileImportParams {
    path: String,
    #[serde(default)]
    name: Option<String>,
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
    env: BTreeMap<String, String>,
    #[serde(default)]
    auto_start: bool,
    #[serde(default)]
    auto_restart: bool,
    #[serde(default)]
    restart_when_changed: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct UpdateCommandParams {
    process_id: ProcessId,
    name: String,
    command: String,
    #[serde(default)]
    working_dir: String,
    #[serde(default)]
    env: BTreeMap<String, String>,
    #[serde(default)]
    auto_start: bool,
    #[serde(default)]
    auto_restart: bool,
    #[serde(default)]
    restart_when_changed: Vec<String>,
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
struct UpdateProjectSettingsParams {
    project_id: ProjectId,
    display_name: String,
    #[serde(default)]
    icon: Option<String>,
    #[serde(default)]
    icon_color: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CustomProjectIconParams {
    project_id: ProjectId,
    source_path: String,
}

#[derive(Debug, Deserialize)]
struct ProjectReorderParams {
    ordered_ids: Vec<ProjectId>,
}

#[derive(Debug, Deserialize)]
struct ProjectFolderNameParams {
    name: String,
}

#[derive(Debug, Deserialize)]
struct ProjectFolderRenameParams {
    folder_id: i64,
    name: String,
}

#[derive(Debug, Deserialize)]
struct ProjectFolderCollapseParams {
    folder_id: i64,
    collapsed: bool,
}

#[derive(Debug, Deserialize)]
struct ProjectFolderDeleteParams {
    folder_id: i64,
    #[serde(default)]
    confirm_delete: bool,
}

#[derive(Debug, Deserialize)]
struct ProjectLayoutParams {
    entries: Vec<ProjectLayoutEntry>,
}

#[derive(Debug, Default, Deserialize)]
struct WorktreeScopeParams {
    #[serde(default)]
    project_id: Option<ProjectId>,
    #[serde(default)]
    refresh_pull_requests: bool,
}

#[derive(Debug, Deserialize)]
struct WorktreeRefParams {
    #[serde(default)]
    project_id: Option<ProjectId>,
    #[serde(rename = "ref")]
    requested_ref: String,
}

#[derive(Debug, Deserialize)]
struct WorktreeCreateParams {
    #[serde(default)]
    project_id: Option<ProjectId>,
    branch: String,
    #[serde(default)]
    from_ref: Option<String>,
    #[serde(default)]
    managed_root: Option<String>,
    #[serde(default)]
    preferences: BTreeMap<String, String>,
    #[serde(default)]
    env_policy: Option<crate::worktrees::EnvPortPolicy>,
    #[serde(default)]
    remember_env_policy: bool,
}

#[derive(Debug, Deserialize)]
struct WorktreeForkParams {
    #[serde(default)]
    project_id: Option<ProjectId>,
    branch: String,
    #[serde(default)]
    managed_root: Option<String>,
    #[serde(default)]
    preferences: BTreeMap<String, String>,
    #[serde(default)]
    env_policy: Option<crate::worktrees::EnvPortPolicy>,
    #[serde(default)]
    remember_env_policy: bool,
}

#[derive(Debug, Deserialize)]
struct WorktreeAdoptParams {
    path: String,
    #[serde(default)]
    preferences: BTreeMap<String, String>,
}

#[derive(Debug, Default, Deserialize)]
struct WorktreeRemoveParams {
    #[serde(default)]
    project_id: Option<ProjectId>,
    #[serde(default)]
    confirm_remove: bool,
    #[serde(default)]
    confirm_stop_running: bool,
    #[serde(default)]
    delete_from_disk: bool,
    #[serde(default)]
    force_dirty: bool,
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
    icon_color: Option<String>,
    icon_image: Option<project_icons::ProjectIconImage>,
    repository_id: Option<i64>,
    repository_root: Option<String>,
    parent_project_id: Option<ProjectId>,
    branch: Option<String>,
    worktree_managed: bool,
    folder_id: Option<i64>,
    status: &'static str,
}

#[derive(Debug, Serialize)]
struct ProjectRailSnapshot {
    projects: Vec<ProjectSummary>,
    folders: Vec<ProjectFolder>,
    layout: Vec<ProjectLayoutEntry>,
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
struct AgentToolIconParams {
    agent_tool_id: AgentToolId,
    source_path: String,
}

#[derive(Debug, Deserialize)]
struct AgentToolOrderParams {
    agent_tool_ids: Vec<AgentToolId>,
}

#[derive(Debug, Deserialize)]
struct QuickPromptParams {
    prompt: QuickPromptInput,
}

#[derive(Debug, Deserialize)]
struct QuickPromptInput {
    #[serde(default)]
    id: Option<i64>,
    name: String,
    body: String,
}

#[derive(Debug, Deserialize)]
struct QuickPromptIdParams {
    quick_prompt_id: i64,
}

#[derive(Debug, Deserialize)]
struct QuickPromptOrderParams {
    quick_prompt_ids: Vec<i64>,
}

#[derive(Debug, Deserialize)]
struct AgentTemplateParams {
    template: AgentTemplateInput,
}

#[derive(Debug, Deserialize)]
struct AgentTemplateInput {
    #[serde(default)]
    id: Option<AgentTemplateId>,
    name: String,
    agent_tool_id: AgentToolId,
    #[serde(default)]
    extra_args: Vec<String>,
    #[serde(default)]
    prompt: String,
}

#[derive(Debug, Deserialize)]
struct AgentTemplateIdParams {
    agent_template_id: AgentTemplateId,
}

#[derive(Debug, Deserialize)]
struct AgentTemplateOrderParams {
    agent_template_ids: Vec<AgentTemplateId>,
}

#[derive(Debug, Deserialize)]
struct UserShellParams {
    #[serde(default)]
    shell: Option<String>,
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
    /// Optional registered agent. Overrides an agent template's default when both are present.
    #[serde(default)]
    agent_tool_id: Option<AgentToolId>,
    /// Optional template. Its prompt always applies; its launch arguments apply only when the
    /// effective agent is the template default.
    #[serde(default)]
    agent_template_id: Option<AgentTemplateId>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    extra_args: Vec<String>,
    #[serde(default)]
    prompt: Option<String>,
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
    input_router: &crate::ProcessInputRouter,
    mcp_url: &str,
    data_dir: &Path,
    timer_events: &TimerLifecycleHub,
) -> String {
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return text.to_owned();
    };
    let Ok(request) = serde_json::from_value::<ControlRequest>(value) else {
        return text.to_owned();
    };

    let id = request.id;
    let result = dispatch(
        &request.method,
        request.params,
        registry,
        input_router,
        mcp_url,
        data_dir,
        timer_events,
    )
    .await;
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
    input_router: &crate::ProcessInputRouter,
    mcp_url: &str,
    data_dir: &Path,
    timer_events: &TimerLifecycleHub,
) -> Result<Value, (&'static str, String)> {
    let readiness = ReadinessService::default();
    match method {
        "process.send_input" => {
            let params: InputParams = params_as(params.clone())?;
            if !params.submit {
                let data = BASE64
                    .decode(params.data)
                    .map_err(|error| ("invalid_params", error.to_string()))?;
                return input_router
                    .send_input(params.process_id, &data)
                    .map(json_value)
                    .map_err(registry_error);
            }
        }
        "agents.spawn" => {
            let params: SpawnAgentParams = params_as(params.clone())?;
            crate::mcp::agent_spawning::validate_initial_prompt(params.prompt.as_deref())
                .map_err(|error| ("invalid_params", error))?;
            let project = {
                let registry = registry.lock().await;
                registry
                    .store()
                    .get_project(params.project_id)
                    .map_err(project_store_error)?
                    .ok_or(("project_not_found", "project not found".to_owned()))?
            };
            return crate::mcp::agent_spawning::spawn_registered_agent(
                registry.clone(),
                project,
                params.agent_tool_id,
                params.agent_template_id,
                params.name,
                params.extra_args,
                params.prompt,
                mcp_url,
                params.auto_acknowledge_dialogs,
                None,
            )
            .await
            .map(json_value)
            .map_err(|error| ("spawn_failed", error));
        }
        "projects.remove" | "project.remove" | "project_remove" => {
            let params: WorktreeRemoveParams = params_as(params)?;
            let project_id = control_worktree_project_id(registry, params.project_id).await?;
            return crate::worktrees::remove(
                registry,
                crate::worktrees::RemoveWorktree {
                    project_id,
                    confirm_remove: params.confirm_remove,
                    confirm_stop_running: params.confirm_stop_running,
                    delete_from_disk: params.delete_from_disk,
                    force_dirty: params.force_dirty,
                    confirm_branch: None,
                },
            )
            .await
            .map(json_value)
            .map_err(worktree_error);
        }
        "worktree.list" | "worktree_list" => {
            let params: WorktreeScopeParams = params_as(params)?;
            let project_id = control_worktree_project_id(registry, params.project_id).await?;
            return crate::worktrees::list_for_project_refresh(
                registry,
                project_id,
                params.refresh_pull_requests,
            )
            .await
            .map(json_value)
            .map_err(worktree_error);
        }
        "worktree.branches" | "worktree_branches" => {
            let params: WorktreeScopeParams = params_as(params)?;
            let project_id = control_worktree_project_id(registry, params.project_id).await?;
            return crate::worktrees::origin_branches_for_project(registry, project_id)
                .await
                .map(json_value)
                .map_err(worktree_error);
        }
        "worktree.ref_validate" | "worktree_ref_validate" => {
            let params: WorktreeRefParams = params_as(params)?;
            let project_id = control_worktree_project_id(registry, params.project_id).await?;
            return crate::worktrees::validate_ref_for_project(
                registry,
                project_id,
                &params.requested_ref,
            )
            .await
            .map(json_value)
            .map_err(worktree_error);
        }
        "worktree.create" | "worktree_create" => {
            let params: WorktreeCreateParams = params_as(params)?;
            let project_id = control_worktree_project_id(registry, params.project_id).await?;
            return crate::worktrees::create(
                registry,
                crate::worktrees::CreateWorktree {
                    source_project_id: project_id,
                    branch: params.branch,
                    from_ref: params.from_ref,
                    managed_root: params.managed_root.map(PathBuf::from),
                    preferences: params.preferences,
                    env_policy: params.env_policy,
                    remember_env_policy: params.remember_env_policy,
                },
            )
            .await
            .map(json_value)
            .map_err(worktree_error);
        }
        "worktree.fork" | "worktree_fork" => {
            let params: WorktreeForkParams = params_as(params)?;
            let project_id = control_worktree_project_id(registry, params.project_id).await?;
            return crate::worktrees::fork(
                registry,
                crate::worktrees::ForkWorktree {
                    source_project_id: project_id,
                    branch: params.branch,
                    managed_root: params.managed_root.map(PathBuf::from),
                    preferences: params.preferences,
                    env_policy: params.env_policy,
                    remember_env_policy: params.remember_env_policy,
                },
            )
            .await
            .map(json_value)
            .map_err(worktree_error);
        }
        "worktree.env_forget" | "worktree_env_forget" => {
            let params: WorktreeScopeParams = params_as(params)?;
            let project_id = control_worktree_project_id(registry, params.project_id).await?;
            return crate::worktrees::forget_env_preference(registry, project_id)
                .await
                .map(json_value)
                .map_err(worktree_error);
        }
        "worktree.health" | "worktree_health" => {
            return Ok(json_value(crate::worktrees::health(registry).await));
        }
        "worktree.adopt" | "worktree_adopt" => {
            let params: WorktreeAdoptParams = params_as(params)?;
            return crate::worktrees::adopt(
                registry,
                crate::worktrees::AdoptWorktree {
                    path: PathBuf::from(params.path),
                    preferences: params.preferences,
                },
            )
            .await
            .map(json_value)
            .map_err(worktree_error);
        }
        "worktree.remove" | "worktree_remove" => {
            let params: WorktreeRemoveParams = params_as(params)?;
            let project_id = control_worktree_project_id(registry, params.project_id).await?;
            return crate::worktrees::remove(
                registry,
                crate::worktrees::RemoveWorktree {
                    project_id,
                    confirm_remove: params.confirm_remove,
                    confirm_stop_running: params.confirm_stop_running,
                    delete_from_disk: params.delete_from_disk,
                    force_dirty: params.force_dirty,
                    confirm_branch: None,
                },
            )
            .await
            .map(json_value)
            .map_err(worktree_error);
        }
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
            let (tools, user_environment) = {
                let registry = registry.lock().await;
                (
                    crate::mcp::agent_spawning::load_agent_tools(&registry)
                        .map_err(|error| ("agent_tool_error", error))?,
                    registry.resolved_user_environment(),
                )
            };
            return Ok(json_value(
                crate::runtime_doctor::check_agent_tools_with_user_environment(
                    tools,
                    &user_environment,
                )
                .await,
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
        "agent_tools.set_icon" => {
            let params: AgentToolIconParams = params_as(params)?;
            let tool = {
                let registry = registry.lock().await;
                crate::mcp::agent_spawning::load_agent_tool(&registry, params.agent_tool_id)
                    .map_err(|error| ("agent_tool_error", error))?
            };
            return agent_icons::set_override(tool, data_dir, Path::new(&params.source_path))
                .map(json_value)
                .map_err(|error| ("invalid_agent_tool_icon", error.to_string()));
        }
        "agent_tools.remove_icon" => {
            let params: AgentToolIdParams = params_as(params)?;
            let tool = {
                let registry = registry.lock().await;
                crate::mcp::agent_spawning::load_agent_tool(&registry, params.agent_tool_id)
                    .map_err(|error| ("agent_tool_error", error))?
            };
            return agent_icons::remove_override(tool, data_dir)
                .map(json_value)
                .map_err(|error| ("agent_tool_icon_error", error.to_string()));
        }
        "settings.terminal_theme_import" => {
            return Ok(json_value(terminal_theme::import_terminal_theme()));
        }
        _ => {}
    }

    let mut registry = registry.lock().await;
    match method {
        "timer.list" | "timers.list" => {
            let params: TimerProjectParams = params_as(params)?;
            let timers = TimerService::new(&mut registry)
                .list_project(params.project_id, crate::timers::now_millis())
                .map_err(timer_error)?;
            return Ok(json!({ "project_id": params.project_id, "timers": timers }));
        }
        "timer.update" | "timers.update" => {
            let params: TimerUpdateParams = params_as(params)?;
            let now = crate::timers::now_millis();
            let lifecycle_kind = match params.paused {
                Some(true) => TimerLifecycleKind::Paused,
                Some(false) => TimerLifecycleKind::Resumed,
                None => TimerLifecycleKind::Updated,
            };
            let timer = TimerService::new(&mut registry)
                .edit_project_timer(
                    params.project_id,
                    params.timer_id,
                    TimerEdit {
                        body: params.body,
                        due_at: params.due_at,
                        delay_ms: params.delay_ms,
                        interval_ms: params.interval_ms,
                        paused: params.paused,
                    },
                    now,
                )
                .map_err(timer_error)?;
            timer_events.publish(TimerLifecycleEvent::for_timer(
                lifecycle_kind,
                params.project_id,
                timer.clone(),
                now,
                None,
            ));
            return Ok(json!({ "project_id": params.project_id, "timer": timer }));
        }
        "timer.delete" | "timers.delete" => {
            let params: TimerTargetParams = params_as(params)?;
            let now = crate::timers::now_millis();
            let timer = TimerService::new(&mut registry)
                .delete_project_timer(params.project_id, params.timer_id)
                .map_err(timer_error)?;
            timer_events.publish(TimerLifecycleEvent::for_timer(
                TimerLifecycleKind::Cancelled,
                params.project_id,
                timer,
                now,
                None,
            ));
            return Ok(json!({
                "project_id": params.project_id,
                "timer_id": params.timer_id,
                "deleted": true,
            }));
        }
        _ => {}
    }
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
        "profile.list" => {
            return crate::profiles::list(&registry);
        }
        "profile.create" => {
            let params: ProfileCreateParams = params_as(params)?;
            return crate::profiles::create(&registry, data_dir, &params.name, params.copy_current);
        }
        "profile.rename" => {
            let params: ProfileRenameParams = params_as(params)?;
            return crate::profiles::rename(&registry, params.profile_id, &params.name);
        }
        "profile.switch_impact" => {
            let params: ProfileParams = params_as(params)?;
            return crate::profiles::switch_impact(&mut registry, params.profile_id);
        }
        "profile.switch" => {
            let params: ProfileSwitchParams = params_as(params)?;
            return crate::profiles::switch(
                &mut registry,
                params.profile_id,
                params.confirm_stop_running,
            );
        }
        "profile.delete" => {
            let params: ProfileDeleteParams = params_as(params)?;
            return crate::profiles::delete(
                &registry,
                data_dir,
                params.profile_id,
                params.confirm_delete,
            );
        }
        "profile.export" => {
            let params: ProfileExportParams = params_as(params)?;
            return crate::profiles::export(
                &registry,
                data_dir,
                params.profile_id,
                Path::new(&params.path),
            );
        }
        "profile.import" => {
            let params: ProfileImportParams = params_as(params)?;
            return crate::profiles::import(
                &registry,
                data_dir,
                Path::new(&params.path),
                params.name.as_deref(),
            );
        }
        "settings.user_shell" => {
            let params: UserShellParams = params_as(params)?;
            crate::user_config::save_user_shell_from_settings_at(
                registry.user_environment_resolver().config_path(),
                params.shell.as_deref(),
            )
            .map_err(|error| ("user_config_error", error.to_string()))?;
            registry
                .store()
                .set_active_profile_terminal_shell(params.shell.as_deref())
                .map_err(project_store_error)?;
            return Ok(json_value(
                registry.resolved_user_environment().info().clone(),
            ));
        }
        "notifications.list" | "notifications_list" => {
            let params: NotificationsListParams = params_as(params)?;
            return registry
                .store()
                .list_notifications(params.read, params.limit.unwrap_or(100))
                .map(json_value)
                .map_err(project_store_error);
        }
        "notifications.mark_read" | "notifications_mark_read" => {
            let params: NotificationsMarkReadParams = params_as(params)?;
            let read_at = crate::timers::now_millis();
            if params.all {
                return registry
                    .store()
                    .mark_all_notifications_read(read_at)
                    .map(|updated| json!({ "updated": updated }))
                    .map_err(project_store_error);
            }
            let notification_id = params.notification_id.ok_or((
                "invalid_params",
                "notification_id is required unless all is true".to_owned(),
            ))?;
            return registry
                .store()
                .mark_notification_read(notification_id, read_at)
                .map(|updated| json!({ "updated": usize::from(updated) }))
                .map_err(project_store_error);
        }
        "projects.list" => {
            return project_result(list_projects(registry.store()));
        }
        "project.rail" => {
            return project_rail_result(registry.store());
        }
        "projects.register" => {
            let params: RegisterProjectParams = params_as(params)?;
            register_project(registry.store(), &params.path)?;
            let _ = crate::worktrees::reconcile_existing_projects(registry.store());
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
        "projects.update_settings" => {
            let params: UpdateProjectSettingsParams = params_as(params)?;
            update_project_settings(registry.store(), params)?;
            return project_result(list_projects(registry.store()));
        }
        "projects.set_custom_icon" => {
            let params: CustomProjectIconParams = params_as(params)?;
            set_custom_project_icon(registry.store(), params)?;
            return project_result(list_projects(registry.store()));
        }
        "projects.refresh_icon" => {
            let params: ProjectParams = params_as(params)?;
            let project = registry
                .store()
                .get_project(params.project_id)
                .map_err(project_store_error)?
                .ok_or(("project_not_found", "project not found".to_owned()))?;
            return Ok(json_value(project_icons::refresh_auto(&project)));
        }
        "project.reorder" => {
            let params: ProjectReorderParams = params_as(params)?;
            registry
                .store_mut()
                .reorder_projects(&params.ordered_ids)
                .map_err(reorder_store_error)?;
            return project_result(list_projects(registry.store()));
        }
        "project.layout" => {
            let params: ProjectLayoutParams = params_as(params)?;
            registry
                .store()
                .update_project_layout(&params.entries)
                .map_err(reorder_store_error)?;
            return project_rail_result(registry.store());
        }
        "project_folders.create" => {
            let params: ProjectFolderNameParams = params_as(params)?;
            registry
                .store()
                .create_project_folder(&params.name)
                .map_err(project_folder_store_error)?;
            return project_rail_result(registry.store());
        }
        "project_folders.rename" => {
            let params: ProjectFolderRenameParams = params_as(params)?;
            let renamed = registry
                .store()
                .rename_project_folder(params.folder_id, &params.name)
                .map_err(project_folder_store_error)?;
            if renamed.is_none() {
                return Err((
                    "project_folder_not_found",
                    "project folder not found".to_owned(),
                ));
            }
            return project_rail_result(registry.store());
        }
        "project_folders.set_collapsed" => {
            let params: ProjectFolderCollapseParams = params_as(params)?;
            if !registry
                .store()
                .set_project_folder_collapsed(params.folder_id, params.collapsed)
                .map_err(project_folder_store_error)?
            {
                return Err((
                    "project_folder_not_found",
                    "project folder not found".to_owned(),
                ));
            }
            return project_rail_result(registry.store());
        }
        "project_folders.delete" => {
            let params: ProjectFolderDeleteParams = params_as(params)?;
            if !params.confirm_delete {
                return Err((
                    "project_folder_delete_requires_confirmation",
                    "confirm_delete=true is required; projects will be lifted to the top level"
                        .to_owned(),
                ));
            }
            if !registry
                .store()
                .delete_project_folder(params.folder_id)
                .map_err(project_folder_store_error)?
            {
                return Err((
                    "project_folder_not_found",
                    "project folder not found".to_owned(),
                ));
            }
            return project_rail_result(registry.store());
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
            let canonical = root.join(crate::WORKMAN_CONFIG_FILE);
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
            return crate::config::write_workman_yml_command(
                &mut registry,
                params.project_id,
                crate::config::CommandDefinition {
                    name: params.name,
                    command: params.command,
                    working_dir: params.working_dir,
                    env: params.env,
                    auto_start: params.auto_start,
                    auto_restart: params.auto_restart,
                    restart_when_changed: params.restart_when_changed,
                },
            )
            .map(json_value)
            .map_err(config_error);
        }
        "config.command_update" => {
            let params: UpdateCommandParams = params_as(params)?;
            return crate::config::update_command(
                &mut registry,
                params.process_id,
                crate::config::CommandDefinition {
                    name: params.name,
                    command: params.command,
                    working_dir: params.working_dir,
                    env: params.env,
                    auto_start: params.auto_start,
                    auto_restart: params.auto_restart,
                    restart_when_changed: params.restart_when_changed,
                },
            )
            .map(json_value)
            .map_err(config_error);
        }
        "config.command_delete" => {
            let params: ProcessIdParams = params_as(params)?;
            return crate::config::delete_command(&mut registry, params.process_id)
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
                .map(|tools| json_value(agent_icons::views(tools, data_dir)))
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
            .map(|tool| json_value(agent_icons::view(tool, data_dir)))
            .map_err(|error| ("agent_tool_error", error));
        }
        "agent_tools.delete" => {
            let params: AgentToolIdParams = params_as(params)?;
            let deleted = crate::mcp::agent_spawning::delete_agent_tool_from_settings(
                &registry,
                params.agent_tool_id,
            )
            .map_err(|error| ("agent_tool_error", error))?;
            if deleted {
                agent_icons::delete_override(data_dir, params.agent_tool_id)
                    .map_err(|error| ("agent_tool_icon_error", error.to_string()))?;
            }
            return Ok(json!({ "agent_tool_id": params.agent_tool_id, "deleted": deleted }));
        }
        "agent_tools.reorder" => {
            let params: AgentToolOrderParams = params_as(params)?;
            return crate::mcp::agent_spawning::reorder_agent_tools_from_settings(
                &registry,
                &params.agent_tool_ids,
            )
            .map(|tools| json_value(agent_icons::views(tools, data_dir)))
            .map_err(|error| ("agent_tool_error", error));
        }
        "quick_prompts.list" => {
            return registry
                .store()
                .list_quick_prompts()
                .map(json_value)
                .map_err(quick_prompt_store_error);
        }
        "quick_prompts.save" => {
            let params: QuickPromptParams = params_as(params)?;
            let name = params.prompt.name.trim();
            if name.is_empty() {
                return Err((
                    "invalid_params",
                    "quick prompt name cannot be empty".to_owned(),
                ));
            }
            if name.chars().count() > 120 {
                return Err((
                    "invalid_params",
                    "quick prompt name must be 120 characters or fewer".to_owned(),
                ));
            }
            if name.contains('\0') {
                return Err((
                    "invalid_params",
                    "quick prompt name may not contain NUL bytes".to_owned(),
                ));
            }
            if params.prompt.body.len() > 64 * 1024 {
                return Err((
                    "invalid_params",
                    "quick prompt body must be 65536 bytes or fewer".to_owned(),
                ));
            }
            if params.prompt.body.contains('\0') {
                return Err((
                    "invalid_params",
                    "quick prompt body may not contain NUL bytes".to_owned(),
                ));
            }
            let id = match params.prompt.id {
                Some(id) => {
                    if registry
                        .store()
                        .get_quick_prompt(id)
                        .map_err(quick_prompt_store_error)?
                        .is_none()
                    {
                        return Err((
                            "quick_prompt_not_found",
                            format!("quick prompt {id} does not belong to the active profile"),
                        ));
                    }
                    id
                }
                None => registry
                    .store()
                    .next_quick_prompt_id()
                    .map_err(quick_prompt_store_error)?,
            };
            registry
                .store()
                .put_quick_prompt(&QuickPrompt {
                    id,
                    name: name.to_owned(),
                    body: params.prompt.body,
                    sort_order: 0,
                    created_at: 0,
                    updated_at: 0,
                })
                .map_err(|error| quick_prompt_save_error(error, name))?;
            return registry
                .store()
                .get_quick_prompt(id)
                .map_err(quick_prompt_store_error)?
                .map(json_value)
                .ok_or((
                    "quick_prompt_not_found",
                    format!("quick prompt {id} does not belong to the active profile"),
                ));
        }
        "quick_prompts.delete" => {
            let params: QuickPromptIdParams = params_as(params)?;
            let deleted = registry
                .store()
                .delete_quick_prompt(params.quick_prompt_id)
                .map_err(quick_prompt_store_error)?;
            return Ok(json!({
                "quick_prompt_id": params.quick_prompt_id,
                "deleted": deleted,
            }));
        }
        "quick_prompts.reorder" => {
            let params: QuickPromptOrderParams = params_as(params)?;
            registry
                .store()
                .reorder_quick_prompts(&params.quick_prompt_ids)
                .map_err(quick_prompt_store_error)?;
            return registry
                .store()
                .list_quick_prompts()
                .map(json_value)
                .map_err(quick_prompt_store_error);
        }
        "agent_templates.list" => {
            return crate::mcp::agent_spawning::load_agent_templates(&registry)
                .map(json_value)
                .map_err(|error| ("agent_template_error", error));
        }
        "agent_templates.save" => {
            let params: AgentTemplateParams = params_as(params)?;
            return crate::mcp::agent_spawning::save_agent_template_from_settings(
                &registry,
                params.template.id,
                params.template.name,
                params.template.agent_tool_id,
                params.template.extra_args,
                params.template.prompt,
            )
            .map(json_value)
            .map_err(|error| ("agent_template_error", error));
        }
        "agent_templates.delete" => {
            let params: AgentTemplateIdParams = params_as(params)?;
            let deleted = crate::mcp::agent_spawning::delete_agent_template_from_settings(
                &registry,
                params.agent_template_id,
            )
            .map_err(|error| ("agent_template_error", error))?;
            return Ok(json!({
                "agent_template_id": params.agent_template_id,
                "deleted": deleted
            }));
        }
        "agent_templates.reorder" => {
            let params: AgentTemplateOrderParams = params_as(params)?;
            return crate::mcp::agent_spawning::reorder_agent_templates_from_settings(
                &registry,
                &params.agent_template_ids,
            )
            .map(json_value)
            .map_err(|error| ("agent_template_error", error));
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
        "process.mark_read" => {
            let params: ProcessIdParams = params_as(params)?;
            registry.mark_agent_read(params.process_id).map(json_value)
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
    let shell = registry
        .resolved_user_environment()
        .active_shell()
        .to_string_lossy()
        .into_owned();
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

fn quick_prompt_store_error(error: workman_core::StoreError) -> (&'static str, String) {
    ("quick_prompt_error", error.to_string())
}

fn quick_prompt_save_error(error: workman_core::StoreError, name: &str) -> (&'static str, String) {
    if let workman_core::StoreError::Sqlite(rusqlite::Error::SqliteFailure(code, Some(detail))) =
        &error
        && code.code == rusqlite::ErrorCode::ConstraintViolation
        && detail.contains("quick_prompts.profile_id, quick_prompts.name")
    {
        return (
            "quick_prompt_error",
            format!("A quick prompt named {name} already exists in this profile"),
        );
    }
    quick_prompt_store_error(error)
}

fn config_error(error: crate::ConfigError) -> (&'static str, String) {
    let code = match &error {
        crate::ConfigError::ProjectNotFound(_) => "project_not_found",
        crate::ConfigError::InvalidProcessName | crate::ConfigError::MissingCommand(_) => {
            "invalid_params"
        }
        crate::ConfigError::LocalNameConflict(_) | crate::ConfigError::ProcessNameConflict(_) => {
            "process_name_conflict"
        }
        crate::ConfigError::NotCommand(_) => "invalid_params",
        crate::ConfigError::Registry(error) => error.code(),
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

fn timer_error(error: TimerError) -> (&'static str, String) {
    (error.code(), error.to_string())
}

fn project_result(
    result: Result<Vec<ProjectSummary>, (&'static str, String)>,
) -> Result<Value, (&'static str, String)> {
    result.map(json_value)
}

fn project_rail_result(store: &Store) -> Result<Value, (&'static str, String)> {
    let projects = list_projects(store)?;
    let folders = store
        .list_project_folders()
        .map_err(project_folder_store_error)?;
    let layout = store.project_layout().map_err(project_folder_store_error)?;
    Ok(json_value(ProjectRailSnapshot {
        projects,
        folders,
        layout,
    }))
}

fn register_project(store: &Store, path: &str) -> Result<(), (&'static str, String)> {
    let canonical = workman_core::canonical_path(path).map_err(|error| {
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
    if let Some(project) = store
        .get_project_by_path_any(&canonical)
        .map_err(project_store_error)?
    {
        store
            .attach_project_to_active_profile(project.id)
            .map_err(project_store_error)?;
    } else {
        let selected = store
            .list_projects()
            .map_err(project_store_error)?
            .is_empty();
        store
            .put_project(&Project {
                id: store.next_project_id().map_err(project_store_error)?,
                path: canonical,
                name,
                display_name: None,
                icon: None,
                selected,
                sort_order: store
                    .next_project_sort_order()
                    .map_err(project_store_error)?,
            })
            .map_err(project_store_error)?;
    }
    Ok(())
}

fn select_project(store: &Store, project_id: ProjectId) -> Result<(), (&'static str, String)> {
    if !store
        .select_project_in_active_profile(project_id)
        .map_err(project_store_error)?
    {
        return Err(("project_not_found", "project not found".to_owned()));
    }
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

fn update_project_settings(
    store: &Store,
    params: UpdateProjectSettingsParams,
) -> Result<(), (&'static str, String)> {
    const COLORS: &[&str] = &["amber", "blue", "rose", "slate", "teal", "violet"];

    let display_name = params.display_name.trim();
    if display_name.is_empty() {
        return Err((
            "invalid_project_name",
            "project name cannot be empty".to_owned(),
        ));
    }
    if params
        .icon
        .as_deref()
        .is_some_and(|icon| !valid_project_icon(icon))
    {
        return Err((
            "invalid_project_icon",
            "project icon must be a Lucide icon name or a managed project image".to_owned(),
        ));
    }
    if params
        .icon_color
        .as_deref()
        .is_some_and(|color| !COLORS.contains(&color))
    {
        return Err((
            "invalid_project_icon_color",
            "project icon color is not one of the supported choices".to_owned(),
        ));
    }

    let icon = params.icon.as_deref();
    let icon_color = icon
        .filter(|icon| !project_icons::is_custom_reference(icon))
        .and(params.icon_color.as_deref());
    let changed = store
        .connection()
        .execute(
            "UPDATE projects
             SET display_name = ?1, icon = ?2, icon_color = ?3
             WHERE id = ?4",
            (display_name, icon, icon_color, params.project_id),
        )
        .map_err(project_store_error)?;
    if changed == 0 {
        return Err(("project_not_found", "project not found".to_owned()));
    }
    if let Some(project) = store
        .get_project(params.project_id)
        .map_err(project_store_error)?
    {
        project_icons::invalidate(&project.path);
    }
    Ok(())
}

fn set_custom_project_icon(
    store: &Store,
    params: CustomProjectIconParams,
) -> Result<(), (&'static str, String)> {
    let project = store
        .get_project(params.project_id)
        .map_err(project_store_error)?
        .ok_or(("project_not_found", "project not found".to_owned()))?;
    let reference = project_icons::copy_custom_image(&project, Path::new(&params.source_path))
        .map_err(|error| ("invalid_project_icon_image", error.to_string()))?;
    store
        .connection()
        .execute(
            "UPDATE projects SET icon = ?1, icon_color = NULL WHERE id = ?2",
            (&reference, params.project_id),
        )
        .map_err(project_store_error)?;
    project_icons::invalidate(&project.path);
    Ok(())
}

fn valid_project_icon(icon: &str) -> bool {
    project_icons::is_custom_reference(icon)
        || (icon.len() <= 80
            && !icon.is_empty()
            && !icon.starts_with('-')
            && !icon.ends_with('-')
            && icon
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'))
}

fn list_projects(store: &Store) -> Result<Vec<ProjectSummary>, (&'static str, String)> {
    let projects = store.list_projects().map_err(project_store_error)?;

    projects
        .into_iter()
        .map(|project| {
            let icon_color = store
                .connection()
                .query_row(
                    "SELECT icon_color FROM projects WHERE id = ?1",
                    [project.id],
                    |row| row.get(0),
                )
                .map_err(project_store_error)?;
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
            let envelope =
                crate::worktrees::project_envelope(store, project).map_err(worktree_error)?;
            let icon_image = project_icons::resolve(&envelope.project);
            let folder_id = store
                .project_folder_id(envelope.project.id)
                .map_err(project_folder_store_error)?;
            Ok(ProjectSummary {
                project: envelope.project,
                icon_color,
                icon_image,
                repository_id: envelope.repository_id,
                repository_root: envelope.repository_root,
                parent_project_id: envelope.parent_project_id,
                branch: envelope.branch,
                worktree_managed: envelope.worktree_managed,
                folder_id,
                status,
            })
        })
        .collect()
}

async fn control_worktree_project_id(
    registry: &SharedProcessRegistry,
    explicit: Option<ProjectId>,
) -> Result<ProjectId, (&'static str, String)> {
    let registry = registry.lock().await;
    if let Some(project_id) = explicit {
        return registry
            .store()
            .get_project(project_id)
            .map_err(project_store_error)?
            .map(|project| project.id)
            .ok_or((
                "project_not_found",
                format!("project {project_id} was not found"),
            ));
    }
    registry
        .store()
        .list_projects()
        .map_err(project_store_error)?
        .into_iter()
        .find(|project| project.selected)
        .map(|project| project.id)
        .ok_or((
            "project_scope_error",
            "no project_id was supplied and no project is selected".to_owned(),
        ))
}

fn worktree_error(error: crate::worktrees::WorktreeError) -> (&'static str, String) {
    (error.code(), error.to_string())
}

fn project_store_error(error: impl std::fmt::Display) -> (&'static str, String) {
    ("store_error", error.to_string())
}

fn reorder_store_error(error: impl std::fmt::Display) -> (&'static str, String) {
    ("invalid_reorder", error.to_string())
}

fn project_folder_store_error(error: impl std::fmt::Display) -> (&'static str, String) {
    ("project_folder_error", error.to_string())
}
