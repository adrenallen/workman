//! Core MCP service: identity, scoping, setup tools, and project tools.

use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{
    extract::{Request, State},
    http::{StatusCode, header, request::Parts},
    middleware::Next,
    response::{IntoResponse, Response},
};
use rmcp::{
    ServerHandler,
    handler::server::{router::tool::ToolRouter, tool::Extension, wrapper::Parameters},
    model::{CallToolResult, Implementation, ServerCapabilities, ServerInfo},
    schemars, tool, tool_handler, tool_router,
    transport::streamable_http_server::{
        SessionId, SessionManager, StreamableHttpServerConfig, StreamableHttpService,
        session::{local::LocalSessionManager, never::NeverSessionManager},
    },
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;
use workman_core::{Actor, Process, ProcessId, ProcessStatus, Project, ProjectId};

use crate::{
    ProcessRegistry, SharedProcessRegistry, project_titles::normalized_project_title,
    timer_events::TimerLifecycleHub,
};

pub(crate) mod agent_spawning;
mod tools_lock;
mod tools_process;
mod tools_readiness;
mod tools_scratchpad;
mod tools_timer;
mod tools_todo;
mod tools_worktree;

pub const WORKMAN_MCP_TOKEN_HEADER: &str = "x-workman-mcp-token";
pub(crate) const SCRATCHPAD_HANDOFF_GUIDANCE: &str = "Put shared notes, plans, briefs, and hand-offs in Workman scratchpads with scratchpad_write so they are visible in the app and verifiable; do not create ad-hoc repo files for them. Review unresolved feedback with scratchpad_read(include_comments=true) or scratchpad_comment_list, and use scratchpad_comment_create for anchored or whole-document discussion. Agents may update, resolve, reopen, or delete only comments they authored; the desktop user may resolve any project comment. After creating a scratchpad or todo, read it back with scratchpad_read or todo_get and reference its ID in every hand-off message.";
pub(crate) const WORKTREE_AGENT_GUIDANCE: &str = "Use worktree_list to inspect repository worktrees and cached PR status, worktree_create for a branch/ref, and worktree_fork to branch from a selected worktree's exact HEAD; each managed worktree becomes a separate Workman project.";
pub(crate) const HUMAN_HANDOFF_GUIDANCE: &str = "Found something out of scope or need human feedback? File a todo or add a comment, then assign it with todo_assign(assignee=\"user\") or mention @user in a new todo comment. A fresh user assignment and each new @user comment notify the human; unrelated edits and comment edits do not. Use todo_assign with assignee omitted/null, or assignee=\"none\", to unassign.";
pub(crate) const SPAWN_AGENT_GUIDANCE: &str = "spawn_agent launches a plain agent by default: pick agent_tool_id from list_agent_tools and omit agent_template_id. Use agent_template_id from list_agent_templates only when the user names a template or explicitly asks for one. With a template, agent_tool_id swaps the agent while retaining the template prompt and skipping its launch args. Prefer model for a per-launch override: it replaces existing long and short model flags in the registered command, template args, and caller args for supported tool_type values; reserve extra_args for other raw flags. attachments accepts up to 8 absolute paths to raster images of at most 32 MiB each; Workman copies them into daemon-owned storage and appends those saved paths to the initial prompt.";
pub(crate) const IDLE_TIMER_WAIT_GUIDANCE: &str = "To wait for subagents without polling, call timer_fire_when_idle_any or timer_fire_when_idle_all once. Choose the condition deliberately: timer_fire_when_idle_any ignores processes already idle at arm time and requires a fresh non-idle-to-idle transition, while timer_fire_when_idle_all counts already-idle processes and waits until each watched process has reached idle. When already_satisfied=false and the timer delivers back to you (the default), immediately finish your response and end the current turn; no additional wait call is needed. Do not loop on timer_list or process status while waiting. Workman keeps the timer in the daemon and submits its body to the delivery agent as a fresh user turn when the idle condition or max_wait_ms is reached. When that turn arrives, inspect the watched processes before assuming they finished because the deadline may have fired or an agent may only be waiting on its own timer. If already_satisfied=true, the body was delivered immediately; do not create another timer, and end your current turn if it was delivered to you so the queued turn can be processed.";
pub(crate) const IDLE_TIMER_LAUNCH_GUIDANCE: &str = "When a Workman idle timer delivers back to you, finish your response and end the turn after arming it instead of polling; Workman wakes you with the timer body as a fresh user turn. Use help(topic=\"timers\") for timer selection and wake verification.";

#[derive(Clone)]
pub struct WorkmanMcp {
    registry: SharedProcessRegistry,
    input_router: crate::ProcessInputRouter,
    mcp_url: String,
    timer_events: TimerLifecycleHub,
    tool_router: ToolRouter<Self>,
}

impl WorkmanMcp {
    pub(crate) fn new(
        registry: SharedProcessRegistry,
        input_router: crate::ProcessInputRouter,
        mcp_url: String,
        timer_events: TimerLifecycleHub,
    ) -> Self {
        let mut tool_router = Self::tool_router();
        tool_router.merge(Self::process_tool_router());
        tool_router.merge(Self::readiness_tool_router());
        tool_router.merge(Self::agent_spawning_tool_router());
        tool_router.merge(Self::todo_tool_router());
        tool_router.merge(Self::lock_tool_router());
        tool_router.merge(Self::scratchpad_tool_router());
        tool_router.merge(Self::timer_tool_router());
        tool_router.merge(Self::worktree_tool_router());
        Self {
            registry,
            input_router,
            mcp_url,
            timer_events,
            tool_router,
        }
    }
}

pub fn streamable_http_service(
    registry: SharedProcessRegistry,
    input_router: crate::ProcessInputRouter,
    mcp_url: String,
    timer_events: TimerLifecycleHub,
) -> (
    StreamableHttpService<WorkmanMcp, LocalSessionManager>,
    Arc<LocalSessionManager>,
) {
    let config = StreamableHttpServerConfig::default().with_json_response(true);
    let sessions = Arc::new(LocalSessionManager::default());
    let service = StreamableHttpService::new(
        move || {
            Ok(WorkmanMcp::new(
                registry.clone(),
                input_router.clone(),
                mcp_url.clone(),
                timer_events.clone(),
            ))
        },
        sessions.clone(),
        config,
    );
    (service, sessions)
}

/// Stateless JSON transport for clients that do not consume server-initiated messages.
///
/// Some MCP SDKs open an otherwise-idle SSE stream immediately after initialization and
/// permanently disable a server after a very small reconnect budget. Workman currently emits no
/// MCP progress, list-changed, or other server-initiated notifications, so Kimi loses no active
/// behavior on this request/response-only endpoint. The stateful endpoint remains the default for
/// every other client and is ready for future server push.
pub fn stateless_http_service(
    registry: SharedProcessRegistry,
    input_router: crate::ProcessInputRouter,
    mcp_url: String,
    timer_events: TimerLifecycleHub,
) -> StreamableHttpService<WorkmanMcp, NeverSessionManager> {
    let config = StreamableHttpServerConfig::default()
        .with_stateful_mode(false)
        .with_json_response(true);
    StreamableHttpService::new(
        move || {
            Ok(WorkmanMcp::new(
                registry.clone(),
                input_router.clone(),
                mcp_url.clone(),
                timer_events.clone(),
            ))
        },
        Arc::new(NeverSessionManager::default()),
        config,
    )
}

/// Reject stale stateful MCP requests before rmcp can dispatch them.
///
/// Streamable HTTP clients use 404 as the signal to discard an unknown or
/// expired session ID and perform a fresh initialize handshake.
pub async fn require_known_session(
    State(sessions): State<Arc<LocalSessionManager>>,
    request: Request,
    next: Next,
) -> Response {
    let path = request.uri().path();
    if path != "/mcp" && !path.starts_with("/mcp/") {
        return next.run(request).await;
    }
    let Some(raw_session_id) = request
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
    else {
        return next.run(request).await;
    };
    let session_id: SessionId = raw_session_id.to_owned().into();

    match sessions.has_session(&session_id).await {
        Ok(true) => next.run(request).await,
        Ok(false) => (StatusCode::NOT_FOUND, "Not Found: Session not found").into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to check MCP session: {error}"),
        )
            .into_response(),
    }
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct IdentifySessionArgs {
    /// Expected Workman process ID. The authenticated process credential already establishes it.
    process_id: ProcessId,
}

#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
struct HelpArgs {
    /// Optional topic: setup, identity, scoping, projects, todos, scratchpads, worktrees, timers, tools, or spawning.
    #[serde(default)]
    topic: Option<String>,
}

#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
struct ProjectScopeArgs {
    /// Optional project ID; an identified agent may name only its owning project.
    #[serde(default)]
    project_id: Option<ProjectId>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ProjectSelectArgs {
    project_id: ProjectId,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ProjectCreateArgs {
    /// Existing directory to register. It is canonicalized before persistence.
    path: String,
    /// Stored name. Defaults to the directory basename.
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    icon: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ProjectRenameArgs {
    #[serde(default)]
    project_id: Option<ProjectId>,
    name: String,
}

fn apply_explicit_project_name(project: &mut Project, name: &str) -> Result<(), &'static str> {
    let name = normalized_project_title(name).ok_or("project name must not be empty")?;
    project.display_name = Some(name.to_owned());
    Ok(())
}

#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
struct ProjectDeleteArgs {
    #[serde(default)]
    project_id: Option<ProjectId>,
    /// Must be true before any project is deleted.
    #[serde(default)]
    confirm_delete: bool,
    /// Also required when the project has running processes.
    #[serde(default)]
    confirm_stop_running: bool,
    /// Also permanently delete the exact local project directory. No remote Git operation is ever performed.
    #[serde(default)]
    delete_from_disk: bool,
    /// Permit guarded loss of dirty/unpublished state or dependent linked worktrees.
    #[serde(default)]
    force_dirty: bool,
}

#[derive(Debug, Serialize)]
struct IdentityResult {
    actor_id: String,
    session_id: String,
    process_id: Option<ProcessId>,
    process_name: Option<String>,
    effective_project_id: Option<ProjectId>,
    selected_project_id: Option<ProjectId>,
}

#[tool_router]
impl WorkmanMcp {
    #[tool(
        description = "Report this MCP session's actor, process, and effective project identity"
    )]
    async fn whoami(&self, Extension(parts): Extension<Parts>) -> CallToolResult {
        let mut registry = self.registry.lock().await;
        match ensure_actor(&mut registry, &parts) {
            Ok((actor, process)) => {
                let effective_project_id = resolve_project_id(&registry, &actor, None).ok();
                success(IdentityResult {
                    actor_id: actor.id,
                    session_id: actor.session_id,
                    process_id: actor.process_id,
                    process_name: process.map(|process| process.name),
                    effective_project_id,
                    selected_project_id: actor.selected_project_id,
                })
            }
            Err(error) => failure("identity_error", error),
        }
    }

    #[tool(
        description = "Confirm this authenticated MCP connection is bound to the expected process; this cannot claim or change identity"
    )]
    async fn identify_session(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<IdentifySessionArgs>,
    ) -> CallToolResult {
        let mut registry = self.registry.lock().await;
        let (actor, existing_process) = match ensure_actor(&mut registry, &parts) {
            Ok(identity) => identity,
            Err(error) => return failure("identity_error", error),
        };
        if let Some(process) = existing_process {
            if args.process_id != process.id {
                let target_detail = match registry.store().get_process(args.process_id) {
                    Ok(Some(target)) => format!(
                        "target process {} belongs to project {}",
                        target.id, target.project_id
                    ),
                    Ok(None) => format!("target process {} was not found", args.process_id),
                    Err(error) => return failure("store_error", error.to_string()),
                };
                return failure(
                    "identity_scope_error",
                    format!(
                        "agent identities are scoped to project {} and bound to process {}; {target_detail}; this MCP identity cannot be retargeted",
                        process.project_id, process.id
                    ),
                );
            }
            let effective_project_id = resolve_project_id(&registry, &actor, None).ok();
            return success(IdentityResult {
                actor_id: actor.id,
                session_id: actor.session_id,
                process_id: actor.process_id,
                process_name: Some(process.name),
                effective_project_id,
                selected_project_id: actor.selected_project_id,
            });
        }
        failure(
            "identity_authentication_required",
            format!(
                "this MCP connection has no authenticated process identity and cannot claim process {}; reconnect using that process's WORKMAN_MCP_TOKEN credential, then call whoami",
                args.process_id
            ),
        )
    }

    #[tool(description = "Show concise Workman MCP help, optionally for one topic")]
    async fn help(&self, Parameters(args): Parameters<HelpArgs>) -> CallToolResult {
        let topic = args.topic.as_deref().unwrap_or("setup");
        let text = match topic {
            "setup" => {
                "Connect to /mcp with Streamable HTTP. Daemon-spawned agents authenticate with their process credential and are automatically jailed to their owning project. A daemon bearer authenticates user-level discovery only and cannot claim a process identity."
            }
            "identity" => {
                "whoami resolves the process credential supplied by the launcher. identify_session only confirms that authenticated identity; it cannot claim or retarget a process. If whoami is unidentified or names the wrong process, stop and report a launch-wiring error."
            }
            "scoping" => {
                "Agent identities are jailed by the daemon to their owning project: list_projects returns only that project, cross-project project_id overrides and indirect process/timer/transfer targets are rejected, select_project cannot escape the jail, and project-creating/global-config tools are unavailable. Unidentified bearer sessions may use discovery/help but cannot claim a process or perform project-scoped actions. The authenticated UI/CLI control channel remains user-scoped and can manage every project."
            }
            "projects" => {
                "list_projects/select_project/get_project/get_project_status/get_project_stats/create_project/rename_project/delete_project manage registered workspaces. Agent identities see and target only their owning project and cannot register a new project. Delete always requires confirm_delete; active processes require confirm_stop_running; delete_from_disk performs guarded local-only deletion and never changes a remote."
            }
            "todos" => HUMAN_HANDOFF_GUIDANCE,
            "scratchpads" => SCRATCHPAD_HANDOFF_GUIDANCE,
            "worktrees" => WORKTREE_AGENT_GUIDANCE,
            "timers" => IDLE_TIMER_WAIT_GUIDANCE,
            "tools" => "Use mcp_tools_summary for the complete core tool list.",
            "spawning" => SPAWN_AGENT_GUIDANCE,
            other => {
                return failure(
                    "unknown_help_topic",
                    format!("unknown help topic {other:?}"),
                );
            }
        };
        success(json!({ "topic": topic, "text": text }))
    }

    #[tool(description = "List the core MCP tools exposed by this daemon")]
    async fn mcp_tools_summary(&self) -> CallToolResult {
        let tools = self
            .tool_router
            .list_all()
            .into_iter()
            .map(|tool| tool.name.into_owned())
            .collect::<Vec<_>>();
        success(json!({
            "enabled": true,
            "count": tools.len(),
            "tools": tools,
            "spawn_agent_guidance": SPAWN_AGENT_GUIDANCE,
            "idle_timer_wait_guidance": IDLE_TIMER_WAIT_GUIDANCE,
        }))
    }

    #[tool(description = "Run a disposable SQLite write-read-cleanup self-test")]
    async fn mcp_smoke_test(&self) -> CallToolResult {
        let mut registry = self.registry.lock().await;
        match registry.store_mut().smoke_test() {
            Ok(true) => success(json!({ "ok": true, "checks": ["sqlite_write_read_cleanup"] })),
            Ok(false) => failure("smoke_test_failed", "SQLite readback did not match"),
            Err(error) => failure("smoke_test_failed", error.to_string()),
        }
    }

    #[tool(
        description = "List registered projects visible to this identity (agents see only their owning project)"
    )]
    async fn list_projects(&self, Extension(parts): Extension<Parts>) -> CallToolResult {
        let mut registry = self.registry.lock().await;
        let (_actor, process) = match ensure_actor(&mut registry, &parts) {
            Ok(identity) => identity,
            Err(error) => return failure("identity_error", error),
        };
        let projects = match process {
            Some(process) => registry
                .store()
                .get_project(process.project_id)
                .map(|project| project.into_iter().collect()),
            None => registry.store().list_projects(),
        };
        match projects {
            Ok(projects) => match crate::worktrees::project_envelopes(registry.store(), projects) {
                Ok(projects) => success(json!({ "projects": projects })),
                Err(error) => failure(error.code(), error.to_string()),
            },
            Err(error) => failure("store_error", error.to_string()),
        }
    }

    #[tool(
        description = "Select the effective project (agent identities cannot select outside their owning project)"
    )]
    async fn select_project(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<ProjectSelectArgs>,
    ) -> CallToolResult {
        let mut registry = self.registry.lock().await;
        let (mut actor, _) = match ensure_actor(&mut registry, &parts) {
            Ok(identity) => identity,
            Err(error) => return failure("identity_error", error),
        };
        if let Err(error) = enforce_project_access(&registry, &actor, args.project_id) {
            return failure("project_scope_error", error);
        }
        let project = match registry.store().get_project(args.project_id) {
            Ok(Some(project)) => project,
            Ok(None) => {
                return failure(
                    "project_not_found",
                    format!("project {} was not found", args.project_id),
                );
            }
            Err(error) => return failure("store_error", error.to_string()),
        };
        actor.selected_project_id = Some(project.id);
        actor.last_seen_at = now_millis();
        if let Err(error) = registry.store().put_actor(&actor) {
            return failure("store_error", error.to_string());
        }
        match crate::worktrees::project_envelope(registry.store(), project) {
            Ok(project) => success(json!({ "project": project, "actor_id": actor.id })),
            Err(error) => failure(error.code(), error.to_string()),
        }
    }

    #[tool(description = "Get the effective or explicitly requested project")]
    async fn get_project(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<ProjectScopeArgs>,
    ) -> CallToolResult {
        let mut registry = self.registry.lock().await;
        match scoped_project(&mut registry, &parts, args.project_id) {
            Ok((project, _)) => match crate::worktrees::project_envelope(registry.store(), project)
            {
                Ok(project) => success(project),
                Err(error) => failure(error.code(), error.to_string()),
            },
            Err(error) => failure("project_scope_error", error),
        }
    }

    #[tool(description = "Get project metadata plus persisted process status")]
    async fn get_project_status(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<ProjectScopeArgs>,
    ) -> CallToolResult {
        let mut registry = self.registry.lock().await;
        let (project, _) = match scoped_project(&mut registry, &parts, args.project_id) {
            Ok(scoped) => scoped,
            Err(error) => return failure("project_scope_error", error),
        };
        let project = match crate::worktrees::project_envelope(registry.store(), project) {
            Ok(project) => project,
            Err(error) => return failure(error.code(), error.to_string()),
        };
        match registry.list_statuses(Some(project.project.id)) {
            Ok(processes) => success(json!({ "project": project, "processes": processes })),
            Err(error) => failure(error.code(), error.to_string()),
        }
    }

    #[tool(description = "Get lightweight process counts for a project")]
    async fn get_project_stats(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<ProjectScopeArgs>,
    ) -> CallToolResult {
        let mut registry = self.registry.lock().await;
        let (project, _) = match scoped_project(&mut registry, &parts, args.project_id) {
            Ok(scoped) => scoped,
            Err(error) => return failure("project_scope_error", error),
        };
        let processes = match registry.list(Some(project.id)) {
            Ok(processes) => processes,
            Err(error) => return failure(error.code(), error.to_string()),
        };
        let mut by_status = BTreeMap::<String, usize>::new();
        for process in &processes {
            *by_status.entry(process.status.as_str().into()).or_default() += 1;
        }
        success(json!({
            "project_id": project.id,
            "process_count": processes.len(),
            "running_count": processes.iter().filter(|process| process.status == ProcessStatus::Running).count(),
            "by_status": by_status,
        }))
    }

    #[tool(
        description = "Register an existing directory as a project (user control only; agent identities cannot create project scope)"
    )]
    async fn create_project(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(_args): Parameters<ProjectCreateArgs>,
    ) -> CallToolResult {
        let mut registry = self.registry.lock().await;
        let (actor, _) = match ensure_actor(&mut registry, &parts) {
            Ok(identity) => identity,
            Err(error) => return failure("identity_error", error),
        };
        match process_project_id(&registry, &actor) {
            Ok(Some(project_id)) => failure(
                "project_scope_error",
                format!(
                    "agent identities are scoped to project {project_id}; creating another project is outside that scope"
                ),
            ),
            Ok(None) => failure(
                "identity_required",
                "MCP session has no authenticated process identity; use the authenticated UI/CLI control channel for project registration",
            ),
            Err(error) => failure("project_scope_error", error),
        }
    }

    #[tool(description = "Rename the effective or explicitly requested project")]
    async fn rename_project(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<ProjectRenameArgs>,
    ) -> CallToolResult {
        if args.name.trim().is_empty() {
            return failure("invalid_project_name", "project name must not be empty");
        }
        let mut registry = self.registry.lock().await;
        let (mut project, _) = match scoped_project(&mut registry, &parts, args.project_id) {
            Ok(scoped) => scoped,
            Err(error) => return failure("project_scope_error", error),
        };
        if let Err(error) = apply_explicit_project_name(&mut project, &args.name) {
            return failure("invalid_project_name", error);
        }
        match registry.store().put_project(&project) {
            Ok(()) => match crate::worktrees::project_envelope(registry.store(), project) {
                Ok(project) => success(project),
                Err(error) => failure(error.code(), error.to_string()),
            },
            Err(error) => failure("project_rename_failed", error.to_string()),
        }
    }

    #[tool(
        description = "Remove a project from Workman after explicit confirmation; set delete_from_disk=true to permanently delete its exact local folder with guarded force confirmation. This never pushes, fetches, or deletes a remote branch"
    )]
    async fn delete_project(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<ProjectDeleteArgs>,
    ) -> CallToolResult {
        let project_id = {
            let mut registry = self.registry.lock().await;
            match scoped_project(&mut registry, &parts, args.project_id) {
                Ok((project, _)) => project.id,
                Err(error) => return failure("project_scope_error", error),
            }
        };
        match crate::worktrees::remove(
            &self.registry,
            crate::worktrees::RemoveWorktree {
                project_id,
                confirm_remove: args.confirm_delete,
                confirm_stop_running: args.confirm_stop_running,
                delete_from_disk: args.delete_from_disk,
                force_dirty: args.force_dirty,
                confirm_branch: None,
            },
        )
        .await
        {
            Ok(removed) => success(removed),
            Err(error) => failure(error.code(), error.to_string()),
        }
    }
}

