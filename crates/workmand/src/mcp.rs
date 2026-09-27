//! Core MCP service: identity, scoping, setup tools, and project tools.

use std::{
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
    RoleServer, ServerHandler,
    handler::server::{router::tool::ToolRouter, tool::Extension, wrapper::Parameters},
    model::{
        CallToolResult, Implementation, ListToolsResult, ServerCapabilities, ServerInfo, Tool,
    },
    schemars,
    service::RequestContext,
    tool, tool_handler, tool_router,
    transport::streamable_http_server::{
        SessionId, SessionManager, StreamableHttpServerConfig, StreamableHttpService,
        session::{local::LocalSessionManager, never::NeverSessionManager},
    },
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;
use workman_core::{Actor, Process, ProcessId, Project, ProjectId};

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
pub(crate) const SCRATCHPAD_HANDOFF_GUIDANCE: &str = "Put shared notes, plans, briefs, and hand-offs in Workman scratchpads with scratchpad_write so they are visible in the app and verifiable; do not create ad-hoc repo files for them. Review feedback with scratchpad_read(include_comments=true), and use scratchpad_comment_create for anchored or whole-document discussion. Agents may update, resolve, reopen, or delete only comments they authored; the human may resolve any project comment. After creating a scratchpad or todo, read it back with scratchpad_read or todo_get and reference its ID in every hand-off message.";
pub(crate) const HUMAN_HANDOFF_GUIDANCE: &str = "Found something out of scope or need human feedback? File a todo or add a comment, then use todo_update(assignee=\"user\") or mention @user in a new todo comment. A fresh user assignment and each new @user comment notify the human; unrelated edits and comment edits do not. Use todo_update(assignee=\"none\") to unassign.";
pub(crate) const SPAWN_AGENT_GUIDANCE: &str = "spawn_agent launches a plain agent by default: pick agent_tool_id from list_agent_tools and omit agent_template_id. Template summaries include launch settings. Use a template only when the user names a template or explicitly asks for one. The template supplies its tool, launch args, and prompt. A tool override carries the template model only for the same agent type; compatible Claude/Codex effort may carry, and command defaults never carry. A selected model arg supersedes the registered command model; an explicit caller model also replaces template and caller model flags. resolved reports effective settings and skipped template args. Set notify_spawner_on_idle=true for a coalesced completion turn; update_process can toggle an existing direct child. Delivery never merges into the human's unsent draft.";
pub(crate) const IDLE_TIMER_WAIT_GUIDANCE: &str = "For a child spawned with notify_spawner_on_idle=true, no timer is needed for ordinary completion wake-up. The opt-in is prospective and survives child exit, crash, and restart. Keep a delay timer when a hung-child deadline matters. For other waits, call timer_fire_when_idle once with wait_for=\"any\" or wait_for=\"all\". any may deliver immediately for a newly reported completion; all counts processes already idle at arm time. Arm results expose already_idle and satisfied_by diagnostics. deadline means the timeout fired without reporting completion. When already_satisfied=false and the timer delivers to this agent, finish the response and end the turn; do not poll timer_list or process status. When the fresh turn arrives, inspect watched processes because the deadline may have fired or an agent may be waiting on its own timer.";
const SERVER_INSTRUCTIONS: &str = "Need human input or found out-of-scope work? Create a todo or comment, then use todo_update(assignee=\"user\") or mention @user in a new todo comment; either notifies the human. Call whoami first. Process credentials jail agents to their owning project; cross-project IDs and indirect targets are rejected. Unidentified bearer sessions have discovery and help only. Use help for todos, scratchpads, worktrees, timers, tools, and spawning.";

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

#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
struct HelpArgs {
    /// Optional topic: setup, identity, scoping, projects, todos, scratchpads, worktrees, timers, tools, or spawning.
    #[serde(default)]
    topic: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ProjectUpdateArgs {
    #[serde(default)]
    project_id: Option<ProjectId>,
    name: String,
}

fn apply_explicit_project_name(project: &mut Project, name: &str) -> Result<(), &'static str> {
    let name = normalized_project_title(name).ok_or("project name must not be empty")?;
    project.display_name = Some(name.to_owned());
    Ok(())
}

#[derive(Debug, Serialize)]
struct IdentityResult {
    actor_id: String,
    session_id: String,
    process_id: Option<ProcessId>,
    process_name: Option<String>,
    effective_project_id: Option<ProjectId>,
    selected_project_id: Option<ProjectId>,
    project: Option<Value>,
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
                let project = match effective_project_id {
                    Some(project_id) => match registry.store().get_project(project_id) {
                        Ok(Some(project)) => {
                            match crate::worktrees::project_envelope(registry.store(), project) {
                                Ok(project) => Some(json!(project)),
                                Err(error) => return failure(error.code(), error.to_string()),
                            }
                        }
                        Ok(None) => None,
                        Err(error) => return failure("store_error", error.to_string()),
                    },
                    None => None,
                };
                success(IdentityResult {
                    actor_id: actor.id,
                    session_id: actor.session_id,
                    process_id: actor.process_id,
                    process_name: process.map(|process| process.name),
                    effective_project_id,
                    selected_project_id: actor.selected_project_id,
                    project,
                })
            }
            Err(error) => failure("identity_error", error),
        }
    }

    #[tool(description = "Show concise Workman MCP help, optionally for one topic")]
    async fn help(&self, Parameters(args): Parameters<HelpArgs>) -> CallToolResult {
        let topic = args.topic.as_deref().unwrap_or("setup");
        let text = match topic {
            "setup" => {
                "Connect to /mcp with Streamable HTTP. Daemon-spawned agents authenticate with their process credential and are automatically jailed to their owning project. A daemon bearer authenticates user-level discovery only and cannot claim a process identity."
            }
            "identity" => {
                "whoami resolves the process credential supplied by the launcher. Identity cannot be claimed or retargeted. If whoami is unidentified or names the wrong process, stop and report a launch-wiring error."
            }
            "scoping" => {
                "Agent identities are jailed by the daemon to their owning project. Cross-project project_id overrides and indirect process or timer targets are rejected; project creation and global configuration are unavailable. Unidentified bearer sessions may use discovery and help but cannot claim a process or perform project-scoped actions. The authenticated UI/CLI control channel remains user-scoped and can manage every project."
            }
            "projects" => {
                "Project-scoped MCP tools operate only on the calling agent's owning project. whoami includes project metadata; list_processes includes process status and counts. Project creation and removal stay in the authenticated UI/CLI control channel."
            }
            "todos" => HUMAN_HANDOFF_GUIDANCE,
            "scratchpads" => SCRATCHPAD_HANDOFF_GUIDANCE,
            "worktrees" => {
                "Use worktree_list to inspect repository worktrees and cached pull-request status. Creation, adoption, and removal stay in the authenticated UI/CLI control channel."
            }
            "timers" => IDLE_TIMER_WAIT_GUIDANCE,
            "tools" => "Use the MCP tools/list request for the complete tool list and schemas.",
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

    #[tool(description = "Update the effective or explicitly requested project")]
    async fn project_update(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<ProjectUpdateArgs>,
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
            Err(error) => failure("project_update_failed", error.to_string()),
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
            .with_instructions(SERVER_INSTRUCTIONS)
    }

    async fn list_tools(
        &self,
        _request: Option<rmcp::model::PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, rmcp::ErrorData> {
        let process_identity = self.request_has_process_identity(&context).await;
        let tools = self
            .tool_router
            .list_all()
            .into_iter()
            .filter(|tool| !process_identity || tool.name.as_ref() != "agent_tool_configure")
            .map(sanitize_tool_schema)
            .collect();
        Ok(ListToolsResult {
            tools,
            ..Default::default()
        })
    }
}

impl WorkmanMcp {
    async fn request_has_process_identity(&self, context: &RequestContext<RoleServer>) -> bool {
        let Some(parts) = context.extensions.get::<Parts>() else {
            return false;
        };
        let token = parts
            .headers
            .get(WORKMAN_MCP_TOKEN_HEADER)
            .or_else(|| parts.headers.get(header::AUTHORIZATION))
            .and_then(|value| value.to_str().ok())
            .map(|value| value.strip_prefix("Bearer ").unwrap_or(value));
        let Some(token) = token else {
            return false;
        };
        self.registry
            .lock()
            .await
            .store()
            .get_process_by_mcp_token(token)
            .ok()
            .flatten()
            .is_some()
    }
}

fn sanitize_tool_schema(mut tool: Tool) -> Tool {
    let mut input_schema = Value::Object((*tool.input_schema).clone());
    sanitize_schema_value(&mut input_schema);
    if let Value::Object(schema) = input_schema {
        tool.input_schema = Arc::new(schema);
    }
    if let Some(output_schema) = tool.output_schema.take() {
        let mut output_schema = Value::Object((*output_schema).clone());
        sanitize_schema_value(&mut output_schema);
        if let Value::Object(schema) = output_schema {
            tool.output_schema = Some(Arc::new(schema));
        }
    }
    tool
}

fn sanitize_schema_value(value: &mut Value) {
    match value {
        Value::Array(values) => {
            for value in values {
                sanitize_schema_value(value);
            }
        }
        Value::Object(schema) => {
            schema.remove("$schema");
            if schema.get("default").is_some_and(Value::is_null) {
                schema.remove("default");
            }

            if let Some(Value::Object(properties)) = schema.get_mut("properties") {
                properties.remove("project_id");
            }
            if let Some(Value::Array(required)) = schema.get_mut("required") {
                required.retain(|name| name.as_str() != Some("project_id"));
                if required.is_empty() {
                    schema.remove("required");
                }
            }

            if let Some(Value::Array(types)) = schema.get_mut("type") {
                types.retain(|schema_type| schema_type.as_str() != Some("null"));
                if types.len() == 1 {
                    let schema_type = types.pop().expect("one schema type remains");
                    schema.insert("type".into(), schema_type);
                }
            }

            for child in schema.values_mut() {
                sanitize_schema_value(child);
            }

            let is_integer = match schema.get("type") {
                Some(Value::String(schema_type)) => schema_type == "integer",
                Some(Value::Array(types)) => types
                    .iter()
                    .any(|schema_type| schema_type.as_str() == Some("integer")),
                _ => false,
            };
            if is_integer {
                schema.remove("format");
                if schema.get("minimum").and_then(Value::as_i64) == Some(0) {
                    schema.remove("minimum");
                }
            }
        }
        _ => {}
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
