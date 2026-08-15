//! Agent-tool discovery and local terminal/agent spawning MCP tools.

use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use axum::http::request::Parts;
use rmcp::{
    handler::server::{tool::Extension, wrapper::Parameters},
    model::CallToolResult,
    schemars, tool, tool_router,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use toml_edit::{DocumentMut, Item, Table, Value as TomlValue};
use uuid::Uuid;
use workman_core::{
    AgentTemplate, AgentTemplateId, AgentTool, AgentToolId, AgentToolSource, Process, ProcessId,
    ProcessKind, ProcessSource, ProcessStatus, Project, ProjectId,
    attention::{AttentionState, DEFAULT_IDLE_AFTER},
};

use super::{
    SCRATCHPAD_HANDOFF_GUIDANCE, WORKTREE_AGENT_GUIDANCE, WorkmanMcp, ensure_actor, failure,
    process_project_id, scoped_project, success,
};
use crate::ProcessRegistry;

const WORKMAN_MCP_URL_ENV: &str = "WORKMAN_MCP_URL";
const OPENCODE_CONFIG_CONTENT_ENV: &str = "OPENCODE_CONFIG_CONTENT";
pub(crate) const WORKMAN_EPHEMERAL_AGENT_HOME_ENV: &str = "WORKMAN_EPHEMERAL_AGENT_HOME";
const KIMI_MCP_TEMPLATE_FILE: &str = ".workman-mcp-template.json";
const KIMI_MCP_TOKEN_SENTINEL: &str = "__WORKMAN_KIMI_PROCESS_MCP_TOKEN__";
const INITIAL_DIALOG_TIMEOUT: Duration = Duration::from_secs(3);
const INITIAL_OUTPUT_SETTLE: Duration = Duration::from_millis(750);
const INITIAL_OUTPUT_QUIET: Duration = Duration::from_millis(200);
const DIALOG_CLEAR_TIMEOUT: Duration = Duration::from_secs(2);
const DIALOG_POLL_INTERVAL: Duration = Duration::from_millis(10);
const INITIAL_PROMPT_READY_TIMEOUT: Duration = Duration::from_secs(60);
const INITIAL_PROMPT_HARD_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const INITIAL_PROMPT_POLL_INTERVAL: Duration = Duration::from_millis(50);
const AGENT_TEMPLATE_NAME_MAX_CHARS: usize = 120;
const AGENT_TEMPLATE_PROMPT_MAX_BYTES: usize = 64 * 1024;
const AGENT_TEMPLATE_EXTRA_ARGS_MAX_ITEMS: usize = 64;
const AGENT_TEMPLATE_EXTRA_ARGS_MAX_BYTES: usize = 4 * 1024;
const INITIAL_PROMPT_MAX_BYTES: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
enum SpawnKind {
    Terminal,
    Agent,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SpawnProcessArgs {
    /// Optional project ID; an identified agent may name only its owning project.
    #[serde(default)]
    project_id: Option<ProjectId>,
    /// Only interactive terminals and managed agents may be launched through this tool.
    kind: SpawnKind,
    /// Optional per-launch process name, unique within the project.
    #[serde(default)]
    name: Option<String>,
    /// Agent-tool registry ID. Required for kind=agent and rejected for kind=terminal.
    #[serde(default)]
    agent_tool_id: Option<AgentToolId>,
    /// Safely shell-quoted arguments appended to the registered agent command.
    #[serde(default)]
    extra_args: Vec<String>,
    /// Automatically accept narrowly recognized first-run trust dialogs.
    #[serde(default = "default_true")]
    auto_acknowledge_dialogs: bool,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SpawnAgentArgs {
    /// Optional project ID; an identified agent may name only its owning project.
    #[serde(default)]
    project_id: Option<ProjectId>,
    /// Agent-tool registry ID. Optional when agent_template_id is provided.
    #[serde(default)]
    agent_tool_id: Option<AgentToolId>,
    /// Optional agent template. Its registered tool and launch arguments take precedence.
    #[serde(default)]
    agent_template_id: Option<AgentTemplateId>,
    /// Optional per-launch process name, unique within the project.
    #[serde(default)]
    name: Option<String>,
    /// Safely shell-quoted arguments appended to the registered agent command.
    #[serde(default)]
    extra_args: Vec<String>,
    /// Optional first prompt delivered once the agent reaches a safe input state.
    #[serde(default)]
    initial_prompt: Option<String>,
    /// Automatically accept narrowly recognized first-run trust dialogs.
    #[serde(default = "default_true")]
    auto_acknowledge_dialogs: bool,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AgentToolConfigArgs {
    agent_tool_id: AgentToolId,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AgentToolConfigWriteArgs {
    agent_tool_id: AgentToolId,
    /// Must be true after the complete resulting config has been shown to the user.
    confirm_write: bool,
    /// SHA-256 returned by agent_tool_configure_preview; prevents stale writes.
    expected_preview_sha256: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AgentToolDeepCheckArgs {
    /// Optional project ID; an identified agent may name only its owning project.
    #[serde(default)]
    project_id: Option<ProjectId>,
    agent_tool_id: AgentToolId,
    /// Hard deadline for the ephemeral whoami roundtrip (default 30s, maximum 60s).
    #[serde(default)]
    timeout_ms: Option<u64>,
}

const fn default_true() -> bool {
    true
}

#[derive(Debug, Serialize)]
pub(crate) struct SpawnResult {
    process_id: ProcessId,
    project_id: ProjectId,
    name: String,
    kind: ProcessKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    agent_instructions: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct McpLaunchCapability {
    pub supported: bool,
    pub mechanism: &'static str,
    pub note: &'static str,
}

#[derive(Debug)]
struct AgentLaunchPlan {
    command: String,
    env: BTreeMap<String, String>,
}

#[derive(Debug)]
struct PreparedRegisteredAgent {
    tool: AgentTool,
    requested_name: Option<String>,
    launch: AgentLaunchPlan,
}

#[derive(Debug)]
struct ResolvedAgentSpawn {
    agent_tool_id: AgentToolId,
    extra_args: Vec<String>,
    initial_prompt: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AgentLaunchPurpose {
    Normal,
    DeepCheck,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum McpLaunchAdapter {
    Claude,
    Codex,
    Gemini,
    OpenCode,
    Grok,
    Kimi,
    Unsupported,
}

impl McpLaunchAdapter {
    fn capability(self) -> McpLaunchCapability {
        match self {
            Self::Claude => McpLaunchCapability {
                supported: true,
                mechanism: "--mcp-config + --strict-mcp-config",
                note: "Workman injects a strict inline MCP config for every launch.",
            },
            Self::Codex => McpLaunchCapability {
                supported: true,
                mechanism: "-c mcp_servers.workman overrides",
                note: "Workman injects URL and environment-backed header overrides for every launch.",
            },
            Self::Gemini => McpLaunchCapability {
                supported: true,
                mechanism: "GEMINI_CLI_SYSTEM_SETTINGS_PATH",
                note: "Workman injects a private, ephemeral highest-precedence settings file for every launch.",
            },
            Self::OpenCode => McpLaunchCapability {
                supported: true,
                mechanism: "OPENCODE_CONFIG_CONTENT",
                note: "Workman injects an inline runtime config for every launch, including model variants.",
            },
            Self::Grok => McpLaunchCapability {
                supported: true,
                mechanism: "private per-launch GROK_HOME config",
                note: "Workman injects a private Grok config home with the current URL and environment-backed token header; the user's config.toml is never changed.",
            },
            Self::Kimi => McpLaunchCapability {
                supported: true,
                mechanism: "private per-launch KIMI_CODE_HOME config",
                note: "Workman injects a private Kimi Code home with the current URL and process token header; the user's mcp.json is never changed.",
            },
            Self::Unsupported => McpLaunchCapability {
                supported: false,
                mechanism: "unsupported",
                note: "This tool type has no registered per-launch Workman MCP adapter.",
            },
        }
    }
}

#[tool_router(router = agent_spawning_tool_router, vis = "pub(crate)")]
impl WorkmanMcp {
    #[tool(
        description = "List enabled and disabled built-in or custom agent command presets. Returns { agent_tools: [...] }"
    )]
    async fn list_agent_tools(&self) -> CallToolResult {
        let registry = self.registry.lock().await;
        match load_agent_tools(&registry) {
            Ok(tools) => success(json!({ "agent_tools": tools })),
            Err(error) => failure("store_error", error),
        }
    }

    #[tool(
        description = "List reusable agent templates for the active workspace profile. Returns { agent_templates: [...] }"
    )]
    async fn list_agent_templates(&self) -> CallToolResult {
        let registry = self.registry.lock().await;
        match load_agent_templates(&registry) {
            Ok(templates) => success(json!({ "agent_templates": templates })),
            Err(error) => failure("store_error", error),
        }
    }

    #[tool(
        description = "Run cheap PATH, version, and config-presence checks for every agent runtime"
    )]
    async fn agent_tools_health(&self) -> CallToolResult {
        let (tools, user_environment) = {
            let registry = self.registry.lock().await;
            match load_agent_tools(&registry) {
                Ok(tools) => (tools, registry.resolved_user_environment()),
                Err(error) => return failure("store_error", error),
            }
        };
        success(
            crate::runtime_doctor::check_agent_tools_with_user_environment(
                tools,
                &user_environment,
            )
            .await,
        )
    }

    #[tool(
        description = "Preview the complete consent-gated workman MCP config for one agent runtime (global configuration is unavailable to agent identities)"
    )]
    async fn agent_tool_configure_preview(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<AgentToolConfigArgs>,
    ) -> CallToolResult {
        let tool = {
            let mut registry = self.registry.lock().await;
            let (actor, _) = match ensure_actor(&mut registry, &parts) {
                Ok(identity) => identity,
                Err(error) => return failure("identity_error", error),
            };
            match process_project_id(&registry, &actor) {
                Ok(Some(project_id)) => {
                    return failure(
                        "project_scope_error",
                        format!(
                            "agent identities are scoped to project {project_id}; inspecting global agent configuration is outside that scope"
                        ),
                    );
                }
                Ok(None) => {}
                Err(error) => return failure("project_scope_error", error),
            }
            match load_agent_tool(&registry, args.agent_tool_id) {
                Ok(tool) => tool,
                Err(error) => return failure("agent_tool_error", error),
            }
        };
        match crate::runtime_doctor::config_preview(&tool, &self.mcp_url) {
            Ok(preview) => success(preview),
            Err(error) => failure("agent_config_error", error),
        }
    }

    #[tool(
        description = "Write a previously previewed agent MCP config after explicit confirmation (global configuration is unavailable to agent identities)"
    )]
    async fn agent_tool_configure(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<AgentToolConfigWriteArgs>,
    ) -> CallToolResult {
        let tool = {
            let mut registry = self.registry.lock().await;
            let (actor, _) = match ensure_actor(&mut registry, &parts) {
                Ok(identity) => identity,
                Err(error) => return failure("identity_error", error),
            };
            match process_project_id(&registry, &actor) {
                Ok(Some(project_id)) => {
                    return failure(
                        "project_scope_error",
                        format!(
                            "agent identities are scoped to project {project_id}; changing global agent configuration is outside that scope"
                        ),
                    );
                }
                Ok(None) => {}
                Err(error) => return failure("project_scope_error", error),
            }
            match load_agent_tool(&registry, args.agent_tool_id) {
                Ok(tool) => tool,
                Err(error) => return failure("agent_tool_error", error),
            }
        };
        match crate::runtime_doctor::apply_config(
            &tool,
            &self.mcp_url,
            args.confirm_write,
            &args.expected_preview_sha256,
        ) {
            Ok(result) => success(result),
            Err(error) => failure("agent_config_error", error),
        }
    }

    #[tool(
        description = "Optionally spawn one ephemeral agent and verify its workman whoami roundtrip"
    )]
    async fn agent_tool_deep_check(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<AgentToolDeepCheckArgs>,
    ) -> CallToolResult {
        let (project_id, spawned_by_process_id) = {
            let mut registry = self.registry.lock().await;
            match scoped_project(&mut registry, &parts, args.project_id) {
                Ok((project, actor)) => (project.id, actor.process_id),
                Err(error) => return failure("project_scope_error", error),
            }
        };
        match deep_check_registered_agent(
            self.registry.clone(),
            project_id,
            args.agent_tool_id,
            &self.mcp_url,
            args.timeout_ms,
            spawned_by_process_id,
        )
        .await
        {
            Ok(result) => success(result),
            Err(error) => failure("deep_check_failed", error),
        }
    }

    #[tool(description = "Spawn a managed interactive terminal or registered agent process")]
    async fn spawn_process(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<SpawnProcessArgs>,
    ) -> CallToolResult {
        let (project, spawned_by_process_id) = {
            let mut registry = self.registry.lock().await;
            match scoped_project(&mut registry, &parts, args.project_id) {
                Ok((project, actor)) => (project, actor.process_id),
                Err(error) => return failure("project_scope_error", error),
            }
        };

        let result = match args.kind {
            SpawnKind::Terminal => {
                if args.agent_tool_id.is_some() {
                    return failure(
                        "invalid_arguments",
                        "agent_tool_id is only valid when kind=agent",
                    );
                }
                if !args.extra_args.is_empty() {
                    return failure(
                        "invalid_arguments",
                        "extra_args is only valid when kind=agent",
                    );
                }
                let mut registry = self.registry.lock().await;
                process_name(&registry, project.id, args.name, "terminal").and_then(|name| {
                    let shell = registry
                        .resolved_user_environment()
                        .active_shell()
                        .to_string_lossy()
                        .into_owned();
                    spawn(
                        &mut registry,
                        &project,
                        ProcessKind::Terminal,
                        name,
                        shell,
                        None,
                        None,
                        BTreeMap::new(),
                        spawned_by_process_id,
                    )
                })
            }
            SpawnKind::Agent => {
                let Some(agent_tool_id) = args.agent_tool_id else {
                    return failure(
                        "agent_tool_required",
                        "agent_tool_id is required when kind=agent",
                    );
                };
                spawn_registered_agent(
                    self.registry.clone(),
                    project,
                    Some(agent_tool_id),
                    None,
                    args.name,
                    args.extra_args,
                    None,
                    &self.mcp_url,
                    args.auto_acknowledge_dialogs,
                    spawned_by_process_id,
                )
                .await
            }
        };
        match result {
            Ok(result) => success(result),
            Err(error) => failure("spawn_failed", error),
        }
    }

    #[tool(
        description = "Spawn a registered agent and return the identity preamble for its first prompt"
    )]
    async fn spawn_agent(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<SpawnAgentArgs>,
    ) -> CallToolResult {
        if let Err(error) = validate_initial_prompt(args.initial_prompt.as_deref()) {
            return failure("invalid_params", error);
        }
        let (project, spawned_by_process_id) = {
            let mut registry = self.registry.lock().await;
            match scoped_project(&mut registry, &parts, args.project_id) {
                Ok((project, actor)) => (project, actor.process_id),
                Err(error) => return failure("project_scope_error", error),
            }
        };
        match spawn_registered_agent(
            self.registry.clone(),
            project,
            args.agent_tool_id,
            args.agent_template_id,
            args.name,
            args.extra_args,
            args.initial_prompt,
            &self.mcp_url,
            args.auto_acknowledge_dialogs,
            spawned_by_process_id,
        )
        .await
        {
            Ok(result) => success(result),
            Err(error) => failure("spawn_failed", error),
        }
    }
}