#[cfg(test)]
mod project_name_tests {
    use super::*;

    #[test]
    fn mcp_rename_sets_display_name_without_rewriting_canonical_name() {
        let mut project = Project {
            id: 7,
            path: "/tmp/repo-feature".into(),
            name: "repo: feature".into(),
            display_name: None,
            icon: None,
            selected: true,
            sort_order: 0,
        };

        apply_explicit_project_name(&mut project, "  Checkout polish  ").unwrap();

        assert_eq!(project.name, "repo: feature");
        assert_eq!(project.display_name.as_deref(), Some("Checkout polish"));
        assert!(apply_explicit_project_name(&mut project, "  ").is_err());
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for WorkmanMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("workman", env!("CARGO_PKG_VERSION")))
            .with_instructions(format!(
                "Workman workspace control. Call whoami first. Agent identities are authenticated by their launch credential and daemon-jailed to their owning project; cross-project IDs and indirect targets are rejected. Unidentified bearer sessions cannot claim a process identity or perform project-scoped work. {IDLE_TIMER_WAIT_GUIDANCE} {HUMAN_HANDOFF_GUIDANCE}"
            ))
    }
}

fn ensure_actor(
    registry: &mut ProcessRegistry,
    parts: &Parts,
) -> Result<(Actor, Option<Process>), String> {
    let explicit_process_token = parts
        .headers
        .get(WORKMAN_MCP_TOKEN_HEADER)
        .and_then(|value| value.to_str().ok());
    let bearer_candidate = parts
        .headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    let token_process = match explicit_process_token {
        Some(token) => registry
            .store()
            .get_process_by_mcp_token(token)
            .map_err(|error| error.to_string())?,
        None => match bearer_candidate {
            Some(token) => registry
                .store()
                .get_process_by_mcp_token(token)
                .map_err(|error| error.to_string())?,
            None => None,
        },
    };
    if explicit_process_token.is_some() && token_process.is_none() {
        return Err("process token is no longer active".to_owned());
    }
    let client_session_id = parts
        .headers
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok());
    if token_process.is_none() && client_session_id.is_none() {
        return Err(
            "sessionless MCP tool calls require an active process credential; reconnect using this launch's WORKMAN_MCP_TOKEN credential"
                .to_owned(),
        );
    }
    // A process credential is the identity authority. Never let a caller-selected session
    // header address or rewrite another process's durable actor row.
    let session_id = token_process.as_ref().map_or_else(
        || {
            client_session_id
                .map(str::to_owned)
                .unwrap_or_else(|| format!("anonymous:{}", Uuid::new_v4().simple()))
        },
        |process| format!("process:{}", process.id),
    );
    let now = now_millis();
    let mut actor = registry
        .store()
        .get_actor_by_session_id(&session_id)
        .map_err(|error| error.to_string())?
        .unwrap_or_else(|| Actor {
            id: format!("mcp-{}", Uuid::new_v4().simple()),
            session_id: session_id.clone(),
            process_id: None,
            selected_project_id: None,
            created_at: now,
            last_seen_at: now,
        });
    if let Some(process) = &token_process {
        actor.process_id = Some(process.id);
        actor.selected_project_id = Some(process.project_id);
    } else {
        // A durable actor record must never turn a daemon-bearer connection into a
        // process identity. Only a current per-process credential can establish it.
        actor.process_id = None;
        actor.selected_project_id = None;
    }
    actor.last_seen_at = now;
    registry
        .store()
        .put_actor(&actor)
        .map_err(|error| error.to_string())?;

    let process = match actor.process_id {
        Some(process_id) => registry
            .store()
            .get_process(process_id)
            .map_err(|error| error.to_string())?,
        None => None,
    };
    Ok((actor, process))
}

