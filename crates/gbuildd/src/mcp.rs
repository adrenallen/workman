//! Core MCP service: identity, scoping, setup tools, and project tools.

use std::{
    collections::BTreeMap,
    time::{SystemTime, UNIX_EPOCH},
};

use axum::http::request::Parts;
use gbuild_core::{Actor, Process, ProcessId, ProcessStatus, Project, ProjectId};
use rmcp::{
    ServerHandler,
    handler::server::{router::tool::ToolRouter, tool::Extension, wrapper::Parameters},
    model::{CallToolResult, Implementation, ServerCapabilities, ServerInfo},
    schemars, tool, tool_handler, tool_router,
    transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
    },
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::{ProcessRegistry, SharedProcessRegistry};

mod agent_spawning;
mod tools_lock;
mod tools_process;
mod tools_readiness;
mod tools_timer;
mod tools_todo;

pub const GBUILD_MCP_TOKEN_HEADER: &str = "x-gbuild-mcp-token";

#[derive(Clone)]
pub struct GbuildMcp {
    registry: SharedProcessRegistry,
    tool_router: ToolRouter<Self>,
}

impl GbuildMcp {
    pub fn new(registry: SharedProcessRegistry) -> Self {
        let mut tool_router = Self::tool_router();
        tool_router.merge(Self::process_tool_router());
        tool_router.merge(Self::readiness_tool_router());
        tool_router.merge(Self::agent_spawning_tool_router());
        tool_router.merge(Self::todo_tool_router());
        tool_router.merge(Self::lock_tool_router());
        tool_router.merge(Self::timer_tool_router());
        Self {
            registry,
            tool_router,
        }
    }
}