pub(crate) fn load_agent_tools(registry: &ProcessRegistry) -> Result<Vec<AgentTool>, String> {
    registry
        .store()
        .list_agent_tools()
        .map_err(|error| error.to_string())
}

pub(crate) fn load_agent_templates(
    registry: &ProcessRegistry,
) -> Result<Vec<AgentTemplate>, String> {
    registry
        .store()
        .list_agent_templates()
        .map_err(|error| error.to_string())
}

pub(crate) fn save_agent_template_from_settings(
    registry: &ProcessRegistry,
    id: Option<AgentTemplateId>,
    name: String,
    agent_tool_id: AgentToolId,
    extra_args: Vec<String>,
    prompt: String,
) -> Result<AgentTemplate, String> {
    let name = name.trim().to_owned();
    validate_agent_template_fields(&name, &prompt, &extra_args)?;
    load_agent_tool(registry, agent_tool_id)?;
    if let Some(id) = id
        && registry
            .store()
            .get_agent_template(id)
            .map_err(|error| error.to_string())?
            .is_none()
    {
        return Err(format!("agent template {id} was not found"));
    }
    if load_agent_templates(registry)?
        .iter()
        .any(|template| Some(template.id) != id && template.name.eq_ignore_ascii_case(&name))
    {
        return Err(format!(
            "agent template name {name:?} is already registered"
        ));
    }
    let template = AgentTemplate {
        id: id.unwrap_or(
            registry
                .store()
                .next_agent_template_id()
                .map_err(|error| error.to_string())?,
        ),
        profile_id: registry
            .store()
            .active_profile_id()
            .map_err(|error| error.to_string())?,
        name,
        agent_tool_id,
        extra_args,
        prompt: prompt.trim().to_owned(),
        sort_order: 0,
        created_at: 0,
        updated_at: 0,
    };
    registry
        .store()
        .put_agent_template(&template)
        .map_err(|error| error.to_string())?;
    registry
        .store()
        .get_agent_template(template.id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("agent template {} was not found after saving", template.id))
}

fn validate_agent_template_fields(
    name: &str,
    prompt: &str,
    extra_args: &[String],
) -> Result<(), String> {
    if name.is_empty() {
        return Err("agent template name cannot be empty".to_owned());
    }
    if name.chars().count() > AGENT_TEMPLATE_NAME_MAX_CHARS {
        return Err(format!(
            "agent template name must be {AGENT_TEMPLATE_NAME_MAX_CHARS} characters or fewer"
        ));
    }
    if prompt.len() > AGENT_TEMPLATE_PROMPT_MAX_BYTES {
        return Err(format!(
            "agent template prompt must be {AGENT_TEMPLATE_PROMPT_MAX_BYTES} bytes or fewer"
        ));
    }
    if extra_args.len() > AGENT_TEMPLATE_EXTRA_ARGS_MAX_ITEMS {
        return Err(format!(
            "agent template may have at most {AGENT_TEMPLATE_EXTRA_ARGS_MAX_ITEMS} arguments"
        ));
    }
    let extra_args_bytes = extra_args.iter().try_fold(0usize, |total, arg| {
        total
            .checked_add(arg.len())
            .ok_or_else(|| "agent template argument size exceeds the supported range".to_owned())
    })?;
    if extra_args_bytes > AGENT_TEMPLATE_EXTRA_ARGS_MAX_BYTES {
        return Err(format!(
            "agent template arguments must total {AGENT_TEMPLATE_EXTRA_ARGS_MAX_BYTES} bytes or fewer"
        ));
    }
    if name.contains('\0') {
        return Err("agent template name may not contain NUL bytes".to_owned());
    }
    if prompt.contains('\0') {
        return Err("agent template prompt may not contain NUL bytes".to_owned());
    }
    if extra_args.iter().any(|arg| arg.contains('\0')) {
        return Err("agent template arguments may not contain NUL bytes".to_owned());
    }
    Ok(())
}

pub(crate) fn validate_initial_prompt(prompt: Option<&str>) -> Result<(), String> {
    if prompt.is_some_and(|prompt| prompt.len() > INITIAL_PROMPT_MAX_BYTES) {
        return Err(format!(
            "initial prompt must be {INITIAL_PROMPT_MAX_BYTES} bytes or fewer"
        ));
    }
    Ok(())
}

pub(crate) fn reorder_agent_templates_from_settings(
    registry: &ProcessRegistry,
    ordered_ids: &[AgentTemplateId],
) -> Result<Vec<AgentTemplate>, String> {
    registry
        .store()
        .reorder_agent_templates(ordered_ids)
        .map_err(|error| error.to_string())?;
    load_agent_templates(registry)
}