fn scoped_project(
    registry: &mut ProcessRegistry,
    parts: &Parts,
    explicit_project_id: Option<ProjectId>,
) -> Result<(Project, Actor), String> {
    let (actor, _) = ensure_actor(registry, parts)?;
    let project_id = resolve_project_id(registry, &actor, explicit_project_id)?;
    let project = registry
        .store()
        .get_project(project_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("project {project_id} was not found"))?;
    Ok((project, actor))
}

fn resolve_project_id(
    registry: &ProcessRegistry,
    actor: &Actor,
    explicit_project_id: Option<ProjectId>,
) -> Result<ProjectId, String> {
    if let Some(owning_project_id) = process_project_id(registry, actor)? {
        if let Some(project_id) = explicit_project_id
            && project_id != owning_project_id
        {
            return Err(agent_project_scope_error(owning_project_id, project_id));
        }
        return Ok(owning_project_id);
    }
    Err(
        "MCP session has no authenticated process identity; reconnect with this process's WORKMAN_MCP_TOKEN credential before project-scoped actions"
            .to_owned(),
    )
}

fn process_project_id(
    registry: &ProcessRegistry,
    actor: &Actor,
) -> Result<Option<ProjectId>, String> {
    let Some(process_id) = actor.process_id else {
        return Ok(None);
    };
    let project_id = registry
        .store()
        .get_process(process_id)
        .map_err(|error| error.to_string())?
        .map(|process| process.project_id)
        .ok_or_else(|| format!("identified process {process_id} was not found"))?;
    if !registry
        .store()
        .is_project_in_active_profile(project_id)
        .map_err(|error| error.to_string())?
    {
        return Err(format!(
            "identified process {process_id} belongs to project {project_id}, which is not in the active profile; reconnect or switch back before project-scoped work"
        ));
    }
    Ok(Some(project_id))
}