pub fn streamable_http_service(
    registry: SharedProcessRegistry,
) -> StreamableHttpService<GbuildMcp, LocalSessionManager> {
    let config = StreamableHttpServerConfig::default().with_json_response(true);
    StreamableHttpService::new(
        move || Ok(GbuildMcp::new(registry.clone())),
        LocalSessionManager::default().into(),
        config,
    )
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct IdentifySessionArgs {
    /// Stable gbuild process ID to associate with this MCP session.
    process_id: ProcessId,
}

#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
struct HelpArgs {
    /// Optional topic: setup, identity, scoping, projects, or tools.
    #[serde(default)]
    topic: Option<String>,
}

#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
struct ProjectScopeArgs {
    /// Explicit project override. Otherwise selected project, then owning project is used.
    #[serde(default)]
    project_id: Option<ProjectId>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ProjectSelectArgs {
    project_id: ProjectId,
}

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
impl GbuildMcp {
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

    #[tool(description = "Manually associate this MCP session with a gbuild process")]
    async fn identify_session(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<IdentifySessionArgs>,
    ) -> CallToolResult {
        let mut registry = self.registry.lock().await;
        let (mut actor, existing_process) = match ensure_actor(&mut registry, &parts) {
            Ok(identity) => identity,
            Err(error) => return failure("identity_error", error),
        };
        if let Some(process) = existing_process {
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
        let process = match registry.store().get_process(args.process_id) {
            Ok(Some(process)) => process,
            Ok(None) => {
                return failure(
                    "process_not_found",
                    format!("process {} was not found", args.process_id),
                );
            }
            Err(error) => return failure("store_error", error.to_string()),
        };
        actor.process_id = Some(process.id);
        actor.last_seen_at = now_millis();
        if let Err(error) = registry.store().put_actor(&actor) {
            return failure("store_error", error.to_string());
        }
        let effective_project_id = resolve_project_id(&registry, &actor, None).ok();
        success(IdentityResult {
            actor_id: actor.id,
            session_id: actor.session_id,
            process_id: actor.process_id,
            process_name: Some(process.name),
            effective_project_id,
            selected_project_id: actor.selected_project_id,
        })
    }

    #[tool(description = "Show concise gbuild MCP help, optionally for one topic")]
    async fn help(&self, Parameters(args): Parameters<HelpArgs>) -> CallToolResult {
        let topic = args.topic.as_deref().unwrap_or("setup");
        let text = match topic {
            "setup" => {
                "Connect to /mcp with Streamable HTTP. Daemon-spawned agents send x-gbuild-mcp-token; external clients use the daemon bearer token then identify_session."
            }
            "identity" => {
                "whoami auto-resolves a process token. identify_session is the explicit fallback for externally launched sessions."
            }
            "scoping" => {
                "Project-scoped tools resolve explicit project_id first, then the session-selected project, then the identified process's project."
            }
            "projects" => {
                "list_projects/select_project/get_project/get_project_status/get_project_stats/create_project/rename_project/delete_project manage registered workspaces. Delete always requires confirm_delete and active processes require confirm_stop_running."
            }
            "tools" => "Use mcp_tools_summary for the complete core tool list.",
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

    #[tool(description = "List all registered projects")]
    async fn list_projects(&self) -> CallToolResult {
        let registry = self.registry.lock().await;
        match registry.store().list_projects() {
            Ok(projects) => success(json!({ "projects": projects })),
            Err(error) => failure("store_error", error.to_string()),
        }
    }

    #[tool(description = "Select the effective project for this MCP session")]
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
        success(json!({ "project": project, "actor_id": actor.id }))
    }

    #[tool(description = "Get the effective or explicitly requested project")]
    async fn get_project(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<ProjectScopeArgs>,
    ) -> CallToolResult {
        let mut registry = self.registry.lock().await;
        match scoped_project(&mut registry, &parts, args.project_id) {
            Ok((project, _)) => success(project),
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
        match registry.list_statuses(Some(project.id)) {
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

    #[tool(description = "Register an existing directory as a project")]
    async fn create_project(
        &self,
        Parameters(args): Parameters<ProjectCreateArgs>,
    ) -> CallToolResult {
        let canonical = match std::fs::canonicalize(&args.path) {
            Ok(path) if path.is_dir() => path,
            Ok(_) => return failure("invalid_project_path", "project path is not a directory"),
            Err(error) => return failure("invalid_project_path", error.to_string()),
        };
        let default_name = canonical
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("project")
            .to_owned();
        let name = args.name.unwrap_or(default_name);
        if name.trim().is_empty() {
            return failure("invalid_project_name", "project name must not be empty");
        }
        let registry = self.registry.lock().await;
        let canonical = canonical.to_string_lossy().into_owned();
        match registry.store().list_projects() {
            Ok(projects) => {
                if let Some(project) = projects
                    .into_iter()
                    .find(|project| project.path == canonical)
                {
                    return success(project);
                }
            }
            Err(error) => return failure("store_error", error.to_string()),
        }
        let project = Project {
            id: match registry.store().next_project_id() {
                Ok(id) => id,
                Err(error) => return failure("store_error", error.to_string()),
            },
            path: canonical,
            name,
            display_name: args.display_name,
            icon: args.icon,
            selected: false,
        };
        match registry.store().put_project(&project) {
            Ok(()) => success(project),
            Err(error) => failure("project_create_failed", error.to_string()),
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
        project.name = args.name;
        match registry.store().put_project(&project) {
            Ok(()) => success(project),
            Err(error) => failure("project_rename_failed", error.to_string()),
        }
    }

    #[tool(description = "Delete a project after explicit confirmation")]
    async fn delete_project(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<ProjectDeleteArgs>,
    ) -> CallToolResult {
        let mut registry = self.registry.lock().await;
        let (project, _) = match scoped_project(&mut registry, &parts, args.project_id) {
            Ok(scoped) => scoped,
            Err(error) => return failure("project_scope_error", error),
        };
        if !args.confirm_delete {
            return failure(
                "confirmation_required",
                "set confirm_delete=true to delete this project",
            );
        }
        let processes = match registry.list(Some(project.id)) {
            Ok(processes) => processes,
            Err(error) => return failure(error.code(), error.to_string()),
        };
        let has_running = processes.iter().any(|process| {
            matches!(
                process.status,
                ProcessStatus::Starting | ProcessStatus::Running
            )
        });
        if has_running && !args.confirm_stop_running {
            return failure(
                "running_confirmation_required",
                "project has running processes; also set confirm_stop_running=true",
            );
        }
        for process in processes {
            if let Err(error) = registry.close(process.id) {
                return failure(error.code(), error.to_string());
            }
        }
        match registry.store().delete_project(project.id) {
            Ok(true) => success(json!({ "project_id": project.id, "deleted": true })),
            Ok(false) => failure(
                "project_not_found",
                format!("project {} was not found", project.id),
            ),
            Err(error) => failure("project_delete_failed", error.to_string()),
        }
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for GbuildMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("gbuild", env!("CARGO_PKG_VERSION")))
            .with_instructions(
                "gbuild workspace control. Call whoami first; project tools use explicit, selected, then owning-project scope.",
            )
    }
}

fn ensure_actor(
    registry: &mut ProcessRegistry,
    parts: &Parts,
) -> Result<(Actor, Option<Process>), String> {
    let token = parts
        .headers
        .get(GBUILD_MCP_TOKEN_HEADER)
        .and_then(|value| value.to_str().ok());
    let token_process = match token {
        Some(token) => registry
            .store()
            .get_process_by_mcp_token(token)
            .map_err(|error| error.to_string())?,
        None => None,
    };
    let session_id = parts
        .headers
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
        .or_else(|| {
            token_process
                .as_ref()
                .map(|process| format!("process:{}", process.id))
        })
        .unwrap_or_else(|| format!("anonymous:{}", Uuid::new_v4().simple()));
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
    if let Some(project_id) = explicit_project_id {
        return registry
            .store()
            .get_project(project_id)
            .map_err(|error| error.to_string())?
            .map(|project| project.id)
            .ok_or_else(|| format!("project {project_id} was not found"));
    }
    if let Some(project_id) = actor.selected_project_id
        && registry
            .store()
            .get_project(project_id)
            .map_err(|error| error.to_string())?
            .is_some()
    {
        return Ok(project_id);
    }
    let process_id = actor.process_id.ok_or_else(|| {
        "session has no selected or owning project; call select_project or identify_session"
            .to_owned()
    })?;
    registry
        .store()
        .get_process(process_id)
        .map_err(|error| error.to_string())?
        .map(|process| process.project_id)
        .ok_or_else(|| format!("process {process_id} was not found"))
}

fn success(value: impl Serialize) -> CallToolResult {
    match serde_json::to_value(value) {
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