pub(crate) fn delete_agent_template_from_settings(
    registry: &ProcessRegistry,
    agent_template_id: AgentTemplateId,
) -> Result<bool, String> {
    registry
        .store()
        .delete_agent_template(agent_template_id)
        .map_err(|error| error.to_string())
}

#[cfg(test)]
pub(crate) fn save_agent_tool(
    registry: &ProcessRegistry,
    id: Option<AgentToolId>,
    name: String,
    command: String,
    tool_type: String,
    enabled: bool,
) -> Result<AgentTool, String> {
    let name = name.trim();
    let command = command.trim();
    let tool_type = tool_type.trim();
    if name.is_empty() {
        return Err("agent tool name cannot be empty".to_owned());
    }
    if command.is_empty() {
        return Err("agent tool command cannot be empty".to_owned());
    }
    if command.contains('\0') {
        return Err("agent tool command may not contain NUL bytes".to_owned());
    }
    if tool_type.is_empty() {
        return Err("agent tool type cannot be empty".to_owned());
    }

    let id = match id {
        Some(id) => {
            let existing = registry
                .store()
                .get_agent_tool(id)
                .map_err(|error| error.to_string())?;
            match existing {
                None => return Err(format!("agent tool {id} was not found")),
                Some(tool) if tool.source == AgentToolSource::Config => {
                    return Err(format!(
                        "agent tool {id} is managed by the per-user config file"
                    ));
                }
                Some(_) => {}
            }
            id
        }
        None => registry
            .store()
            .next_agent_tool_id()
            .map_err(|error| error.to_string())?,
    };
    let tool = AgentTool {
        id,
        name: name.to_owned(),
        command: command.to_owned(),
        tool_type: tool_type.to_owned(),
        enabled,
        source: AgentToolSource::Local,
        resume_args: None,
        continue_args: None,
    };
    registry
        .store()
        .put_agent_tool(&tool)
        .map_err(|error| error.to_string())?;
    Ok(tool)
}

pub(crate) fn save_agent_tool_from_settings(
    registry: &ProcessRegistry,
    id: Option<AgentToolId>,
    name: String,
    command: String,
    tool_type: String,
    enabled: bool,
) -> Result<AgentTool, String> {
    let name = name.trim().to_owned();
    let command = command.trim().to_owned();
    let tool_type = tool_type.trim().to_owned();
    if name.is_empty() {
        return Err("agent tool name cannot be empty".to_owned());
    }
    if command.is_empty() {
        return Err("agent tool command cannot be empty".to_owned());
    }
    if command.contains('\0') {
        return Err("agent tool command may not contain NUL bytes".to_owned());
    }
    if tool_type.is_empty() {
        return Err("agent tool type cannot be empty".to_owned());
    }
    let existing = id
        .map(|id| {
            registry
                .store()
                .get_agent_tool(id)
                .map_err(|error| error.to_string())?
                .ok_or_else(|| format!("agent tool {id} was not found"))
        })
        .transpose()?;
    if registry
        .store()
        .list_agent_tools()
        .map_err(|error| error.to_string())?
        .iter()
        .any(|tool| Some(tool.id) != id && tool.name.eq_ignore_ascii_case(&name))
    {
        return Err(format!("agent tool name {name:?} is already registered"));
    }
    let tool = AgentTool {
        id: id.unwrap_or(
            registry
                .store()
                .next_agent_tool_id()
                .map_err(|error| error.to_string())?,
        ),
        name,
        command,
        tool_type,
        enabled,
        source: AgentToolSource::Config,
        resume_args: existing.as_ref().and_then(|tool| tool.resume_args.clone()),
        continue_args: existing
            .as_ref()
            .and_then(|tool| tool.continue_args.clone()),
    };
    registry
        .store()
        .put_agent_tool(&tool)
        .map_err(|error| error.to_string())?;
    Ok(tool)
}

pub(crate) fn reorder_agent_tools_from_settings(
    registry: &ProcessRegistry,
    ordered_ids: &[AgentToolId],
) -> Result<Vec<AgentTool>, String> {
    registry
        .store()
        .reorder_agent_tools(ordered_ids)
        .map_err(|error| error.to_string())?;
    registry
        .store()
        .list_agent_tools()
        .map_err(|error| error.to_string())
}

pub(crate) fn delete_agent_tool_from_settings(
    registry: &ProcessRegistry,
    agent_tool_id: AgentToolId,
) -> Result<bool, String> {
    reject_referenced_agent_tool(registry, agent_tool_id)?;
    registry
        .store()
        .delete_agent_tool(agent_tool_id)
        .map_err(|error| error.to_string())
}

#[cfg(test)]
pub(crate) fn delete_agent_tool(
    registry: &ProcessRegistry,
    agent_tool_id: AgentToolId,
) -> Result<bool, String> {
    if registry
        .store()
        .get_agent_tool(agent_tool_id)
        .map_err(|error| error.to_string())?
        .is_some_and(|tool| tool.source == AgentToolSource::Config)
    {
        return Err(format!(
            "agent tool {agent_tool_id} is managed by the per-user config file"
        ));
    }
    reject_referenced_agent_tool(registry, agent_tool_id)?;
    registry
        .store()
        .delete_agent_tool(agent_tool_id)
        .map_err(|error| error.to_string())
}

fn reject_referenced_agent_tool(
    registry: &ProcessRegistry,
    agent_tool_id: AgentToolId,
) -> Result<(), String> {
    let referenced = registry
        .store()
        .connection()
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM processes WHERE agent_tool_id = ?1)",
            [agent_tool_id],
            |row| row.get::<_, bool>(0),
        )
        .map_err(|error| error.to_string())?;
    if referenced {
        return Err(format!(
            "agent tool {agent_tool_id} is used by an existing agent; disable it instead"
        ));
    }
    Ok(())
}