fn enforce_project_access(
    registry: &ProcessRegistry,
    actor: &Actor,
    requested_project_id: ProjectId,
) -> Result<(), String> {
    match process_project_id(registry, actor)? {
        Some(owning_project_id) if owning_project_id != requested_project_id => Err(
            agent_project_scope_error(owning_project_id, requested_project_id),
        ),
        Some(_) => Ok(()),
        None => Err(
            "MCP session has no authenticated process identity; reconnect with this process's WORKMAN_MCP_TOKEN credential before project-scoped actions"
                .to_owned(),
        ),
    }
}

fn agent_project_scope_error(
    owning_project_id: ProjectId,
    requested_project_id: ProjectId,
) -> String {
    format!(
        "agent identities are scoped to project {owning_project_id}; project {requested_project_id} is outside that scope"
    )
}

fn success(value: impl Serialize) -> CallToolResult {
    match serde_json::to_value(value) {
        // Some MCP clients reject a top-level array in `structuredContent`.
        // List tools use semantic envelopes; this fallback keeps newly added
        // tools from accidentally reintroducing an incompatible root array.
        Ok(Value::Array(items)) => CallToolResult::structured(json!({ "items": items })),
        Ok(value) => CallToolResult::structured(value),
        Err(error) => failure("serialization_error", error.to_string()),
    }
}

fn failure(code: &'static str, message: impl Into<String>) -> CallToolResult {
    CallToolResult::structured_error(json!({ "code": code, "message": message.into() }))
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn success_never_emits_a_root_array_and_keeps_text_in_sync() {
        let result = success(vec![json!({ "id": 1 }), json!({ "id": 2 })]);
        let structured = result.structured_content.unwrap();

        assert_eq!(structured, json!({ "items": [{ "id": 1 }, { "id": 2 }] }));
        let text = result.content[0].as_text().unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&text.text).unwrap(),
            structured
        );
    }
}