pub(crate) fn load_agent_tool(
    registry: &ProcessRegistry,
    agent_tool_id: AgentToolId,
) -> Result<AgentTool, String> {
    registry
        .store()
        .get_agent_tool(agent_tool_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("agent tool {agent_tool_id} was not found"))
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn spawn_registered_agent(
    registry: crate::SharedProcessRegistry,
    project: Project,
    agent_tool_id: Option<AgentToolId>,
    agent_template_id: Option<AgentTemplateId>,
    name: Option<String>,
    extra_args: Vec<String>,
    initial_prompt: Option<String>,
    mcp_url: &str,
    auto_acknowledge_dialogs: bool,
    spawned_by_process_id: Option<ProcessId>,
) -> Result<SpawnResult, String> {
    validate_initial_prompt(initial_prompt.as_deref())?;
    let resolved = {
        let registry = registry.lock().await;
        resolve_agent_spawn(
            &registry,
            agent_tool_id,
            agent_template_id,
            extra_args,
            initial_prompt,
        )?
    };
    let result = spawn_registered_agent_for(
        registry.clone(),
        project,
        resolved.agent_tool_id,
        name,
        resolved.extra_args,
        mcp_url,
        auto_acknowledge_dialogs,
        spawned_by_process_id,
        AgentLaunchPurpose::Normal,
    )
    .await?;
    if let Some(prompt) = resolved.initial_prompt {
        schedule_initial_prompt(registry, result.process_id, prompt);
    }
    Ok(result)
}

fn resolve_agent_spawn(
    registry: &ProcessRegistry,
    requested_agent_tool_id: Option<AgentToolId>,
    agent_template_id: Option<AgentTemplateId>,
    caller_extra_args: Vec<String>,
    caller_prompt: Option<String>,
) -> Result<ResolvedAgentSpawn, String> {
    let Some(agent_template_id) = agent_template_id else {
        let agent_tool_id = requested_agent_tool_id.ok_or_else(|| {
            "agent_tool_id is required when no agent_template_id is provided".to_owned()
        })?;
        return Ok(ResolvedAgentSpawn {
            agent_tool_id,
            extra_args: caller_extra_args,
            initial_prompt: compose_initial_prompt(None, caller_prompt.as_deref()),
        });
    };
    let template = registry
        .store()
        .get_agent_template(agent_template_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| {
            format!("agent template {agent_template_id} was not found in the active profile")
        })?;
    if requested_agent_tool_id.is_some_and(|id| id != template.agent_tool_id) {
        return Err(format!(
            "agent template {agent_template_id} uses agent tool {}; conflicting agent_tool_id was provided",
            template.agent_tool_id
        ));
    }
    let mut extra_args = template.extra_args;
    extra_args.extend(caller_extra_args);
    Ok(ResolvedAgentSpawn {
        agent_tool_id: template.agent_tool_id,
        extra_args,
        initial_prompt: compose_initial_prompt(Some(&template.prompt), caller_prompt.as_deref()),
    })
}

pub(crate) fn compose_initial_prompt(
    template_prompt: Option<&str>,
    caller_prompt: Option<&str>,
) -> Option<String> {
    let template_prompt = template_prompt
        .map(str::trim)
        .filter(|prompt| !prompt.is_empty());
    let caller_prompt = caller_prompt
        .map(str::trim)
        .filter(|prompt| !prompt.is_empty());
    match (template_prompt, caller_prompt) {
        (Some(template), Some(caller)) => Some(format!("{template}\n\n{caller}")),
        (Some(template), None) => Some(template.to_owned()),
        (None, Some(caller)) => Some(caller.to_owned()),
        (None, None) => None,
    }
}

fn schedule_initial_prompt(
    registry: crate::SharedProcessRegistry,
    process_id: ProcessId,
    prompt: String,
) {
    tokio::spawn(async move {
        let started = Instant::now();
        let ready_deadline = started + INITIAL_PROMPT_READY_TIMEOUT;
        let hard_deadline = started + INITIAL_PROMPT_HARD_TIMEOUT;
        loop {
            let delivery = {
                let mut registry = registry.lock().await;
                let now = Instant::now();
                if now >= hard_deadline {
                    let _ = registry.record_process_event(
                        process_id,
                        "initial_prompt_dropped",
                        format!(
                            "initial prompt dropped (reason: hard_cap) after {}s without a safe delivery point",
                            INITIAL_PROMPT_HARD_TIMEOUT.as_secs()
                        ),
                    );
                    return;
                }
                let agent_state = match registry.get_status(process_id) {
                    Ok(status) => status.agent_state,
                    Err(error) => {
                        let _ = registry.record_process_event(
                            process_id,
                            "initial_prompt_dropped",
                            format!(
                                "initial prompt dropped (reason: submit_failed): status unavailable: {error}"
                            ),
                        );
                        return;
                    }
                };
                if agent_state.state == AttentionState::Exited {
                    let _ = registry.record_process_event(
                        process_id,
                        "initial_prompt_dropped",
                        "initial prompt dropped (reason: exited) before a safe delivery point",
                    );
                    return;
                }
                let dialog = match registry.pending_dialog(process_id) {
                    Ok(dialog) => dialog,
                    Err(error) => {
                        let _ = registry.record_process_event(
                            process_id,
                            "initial_prompt_dropped",
                            format!(
                                "initial prompt dropped (reason: submit_failed): dialog state unavailable: {error}"
                            ),
                        );
                        return;
                    }
                };
                if dialog.is_some() {
                    None
                } else {
                    let output_observed = agent_state.last_output_at.is_some();
                    let ready = matches!(
                        agent_state.state,
                        AttentionState::Idle | AttentionState::NeedsInput
                    ) && output_observed;
                    let quiet_fallback = now >= ready_deadline
                        && output_observed
                        && agent_state
                            .last_output_seconds
                            .is_some_and(|seconds| seconds >= DEFAULT_IDLE_AFTER.as_secs_f64());
                    (ready || quiet_fallback).then(|| {
                        let used_fallback = !ready;
                        let result = registry
                            .submit_input(process_id, prompt.as_bytes())
                            .map_err(|error| error.to_string());
                        match &result {
                            Ok(_) => {
                                let suffix = if used_fallback {
                                    " using the observed-output quiet fallback"
                                } else {
                                    ""
                                };
                                let _ = registry.record_process_event(
                                    process_id,
                                    "initial_prompt_delivered",
                                    format!("initial prompt delivered{suffix}"),
                                );
                            }
                            Err(error) => {
                                let _ = registry.record_process_event(
                                    process_id,
                                    "initial_prompt_dropped",
                                    format!(
                                        "initial prompt dropped (reason: submit_failed): {error}"
                                    ),
                                );
                            }
                        }
                        (result, used_fallback)
                    })
                }
            };
            if let Some((delivery, used_fallback)) = delivery {
                if used_fallback {
                    eprintln!(
                        "process {process_id}: initial prompt readiness was not detected within {}s; using fallback delivery",
                        INITIAL_PROMPT_READY_TIMEOUT.as_secs()
                    );
                }
                if let Err(error) = delivery {
                    eprintln!("process {process_id}: initial prompt delivery failed: {error}");
                }
                return;
            }
            tokio::time::sleep(INITIAL_PROMPT_POLL_INTERVAL).await;
        }
    });
}

#[allow(clippy::too_many_arguments)]
async fn spawn_registered_agent_for(
    registry: crate::SharedProcessRegistry,
    project: Project,
    agent_tool_id: AgentToolId,
    name: Option<String>,
    extra_args: Vec<String>,
    mcp_url: &str,
    auto_acknowledge_dialogs: bool,
    spawned_by_process_id: Option<ProcessId>,
    purpose: AgentLaunchPurpose,
) -> Result<SpawnResult, String> {
    let (tool, user_environment) = {
        let registry = registry.lock().await;
        (
            load_agent_tool(&registry, agent_tool_id)?,
            registry.user_environment_resolver().clone(),
        )
    };
    if !tool.enabled {
        return Err(format!(
            "agent tool {} ({}) is disabled",
            tool.id, tool.name
        ));
    }
    if tool.command.trim().is_empty() {
        return Err(format!(
            "agent tool {} ({}) has no command",
            tool.id, tool.name
        ));
    }
    let mcp_url = mcp_url.to_owned();
    let prepared = tokio::task::spawn_blocking(move || {
        let resolved_environment = user_environment.resolve();
        let source_home = agent_source_home(&resolved_environment, &tool.tool_type);
        let launch = prepare_agent_launch(
            &tool.command,
            &tool.tool_type,
            &mcp_url,
            &extra_args,
            purpose,
            source_home.as_deref(),
        )?;
        Ok::<_, String>(PreparedRegisteredAgent {
            tool,
            requested_name: name,
            launch,
        })
    })
    .await
    .map_err(|error| format!("agent launch preparation task failed: {error}"))??;
    let tool_type = prepared.tool.tool_type.clone();
    let ephemeral_home = prepared
        .launch
        .env
        .get(WORKMAN_EPHEMERAL_AGENT_HOME_ENV)
        .map(PathBuf::from);
    let result = {
        let mut locked = registry.lock().await;
        let current_tool = load_agent_tool(&locked, prepared.tool.id)?;
        if current_tool != prepared.tool {
            if let Some(home) = &ephemeral_home {
                let _ = fs::remove_dir_all(home);
            }
            return Err(format!(
                "agent tool {} changed while its launch was being prepared; retry the spawn",
                prepared.tool.id
            ));
        }
        let name = process_name(
            &locked,
            project.id,
            prepared.requested_name,
            &prepared.tool.name,
        )?;
        spawn(
            &mut locked,
            &project,
            ProcessKind::Agent,
            name,
            prepared.launch.command,
            Some(prepared.tool.id),
            Some(tool_type.clone()),
            prepared.launch.env,
            spawned_by_process_id,
        )
    };
    let result = match result {
        Ok(result) => result,
        Err(error) => {
            if let Some(home) = &ephemeral_home {
                let _ = fs::remove_dir_all(home);
            }
            return Err(error);
        }
    };
    if auto_acknowledge_dialogs
        && supports_first_run_dialog_ack(&tool_type)
        && let Err(error) =
            auto_acknowledge_initial_dialog(registry.clone(), result.process_id).await
    {
        eprintln!(
            "process {}: initial dialog auto-acknowledgment failed; continuing with the live process: {error}",
            result.process_id
        );
    }
    Ok(result)
}

#[derive(Debug, Serialize)]
pub(crate) struct AgentToolDeepCheckResult {
    pub agent_tool_id: AgentToolId,
    pub process_id: Option<ProcessId>,
    pub success: bool,
    pub elapsed_ms: u64,
    pub message: String,
}

pub(crate) async fn deep_check_registered_agent(
    registry: crate::SharedProcessRegistry,
    project_id: ProjectId,
    agent_tool_id: AgentToolId,
    mcp_url: &str,
    timeout_ms: Option<u64>,
    spawned_by_process_id: Option<ProcessId>,
) -> Result<AgentToolDeepCheckResult, String> {
    let started = Instant::now();
    let (tool, project, user_environment) = {
        let registry = registry.lock().await;
        let tool = load_agent_tool(&registry, agent_tool_id)?;
        let project = registry
            .store()
            .get_project(project_id)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| format!("project {project_id} was not found"))?;
        (tool, project, registry.resolved_user_environment())
    };
    let capability = mcp_launch_capability(&tool.tool_type);
    if !capability.supported {
        return Ok(AgentToolDeepCheckResult {
            agent_tool_id,
            process_id: None,
            success: false,
            elapsed_ms: elapsed_millis(started),
            message: format!(
                "Workman cannot deep-check this runtime: {}",
                capability.note
            ),
        });
    }
    let health = crate::runtime_doctor::check_agent_tools_with_user_environment(
        vec![tool.clone()],
        &user_environment,
    )
    .await;
    if !health
        .tools
        .first()
        .is_some_and(|health| health.found_on_path)
    {
        return Ok(AgentToolDeepCheckResult {
            agent_tool_id,
            process_id: None,
            success: false,
            elapsed_ms: elapsed_millis(started),
            message: "Runtime binary was not found on PATH; no process was spawned.".to_owned(),
        });
    }

    let prompt = "Use only the MCP server named workman. Call whoami once. When it identifies you, print exactly WORKMAN_DEEP_CHECK_OK and exit.";
    let normalized = normalize_tool_type(&tool.tool_type);
    let (extra_args, submit_prompt) = match normalized.as_str() {
        "claude" | "claude_code" => (
            vec![
                "--print".to_owned(),
                "--output-format".to_owned(),
                "text".to_owned(),
                prompt.to_owned(),
            ],
            false,
        ),
        "codex" => (
            vec![
                "exec".to_owned(),
                "--skip-git-repo-check".to_owned(),
                prompt.to_owned(),
            ],
            false,
        ),
        "gemini" | "gemini_cli" => (vec!["--prompt".to_owned(), prompt.to_owned()], false),
        "opencode" | "open_code" => (vec!["run".to_owned(), prompt.to_owned()], false),
        "grok" | "grok_cli" | "grok_build" => (
            vec![
                "--single".to_owned(),
                prompt.to_owned(),
                "--output-format".to_owned(),
                "plain".to_owned(),
            ],
            false,
        ),
        "kimi" | "kimi_code" => (
            vec![
                "--prompt".to_owned(),
                prompt.to_owned(),
                "--output-format".to_owned(),
                "text".to_owned(),
            ],
            false,
        ),
        _ => (Vec::new(), true),
    };
    let spawned = spawn_registered_agent_for(
        registry.clone(),
        project,
        agent_tool_id,
        None,
        extra_args,
        mcp_url,
        true,
        spawned_by_process_id,
        AgentLaunchPurpose::DeepCheck,
    )
    .await?;
    let process_id = {
        let mut registry = registry.lock().await;
        if submit_prompt
            && let Err(error) = registry.submit_input(spawned.process_id, prompt.as_bytes())
        {
            let _ = registry.close(spawned.process_id);
            return Err(error.to_string());
        }
        spawned.process_id
    };

    let deadline =
        Instant::now() + Duration::from_millis(timeout_ms.unwrap_or(30_000).clamp(1_000, 60_000));
    let mut success = false;
    let mut message = "The agent did not call whoami before the deep-check deadline.".to_owned();
    let mut last_output = String::new();
    loop {
        let (identified, output) = {
            let mut registry = registry.lock().await;
            let _ = registry.get_status(process_id);
            let identified = registry
                .store()
                .connection()
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM actors WHERE process_id = ?1)",
                    [process_id],
                    |row| row.get::<_, bool>(0),
                )
                .map_err(|error| error.to_string())?;
            let output = registry
                .rendered_output(process_id)
                .map(|output| output.text)
                .unwrap_or_default();
            (identified, output)
        };
        if !output.trim().is_empty() {
            last_output = output.clone();
        }
        if identified {
            success = true;
            message = if output.contains("WORKMAN_DEEP_CHECK_OK") {
                "Ephemeral agent called whoami through this daemon and confirmed the roundtrip."
                    .to_owned()
            } else {
                "Ephemeral agent called whoami through this daemon.".to_owned()
            };
            break;
        }
        if Instant::now() >= deadline {
            if !last_output.is_empty() {
                const MAX_DIAGNOSTIC_CHARS: usize = 1_000;
                let start = last_output
                    .char_indices()
                    .rev()
                    .nth(MAX_DIAGNOSTIC_CHARS)
                    .map_or(0, |(index, _)| index);
                message.push_str(" Last terminal output: ");
                message.push_str(last_output[start..].trim());
            }
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    {
        let mut registry = registry.lock().await;
        let _ = registry.close(process_id);
    }
    Ok(AgentToolDeepCheckResult {
        agent_tool_id,
        process_id: Some(process_id),
        success,
        elapsed_ms: elapsed_millis(started),
        message,
    })
}

fn elapsed_millis(started: Instant) -> u64 {
    started.elapsed().as_millis().try_into().unwrap_or(u64::MAX)
}

fn supports_first_run_dialog_ack(tool_type: &str) -> bool {
    matches!(
        normalize_tool_type(tool_type).as_str(),
        "claude" | "claude_code" | "codex"
    )
}

async fn auto_acknowledge_initial_dialog(
    registry: crate::SharedProcessRegistry,
    process_id: ProcessId,
) -> Result<(), String> {
    let started = Instant::now();
    let deadline = started + INITIAL_DIALOG_TIMEOUT;
    let mut last_total_bytes = 0_u64;
    let mut last_output_change = started;

    while Instant::now() < deadline {
        let dialog = {
            let mut registry = registry.lock().await;
            registry
                .pending_dialog(process_id)
                .map_err(|error| error.to_string())?
        };
        if let Some(dialog) = dialog {
            if !dialog.known_first_run {
                return Ok(());
            }
            {
                let mut registry = registry.lock().await;
                registry
                    .acknowledge_known_dialog(process_id)
                    .map_err(|error| error.to_string())?;
            }
            let clear_deadline = Instant::now() + DIALOG_CLEAR_TIMEOUT;
            while Instant::now() < clear_deadline {
                let cleared = {
                    let mut registry = registry.lock().await;
                    registry
                        .pending_dialog(process_id)
                        .map_err(|error| error.to_string())?
                        .is_none()
                };
                if cleared {
                    return Ok(());
                }
                tokio::time::sleep(DIALOG_POLL_INTERVAL).await;
            }
            return Err(format!(
                "process {process_id} did not clear its acknowledged first-run dialog"
            ));
        }

        let raw = {
            let mut registry = registry.lock().await;
            registry
                .raw_output(process_id, None, 0)
                .map_err(|error| error.to_string())?
        };
        if raw.total_bytes != last_total_bytes {
            last_total_bytes = raw.total_bytes;
            last_output_change = Instant::now();
        }
        if last_total_bytes > 0
            && started.elapsed() >= INITIAL_OUTPUT_SETTLE
            && last_output_change.elapsed() >= INITIAL_OUTPUT_QUIET
        {
            return Ok(());
        }
        tokio::time::sleep(DIALOG_POLL_INTERVAL).await;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn spawn(
    registry: &mut ProcessRegistry,
    project: &Project,
    kind: ProcessKind,
    name: String,
    command: String,
    agent_tool_id: Option<AgentToolId>,
    agent_tool_type: Option<String>,
    env: BTreeMap<String, String>,
    spawned_by_process_id: Option<ProcessId>,
) -> Result<SpawnResult, String> {
    let created = registry
        .create(Process {
            id: 0,
            project_id: project.id,
            kind,
            name,
            command: Some(command),
            working_dir: project.path.clone(),
            env,
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
            agent_tool_id,
            spawned_by_process_id,
            sort_order: 0,
        })
        .map_err(|error| error.to_string())?;
    let running = match registry.start(created.id) {
        Ok(process) => process,
        Err(error) => {
            let _ = registry.close(created.id);
            return Err(error.to_string());
        }
    };
    let agent_instructions = (kind == ProcessKind::Agent).then(|| {
        agent_instructions(
            &running,
            project,
            running
                .env
                .get(WORKMAN_MCP_URL_ENV)
                .expect("agent spawn always records its MCP URL"),
            agent_tool_type.as_deref().unwrap_or("unknown"),
        )
    });
    Ok(SpawnResult {
        process_id: running.id,
        project_id: running.project_id,
        name: running.name,
        kind: running.kind,
        agent_instructions,
    })
}

fn process_name(
    registry: &ProcessRegistry,
    project_id: ProjectId,
    requested: Option<String>,
    fallback: &str,
) -> Result<String, String> {
    if let Some(name) = requested {
        return Ok(name);
    }
    let slug = fallback
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_owned();
    let process_id = registry
        .store()
        .next_process_id()
        .map_err(|error| error.to_string())?;
    let stem = if slug.is_empty() { "process" } else { &slug };
    let existing_names = registry
        .store()
        .list_processes(Some(project_id))
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|process| process.name)
        .collect::<Vec<_>>();
    let base = format!("{stem}--{process_id}");
    let mut candidate = base.clone();
    let mut suffix = 2_u64;
    while existing_names.iter().any(|name| name == &candidate) {
        candidate = format!("{base}-{suffix}");
        suffix = suffix.saturating_add(1);
    }
    Ok(candidate)
}

fn command_with_args(command: &str, extra_args: &[String]) -> Result<String, String> {
    if command.contains('\0') || extra_args.iter().any(|arg| arg.contains('\0')) {
        return Err("commands and arguments may not contain NUL bytes".to_owned());
    }
    if extra_args.is_empty() {
        return Ok(command.to_owned());
    }
    let args = extra_args
        .iter()
        .map(|argument| shell_quote(argument))
        .collect::<Vec<_>>()
        .join(" ");
    Ok(format!("{command} {args}"))
}

fn prepare_agent_launch(
    command: &str,
    tool_type: &str,
    mcp_url: &str,
    extra_args: &[String],
    purpose: AgentLaunchPurpose,
    source_home: Option<&Path>,
) -> Result<AgentLaunchPlan, String> {
    let adapter = mcp_launch_adapter(tool_type);
    let mut env = BTreeMap::from([(WORKMAN_MCP_URL_ENV.to_owned(), mcp_url.to_owned())]);
    let command = match adapter {
        McpLaunchAdapter::Claude | McpLaunchAdapter::Codex => {
            let mut launch_args = mcp_launch_args(adapter, mcp_url, purpose);
            launch_args.extend(extra_args.iter().cloned());
            command_with_args(command, &launch_args)?
        }
        McpLaunchAdapter::Gemini => {
            let command = command_with_args(command, extra_args)?;
            gemini_command_with_ephemeral_settings(&command, mcp_url)
        }
        McpLaunchAdapter::OpenCode => {
            env.insert(
                OPENCODE_CONFIG_CONTENT_ENV.to_owned(),
                opencode_inline_config(mcp_url),
            );
            command_with_args(command, extra_args)?
        }
        McpLaunchAdapter::Grok => {
            let command = command_with_args(command, extra_args)?;
            let grok_home = prepare_private_agent_home(
                "grok",
                source_home,
                "config.toml",
                "config.toml",
                &grok_config(source_home, mcp_url)?,
            )?;
            env.insert(
                "GROK_HOME".to_owned(),
                grok_home.to_string_lossy().into_owned(),
            );
            env.insert(
                WORKMAN_EPHEMERAL_AGENT_HOME_ENV.to_owned(),
                grok_home.to_string_lossy().into_owned(),
            );
            command
        }
        McpLaunchAdapter::Kimi => {
            // Kimi 0.34 rejects --prompt with --yolo/--auto. Those policy flags remain intact for
            // normal launches; prompt-mode deep checks are already non-interactive and narrowly
            // constrained to whoami.
            let command = if purpose == AgentLaunchPurpose::DeepCheck {
                kimi_deep_check_command(command, extra_args)?
            } else {
                command_with_args(command, extra_args)?
            };
            let kimi_home = prepare_private_agent_home(
                "kimi",
                source_home,
                "mcp.json",
                KIMI_MCP_TEMPLATE_FILE,
                &kimi_mcp_template(mcp_url),
            )?;
            let command = kimi_command_with_materialized_token(&command, &kimi_home);
            env.insert(
                "KIMI_CODE_HOME".to_owned(),
                kimi_home.to_string_lossy().into_owned(),
            );
            env.insert(
                WORKMAN_EPHEMERAL_AGENT_HOME_ENV.to_owned(),
                kimi_home.to_string_lossy().into_owned(),
            );
            command
        }
        McpLaunchAdapter::Unsupported => command_with_args(command, extra_args)?,
    };
    Ok(AgentLaunchPlan { command, env })
}

pub(crate) fn mcp_launch_capability(tool_type: &str) -> McpLaunchCapability {
    mcp_launch_adapter(tool_type).capability()
}

fn mcp_launch_adapter(tool_type: &str) -> McpLaunchAdapter {
    match normalize_tool_type(tool_type).as_str() {
        "claude" | "claude_code" => McpLaunchAdapter::Claude,
        "codex" => McpLaunchAdapter::Codex,
        "gemini" | "gemini_cli" => McpLaunchAdapter::Gemini,
        "opencode" | "open_code" => McpLaunchAdapter::OpenCode,
        "grok" | "grok_cli" | "grok_build" => McpLaunchAdapter::Grok,
        "kimi" | "kimi_code" => McpLaunchAdapter::Kimi,
        _ => McpLaunchAdapter::Unsupported,
    }
}

fn agent_source_home(
    user_environment: &crate::ResolvedUserEnvironment,
    tool_type: &str,
) -> Option<PathBuf> {
    let (override_name, default_directory) = match mcp_launch_adapter(tool_type) {
        McpLaunchAdapter::Grok => ("GROK_HOME", ".grok"),
        McpLaunchAdapter::Kimi => ("KIMI_CODE_HOME", ".kimi-code"),
        _ => return None,
    };
    let environment = user_environment.command_environment();
    environment
        .get(std::ffi::OsStr::new(override_name))
        .map(PathBuf::from)
        .or_else(|| {
            environment
                .get(std::ffi::OsStr::new("HOME"))
                .map(PathBuf::from)
                .map(|home| home.join(default_directory))
        })
}

fn kimi_mcp_template(mcp_url: &str) -> String {
    format!(
        "{}\n",
        json!({
            "mcpServers": {
                "workman": {
                    "url": mcp_url,
                    "headers": {
                        "x-workman-mcp-token": KIMI_MCP_TOKEN_SENTINEL
                    }
                }
            }
        })
    )
}

fn kimi_deep_check_command(command: &str, extra_args: &[String]) -> Result<String, String> {
    let mut words = shell_words::split(command)
        .map_err(|error| format!("parse Kimi command for deep check: {error}"))?;
    let executable = words
        .first()
        .and_then(|word| Path::new(word).file_name())
        .and_then(|name| name.to_str());
    if !matches!(executable, Some("kimi" | "kimi-code"))
        || words.iter().any(|word| {
            matches!(
                word.as_str(),
                ";" | "&&" | "||" | "|" | "&" | ">" | ">>" | "<"
            )
        })
    {
        return Err(
            "Kimi deep checks require a direct kimi command so prompt-mode flags can be applied safely"
                .to_owned(),
        );
    }
    words.retain(|word| !matches!(word.as_str(), "-y" | "--yolo" | "--auto"));
    let command = words
        .iter()
        .map(|word| shell_quote(word))
        .collect::<Vec<_>>()
        .join(" ");
    command_with_args(&command, extra_args)
}

fn kimi_command_with_materialized_token(command: &str, home: &Path) -> String {
    let template = shell_quote(&home.join(KIMI_MCP_TEMPLATE_FILE).to_string_lossy());
    let config = shell_quote(&home.join("mcp.json").to_string_lossy());
    format!(
        "umask 077; [ -n \"$WORKMAN_MCP_TOKEN\" ] || exit 1; \
         IFS= read -r workman_kimi_mcp_template < {template} || exit 1; \
         case \"$workman_kimi_mcp_template\" in *{KIMI_MCP_TOKEN_SENTINEL}*) ;; *) exit 1 ;; esac; \
         workman_kimi_mcp_prefix=${{workman_kimi_mcp_template%%{KIMI_MCP_TOKEN_SENTINEL}*}}; \
         workman_kimi_mcp_suffix=${{workman_kimi_mcp_template#*{KIMI_MCP_TOKEN_SENTINEL}}}; \
         printf '%s%s%s\\n' \"$workman_kimi_mcp_prefix\" \"$WORKMAN_MCP_TOKEN\" \"$workman_kimi_mcp_suffix\" > {config} || exit 1; \
         {command}"
    )
}

fn grok_config(source_home: Option<&Path>, mcp_url: &str) -> Result<String, String> {
    let source = source_home.map(|home| home.join("config.toml"));
    let contents = match source.as_deref().map(fs::read_to_string).transpose() {
        Ok(Some(contents)) => contents,
        Ok(None) => String::new(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => {
            return Err(format!(
                "could not read Grok config {}: {error}",
                source
                    .as_deref()
                    .unwrap_or_else(|| Path::new("config.toml"))
                    .display()
            ));
        }
    };
    let mut document = contents
        .parse::<DocumentMut>()
        .map_err(|error| format!("could not parse Grok config.toml: {error}"))?;
    if !document.contains_key("mcp_servers") {
        document["mcp_servers"] = Item::Table(Table::new());
    }
    let servers = document["mcp_servers"]
        .as_table_mut()
        .ok_or_else(|| "Grok config [mcp_servers] must be a table".to_owned())?;
    let mut workman = Table::new();
    workman["url"] = Item::Value(TomlValue::from(mcp_url));
    workman["enabled"] = Item::Value(TomlValue::from(true));
    let mut headers = toml_edit::InlineTable::new();
    headers.insert(
        "x-workman-mcp-token",
        TomlValue::from("${WORKMAN_MCP_TOKEN}"),
    );
    workman["headers"] = Item::Value(TomlValue::InlineTable(headers));
    servers["workman"] = Item::Table(workman);
    Ok(document.to_string())
}

fn prepare_private_agent_home(
    prefix: &str,
    source_home: Option<&Path>,
    replaced_file: &str,
    replacement_file: &str,
    replacement: &str,
) -> Result<PathBuf, String> {
    let home = env::temp_dir().join(format!("workman-{prefix}-mcp.{}", Uuid::new_v4().simple()));
    fs::create_dir(&home)
        .map_err(|error| format!("create private {prefix} config home: {error}"))?;
    let prepared = (|| -> Result<(), String> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&home, fs::Permissions::from_mode(0o700))
                .map_err(|error| format!("secure private {prefix} config home: {error}"))?;
        }
        if let Some(source_home) = source_home {
            let entries = match fs::read_dir(source_home) {
                Ok(entries) => Some(entries),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => {
                    return Err(format!(
                        "read {prefix} config home {}: {error}",
                        source_home.display()
                    ));
                }
            };
            for entry in entries.into_iter().flatten() {
                let entry = entry.map_err(|error| {
                    format!(
                        "read {prefix} config home {}: {error}",
                        source_home.display()
                    )
                })?;
                if entry.file_name() == replaced_file
                    || entry.file_name() == replacement_file
                    || entry.file_name() == "leader.sock"
                {
                    continue;
                }
                let target = home.join(entry.file_name());
                #[cfg(unix)]
                std::os::unix::fs::symlink(entry.path(), &target).map_err(|error| {
                    format!(
                        "seed private {prefix} config home from {}: {error}",
                        entry.path().display()
                    )
                })?;
                #[cfg(not(unix))]
                if entry.path().is_file() {
                    fs::copy(entry.path(), &target).map_err(|error| {
                        format!(
                            "seed private {prefix} config home from {}: {error}",
                            entry.path().display()
                        )
                    })?;
                }
            }
        }
        let path = home.join(replacement_file);
        fs::write(&path, replacement).map_err(|error| {
            format!("write private {prefix} config {}: {error}", path.display())
        })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
                .map_err(|error| format!("secure private {prefix} config: {error}"))?;
        }
        Ok(())
    })();
    if let Err(error) = prepared {
        let _ = fs::remove_dir_all(&home);
        return Err(error);
    }
    Ok(home)
}

fn mcp_launch_args(
    adapter: McpLaunchAdapter,
    mcp_url: &str,
    purpose: AgentLaunchPurpose,
) -> Vec<String> {
    // Every launch gets the process-scoped connector. Authorization narrowing belongs only to
    // the fixed whoami deep check; a normal launch must honor the registered command's policy.
    match adapter {
        McpLaunchAdapter::Claude => {
            let mut args = vec![
                "--mcp-config".to_owned(),
                json!({
                    "mcpServers": {
                        "workman": {
                            "type": "http",
                            "url": mcp_url,
                            "headers": {
                                "x-workman-mcp-token": "${WORKMAN_MCP_TOKEN}"
                            }
                        }
                    }
                })
                .to_string(),
                "--strict-mcp-config".to_owned(),
            ];
            if purpose == AgentLaunchPurpose::DeepCheck {
                args.extend([
                    "--allowedTools".to_owned(),
                    "mcp__workman__whoami".to_owned(),
                ]);
            }
            args
        }
        McpLaunchAdapter::Codex => {
            let mut args = vec![
                "-c".to_owned(),
                format!(
                    "mcp_servers.workman.url={}",
                    serde_json::to_string(mcp_url).expect("MCP URL serializes as a TOML string")
                ),
                "-c".to_owned(),
                "mcp_servers.workman.env_http_headers={\"x-workman-mcp-token\"=\"WORKMAN_MCP_TOKEN\"}"
                    .to_owned(),
            ];
            if purpose == AgentLaunchPurpose::DeepCheck {
                args.extend([
                    "-c".to_owned(),
                    "mcp_servers.workman.tools.whoami.approval_mode=\"approve\"".to_owned(),
                ]);
            }
            args
        }
        _ => Vec::new(),
    }
}

fn gemini_command_with_ephemeral_settings(command: &str, mcp_url: &str) -> String {
    let settings = json!({
        "mcp": { "allowed": ["workman"] },
        "mcpServers": {
            "workman": {
                "httpUrl": mcp_url,
                "headers": {
                    "x-workman-mcp-token": "$WORKMAN_MCP_TOKEN"
                }
            }
        }
    })
    .to_string();
    format!(
        "umask 077; workman_mcp_config_dir=$(mktemp -d \"${{TMPDIR:-/tmp}}/workman-gemini-mcp.XXXXXX\") || exit 1; \
         workman_mcp_config_file=\"$workman_mcp_config_dir/settings.json\"; \
         trap 'rm -f -- \"$workman_mcp_config_file\"; rmdir -- \"$workman_mcp_config_dir\"' EXIT; \
         printf '%s\\n' {} > \"$workman_mcp_config_file\" || exit 1; \
         GEMINI_CLI_SYSTEM_SETTINGS_PATH=\"$workman_mcp_config_file\" {command}",
        shell_quote(&settings)
    )
}

fn opencode_inline_config(mcp_url: &str) -> String {
    json!({
        "mcp": {
            "workman": {
                "type": "remote",
                "url": mcp_url,
                "oauth": false,
                "headers": {
                    "x-workman-mcp-token": "{env:WORKMAN_MCP_TOKEN}"
                }
            }
        }
    })
    .to_string()
}

fn normalize_tool_type(tool_type: &str) -> String {
    tool_type
        .trim()
        .to_ascii_lowercase()
        .replace(['-', ' '], "_")
}

fn shell_quote(argument: &str) -> String {
    if !argument.is_empty()
        && argument.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'_' | b'@' | b'%' | b'+' | b'=' | b':' | b',' | b'.' | b'/' | b'-'
                )
        })
    {
        return argument.to_owned();
    }
    format!("'{}'", argument.replace('\'', "'\"'\"'"))
}

fn agent_instructions(
    process: &Process,
    project: &Project,
    mcp_url: &str,
    tool_type: &str,
) -> String {
    let capability = mcp_launch_capability(tool_type);
    let client_wiring = if capability.supported {
        format!(
            "This launch already has the server named workman wired through {}.",
            capability.mechanism
        )
    } else {
        format!(
            "This runtime is not auto-wired: {} Do not claim Workman MCP access unless the client exposes it.",
            capability.note
        )
    };
    let identity_guidance = if capability.supported {
        format!(
            "Call whoami() through workman first to confirm that you auto-identify as process {}. Use identify_session(process_id={}) only if whoami cannot identify you.",
            process.id, process.id
        )
    } else {
        "The Workman MCP identity check is unavailable for this launch.".to_owned()
    };
    format!(
        "[workman context] You are Workman process ID {process_id} ({process_name}), in project \
         {project_id} ({project_name}, repo {project_path}). Workman set \
         WORKMAN_PROCESS_ID={process_id}, WORKMAN_MCP_URL={mcp_url}, and the secret \
         WORKMAN_MCP_TOKEN environment variable. {client_wiring} The connector must use the exact \
         URL in ${{WORKMAN_MCP_URL}} ({mcp_url}) and send the x-workman-mcp-token header from \
         ${{WORKMAN_MCP_TOKEN}}. Use the MCP server named workman, never a globally configured Solo \
         or unrelated workman server. {identity_guidance} \
         {worktree_agent_guidance} \
         {scratchpad_handoff_guidance} \
         [END WORKMAN CONTEXT]",
        process_id = process.id,
        process_name = process.name,
        project_id = project.id,
        project_name = project.name,
        project_path = project.path,
        client_wiring = client_wiring,
        identity_guidance = identity_guidance,
        mcp_url = mcp_url,
        scratchpad_handoff_guidance = SCRATCHPAD_HANDOFF_GUIDANCE,
        worktree_agent_guidance = WORKTREE_AGENT_GUIDANCE,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use workman_core::Store;

    #[test]
    fn dialog_acknowledgment_defaults_on_and_can_be_disabled() {
        let defaulted: SpawnAgentArgs = serde_json::from_value(json!({
            "agent_tool_id": 1
        }))
        .unwrap();
        assert!(defaulted.auto_acknowledge_dialogs);

        let disabled: SpawnAgentArgs = serde_json::from_value(json!({
            "agent_tool_id": 1,
            "auto_acknowledge_dialogs": false
        }))
        .unwrap();
        assert!(!disabled.auto_acknowledge_dialogs);
        assert!(supports_first_run_dialog_ack("claude-code"));
        assert!(supports_first_run_dialog_ack("codex"));
        assert!(!supports_first_run_dialog_ack("custom"));
    }

    #[test]
    fn initial_prompt_composition_trims_edges_and_adds_one_blank_line() {
        assert_eq!(
            compose_initial_prompt(
                Some("  Keep the change focused.\n"),
                Some("\nImplement the dialog.  ")
            ),
            Some("Keep the change focused.\n\nImplement the dialog.".into())
        );
        assert_eq!(
            compose_initial_prompt(Some(" Template only "), Some("  ")),
            Some("Template only".into())
        );
        assert_eq!(
            compose_initial_prompt(None, Some(" Prompt only ")),
            Some("Prompt only".into())
        );
        assert_eq!(compose_initial_prompt(Some("\n"), None), None);
    }

    #[test]
    fn agent_template_and_initial_prompt_limits_accept_boundaries_and_reject_excess() {
        let boundary_args = vec!["x".repeat(64); AGENT_TEMPLATE_EXTRA_ARGS_MAX_ITEMS];
        assert_eq!(
            boundary_args.iter().map(String::len).sum::<usize>(),
            AGENT_TEMPLATE_EXTRA_ARGS_MAX_BYTES
        );
        assert!(
            validate_agent_template_fields(
                &"n".repeat(AGENT_TEMPLATE_NAME_MAX_CHARS),
                &"p".repeat(AGENT_TEMPLATE_PROMPT_MAX_BYTES),
                &boundary_args,
            )
            .is_ok()
        );
        assert!(validate_initial_prompt(Some(&"p".repeat(INITIAL_PROMPT_MAX_BYTES))).is_ok());

        assert_eq!(
            validate_agent_template_fields(
                &"n".repeat(AGENT_TEMPLATE_NAME_MAX_CHARS + 1),
                "",
                &[],
            )
            .unwrap_err(),
            "agent template name must be 120 characters or fewer"
        );
        assert_eq!(
            validate_agent_template_fields(
                "name",
                &"p".repeat(AGENT_TEMPLATE_PROMPT_MAX_BYTES + 1),
                &[],
            )
            .unwrap_err(),
            "agent template prompt must be 65536 bytes or fewer"
        );
        assert_eq!(
            validate_agent_template_fields(
                "name",
                "",
                &vec![String::new(); AGENT_TEMPLATE_EXTRA_ARGS_MAX_ITEMS + 1],
            )
            .unwrap_err(),
            "agent template may have at most 64 arguments"
        );
        assert_eq!(
            validate_agent_template_fields(
                "name",
                "",
                &["x".repeat(AGENT_TEMPLATE_EXTRA_ARGS_MAX_BYTES + 1)],
            )
            .unwrap_err(),
            "agent template arguments must total 4096 bytes or fewer"
        );
        assert_eq!(
            validate_initial_prompt(Some(&"p".repeat(INITIAL_PROMPT_MAX_BYTES + 1))).unwrap_err(),
            "initial prompt must be 65536 bytes or fewer"
        );
    }

    #[test]
    fn agent_template_fields_reject_nul_bytes() {
        assert!(validate_agent_template_fields("bad\0name", "", &[]).is_err());
        assert!(validate_agent_template_fields("name", "bad\0prompt", &[]).is_err());
        assert!(validate_agent_template_fields("name", "", &["bad\0argument".into()]).is_err());
    }

    #[test]
    fn migration_presets_and_custom_commands_are_listed_together() {
        let registry = ProcessRegistry::new(Store::open_in_memory().unwrap()).unwrap();
        registry
            .store()
            .put_agent_tool(&AgentTool {
                id: 99,
                name: "Scripted test agent".into(),
                command: "/tmp/fake-agent".into(),
                tool_type: "claude_code".into(),
                enabled: true,
                source: AgentToolSource::Local,
                resume_args: None,
                continue_args: None,
            })
            .unwrap();
        let tools = load_agent_tools(&registry).unwrap();
        assert_eq!(tools.len(), 8);
        assert!(
            tools
                .iter()
                .any(|tool| tool.command == "claude --dangerously-skip-permissions")
        );
        assert!(tools.iter().any(|tool| tool.command == "/tmp/fake-agent"));
        assert!(
            tools
                .iter()
                .any(|tool| tool.command == "grok --always-approve")
        );
    }

    #[test]
    fn config_managed_tools_are_read_only_through_registry_mutations() {
        let registry = ProcessRegistry::new(Store::open_in_memory().unwrap()).unwrap();
        registry
            .store()
            .put_agent_tool(&AgentTool {
                id: 99,
                name: "Configured".into(),
                command: "configured-agent".into(),
                tool_type: "future".into(),
                enabled: true,
                source: AgentToolSource::Config,
                resume_args: None,
                continue_args: None,
            })
            .unwrap();

        assert!(
            save_agent_tool(
                &registry,
                Some(99),
                "Configured".into(),
                "changed".into(),
                "future".into(),
                false,
            )
            .unwrap_err()
            .contains("managed by the per-user config file")
        );
        assert!(
            delete_agent_tool(&registry, 99)
                .unwrap_err()
                .contains("managed by the per-user config file")
        );
    }

    #[test]
    fn shell_quotes_each_extra_argument_without_reinterpreting_it() {
        let command = command_with_args(
            "claude --flag",
            &[
                "plain".into(),
                "two words".into(),
                "don't".into(),
                "".into(),
            ],
        )
        .unwrap();
        assert_eq!(command, "claude --flag plain 'two words' 'don'\"'\"'t' ''");
    }

    #[test]
    fn claude_launch_uses_only_the_per_process_workman_connector() {
        let launch = prepare_agent_launch(
            "claude --dangerously-skip-permissions",
            "claude_code",
            "http://127.0.0.1:43123/mcp",
            &["--model".into(), "opus".into()],
            AgentLaunchPurpose::Normal,
            None,
        )
        .unwrap();
        let command = launch.command;
        assert!(command.contains("--mcp-config"));
        assert!(command.contains("--strict-mcp-config"));
        assert!(command.contains("http://127.0.0.1:43123/mcp"));
        assert!(command.contains("x-workman-mcp-token"));
        assert!(command.contains("${WORKMAN_MCP_TOKEN}"));
        assert!(!command.contains("--allowedTools"));
        assert!(command.ends_with("--model opus"));

        let deep_check = prepare_agent_launch(
            "claude --dangerously-skip-permissions",
            "claude_code",
            "http://127.0.0.1:43123/mcp",
            &[],
            AgentLaunchPurpose::DeepCheck,
            None,
        )
        .unwrap();
        assert!(
            deep_check
                .command
                .contains("--allowedTools mcp__workman__whoami")
        );
    }

    #[test]
    fn codex_launch_overrides_workman_url_and_process_token_header() {
        let launch = prepare_agent_launch(
            "codex --dangerously-bypass-approvals-and-sandbox",
            "codex",
            "http://127.0.0.1:43124/mcp",
            &["--model".into(), "gpt-test".into()],
            AgentLaunchPurpose::Normal,
            None,
        )
        .unwrap();
        let command = launch.command;
        assert!(command.contains("mcp_servers.workman.url="));
        assert!(command.contains("http://127.0.0.1:43124/mcp"));
        assert!(command.contains("mcp_servers.workman.env_http_headers="));
        assert!(command.contains("WORKMAN_MCP_TOKEN"));
        assert!(!command.contains("approval_mode"));
        assert!(command.ends_with("--model gpt-test"));

        let deep_check = prepare_agent_launch(
            "codex --dangerously-bypass-approvals-and-sandbox",
            "codex",
            "http://127.0.0.1:43124/mcp",
            &[],
            AgentLaunchPurpose::DeepCheck,
            None,
        )
        .unwrap();
        assert!(
            deep_check
                .command
                .contains("mcp_servers.workman.tools.whoami.approval_mode=\"approve\"")
        );
    }

    #[test]
    fn gemini_launch_uses_an_ephemeral_system_settings_file() {
        let launch = prepare_agent_launch(
            "gemini --approval-mode=yolo",
            "gemini",
            "http://127.0.0.1:43125/mcp",
            &["--model".into(), "gemini-test".into()],
            AgentLaunchPurpose::Normal,
            None,
        )
        .unwrap();
        assert!(launch.command.contains("GEMINI_CLI_SYSTEM_SETTINGS_PATH"));
        assert!(launch.command.contains("mktemp -d"));
        assert!(launch.command.contains("http://127.0.0.1:43125/mcp"));
        assert!(launch.command.contains("x-workman-mcp-token"));
        assert!(launch.command.contains("$WORKMAN_MCP_TOKEN"));
        assert!(
            launch
                .command
                .contains("gemini --approval-mode=yolo --model gemini-test")
        );
        assert!(!launch.command.contains(".gemini/settings.json"));
    }

    #[test]
    fn opencode_and_model_variants_use_inline_runtime_config() {
        let launch = prepare_agent_launch(
            "opencode --auto --model deepseek/deepseek-v4-flash",
            "opencode",
            "http://127.0.0.1:43126/mcp",
            &["run".into(), "call workman".into()],
            AgentLaunchPurpose::Normal,
            None,
        )
        .unwrap();
        assert_eq!(
            launch.command,
            "opencode --auto --model deepseek/deepseek-v4-flash run 'call workman'"
        );
        let config: serde_json::Value = serde_json::from_str(
            launch
                .env
                .get(OPENCODE_CONFIG_CONTENT_ENV)
                .expect("inline OpenCode config"),
        )
        .unwrap();
        assert_eq!(
            config["mcp"]["workman"]["url"],
            "http://127.0.0.1:43126/mcp"
        );
        assert_eq!(
            config["mcp"]["workman"]["headers"]["x-workman-mcp-token"],
            "{env:WORKMAN_MCP_TOKEN}"
        );
    }

    #[test]
    fn grok_launch_uses_a_private_merged_config_home() {
        let source = tempfile::tempdir().unwrap();
        let source_config =
            "[ui]\nscreen_mode = \"minimal\"\n\n[mcp_servers.old]\nurl = \"http://old.test/mcp\"\n";
        fs::write(source.path().join("config.toml"), source_config).unwrap();
        fs::write(source.path().join("auth.json"), "fixture-auth\n").unwrap();

        let launch = prepare_agent_launch(
            "grok --always-approve",
            "grok_build",
            "http://127.0.0.1:43127/mcp",
            &["--model".into(), "grok-test".into()],
            AgentLaunchPurpose::Normal,
            Some(source.path()),
        )
        .unwrap();
        assert_eq!(launch.command, "grok --always-approve --model grok-test");
        let home = PathBuf::from(launch.env.get("GROK_HOME").unwrap());
        assert_eq!(
            launch.env.get(WORKMAN_EPHEMERAL_AGENT_HOME_ENV),
            launch.env.get("GROK_HOME")
        );
        let config = fs::read_to_string(home.join("config.toml")).unwrap();
        assert!(config.contains("screen_mode = \"minimal\""));
        assert!(config.contains("http://old.test/mcp"));
        assert!(config.contains("http://127.0.0.1:43127/mcp"));
        assert!(config.contains("x-workman-mcp-token"));
        assert!(config.contains("${WORKMAN_MCP_TOKEN}"));
        assert_eq!(
            fs::read_to_string(home.join("auth.json")).unwrap(),
            "fixture-auth\n"
        );
        assert_eq!(
            fs::read_to_string(source.path().join("config.toml")).unwrap(),
            source_config
        );
        let capability = mcp_launch_capability("grok-cli");
        assert!(capability.supported);
        assert_eq!(capability.mechanism, "private per-launch GROK_HOME config");
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn kimi_launch_uses_a_private_home_and_materializes_the_process_token() {
        let deep_check = kimi_deep_check_command(
            "kimi --yolo --model 'kimi test'",
            &["--prompt".into(), "check".into()],
        )
        .unwrap();
        assert!(!deep_check.contains("--yolo"));
        assert!(deep_check.contains("--model 'kimi test'"));
        assert!(deep_check.ends_with("--prompt check"));
        assert!(kimi_deep_check_command("env kimi --yolo", &[]).is_err());

        let source = tempfile::tempdir().unwrap();
        let source_mcp = "{\"mcpServers\":{\"existing\":{\"url\":\"http://old.test/mcp\"}}}\n";
        fs::write(source.path().join("mcp.json"), source_mcp).unwrap();
        fs::write(
            source.path().join("config.toml"),
            "default_model = \"fixture\"\n",
        )
        .unwrap();

        let launch = prepare_agent_launch(
            "true",
            "kimi",
            "http://127.0.0.1:43127/mcp",
            &["--model".into(), "kimi-test".into()],
            AgentLaunchPurpose::Normal,
            Some(source.path()),
        )
        .unwrap();
        let home = PathBuf::from(launch.env.get("KIMI_CODE_HOME").unwrap());
        assert_eq!(
            launch.env.get(WORKMAN_EPHEMERAL_AGENT_HOME_ENV),
            launch.env.get("KIMI_CODE_HOME")
        );
        assert!(launch.command.contains("WORKMAN_MCP_TOKEN"));
        assert!(!launch.command.contains("test-process-token"));
        assert!(launch.command.ends_with("true --model kimi-test"));
        assert!(!home.join("mcp.json").exists());
        let template = fs::read_to_string(home.join(KIMI_MCP_TEMPLATE_FILE)).unwrap();
        assert!(template.contains(KIMI_MCP_TOKEN_SENTINEL));
        assert!(template.contains("x-workman-mcp-token"));
        assert!(!template.contains("existing"));
        assert_eq!(
            fs::read_to_string(source.path().join("mcp.json")).unwrap(),
            source_mcp
        );
        assert_eq!(
            fs::read_to_string(home.join("config.toml")).unwrap(),
            "default_model = \"fixture\"\n"
        );

        let status = std::process::Command::new("/bin/sh")
            .args(["-c", &launch.command])
            .env("WORKMAN_MCP_TOKEN", "test-process-token")
            .status()
            .unwrap();
        assert!(status.success());
        let config: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(home.join("mcp.json")).unwrap()).unwrap();
        assert_eq!(
            config["mcpServers"]["workman"]["url"],
            "http://127.0.0.1:43127/mcp"
        );
        assert_eq!(
            config["mcpServers"]["workman"]["headers"]["x-workman-mcp-token"],
            "test-process-token"
        );
        assert!(
            fs::read_to_string(home.join(KIMI_MCP_TEMPLATE_FILE))
                .unwrap()
                .contains(KIMI_MCP_TOKEN_SENTINEL)
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(home.join("mcp.json"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }

        let capability = mcp_launch_capability("kimi");
        assert!(capability.supported);
        assert_eq!(
            capability.mechanism,
            "private per-launch KIMI_CODE_HOME config"
        );
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn preamble_carries_identity_project_and_mcp_hints_without_the_secret() {
        let project = Project {
            id: 7,
            path: "/tmp/workspace".into(),
            name: "demo".into(),
            display_name: None,
            icon: None,
            selected: false,
            sort_order: 0,
        };
        let process = Process {
            id: 41,
            project_id: project.id,
            kind: ProcessKind::Agent,
            name: "worker".into(),
            command: Some("claude".into()),
            working_dir: project.path.clone(),
            env: BTreeMap::new(),
            auto_start: false,
            auto_restart: false,
            restart_when_changed: Vec::new(),
            source: ProcessSource::Local,
            trust_hash: None,
            status: ProcessStatus::Running,
            pid: Some(123),
            exit_code: None,
            exit_signal: None,
            exited_at: None,
            agent_tool_id: Some(1),
            spawned_by_process_id: None,
            sort_order: 0,
        };
        let preamble = agent_instructions(
            &process,
            &project,
            "http://127.0.0.1:43126/mcp",
            "claude_code",
        );
        assert!(preamble.contains("process ID 41 (worker)"));
        assert!(preamble.contains("project 7 (demo, repo /tmp/workspace)"));
        assert!(preamble.contains("WORKMAN_PROCESS_ID=41"));
        assert!(preamble.contains("WORKMAN_MCP_URL=http://127.0.0.1:43126/mcp"));
        assert!(preamble.contains("${WORKMAN_MCP_TOKEN}"));
        assert!(preamble.contains("server named workman"));
        assert!(preamble.contains("never a globally configured Solo"));
        assert!(preamble.contains("Call whoami() through workman first"));
        assert!(preamble.contains(WORKTREE_AGENT_GUIDANCE));
        assert!(preamble.contains(
            "Put shared notes, plans, briefs, and hand-offs in Workman scratchpads with \
             scratchpad_write so they are visible in the app and verifiable; do not create \
             ad-hoc repo files for them."
        ));
        assert!(preamble.contains(
            "After creating a scratchpad or todo, read it back with scratchpad_read or todo_get \
             and reference its ID in every hand-off message."
        ));
        assert!(!preamble.contains("secret-token"));
    }
}
