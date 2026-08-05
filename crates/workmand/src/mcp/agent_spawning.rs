//! Agent-tool discovery and local terminal/agent spawning MCP tools.

use std::{
    collections::BTreeMap,
    env,
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
use workman_core::{
    AgentTool, AgentToolId, AgentToolSource, Process, ProcessId, ProcessKind, ProcessSource,
    ProcessStatus, Project, ProjectId,
};

use super::{
    SCRATCHPAD_HANDOFF_GUIDANCE, WORKTREE_AGENT_GUIDANCE, WorkmanMcp, failure, scoped_project,
    success,
};
use crate::ProcessRegistry;

const WORKMAN_MCP_URL_ENV: &str = "WORKMAN_MCP_URL";
const INITIAL_DIALOG_TIMEOUT: Duration = Duration::from_secs(3);
const INITIAL_OUTPUT_SETTLE: Duration = Duration::from_millis(750);
const INITIAL_OUTPUT_QUIET: Duration = Duration::from_millis(200);
const DIALOG_CLEAR_TIMEOUT: Duration = Duration::from_secs(2);
const DIALOG_POLL_INTERVAL: Duration = Duration::from_millis(10);

#[derive(Clone, Copy, Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
enum SpawnKind {
    Terminal,
    Agent,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SpawnProcessArgs {
    /// Explicit project override. Otherwise selected project, then owning project is used.
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
    /// Explicit project override. Otherwise selected project, then owning project is used.
    #[serde(default)]
    project_id: Option<ProjectId>,
    agent_tool_id: AgentToolId,
    /// Optional per-launch process name, unique within the project.
    #[serde(default)]
    name: Option<String>,
    /// Safely shell-quoted arguments appended to the registered agent command.
    #[serde(default)]
    extra_args: Vec<String>,
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
    /// Explicit project override. Otherwise selected project, then owning project is used.
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
        description = "Run cheap PATH, version, and config-presence checks for every agent runtime"
    )]
    async fn agent_tools_health(&self) -> CallToolResult {
        let tools = {
            let registry = self.registry.lock().await;
            match load_agent_tools(&registry) {
                Ok(tools) => tools,
                Err(error) => return failure("store_error", error),
            }
        };
        success(crate::runtime_doctor::check_agent_tools(tools).await)
    }

    #[tool(
        description = "Preview the complete consent-gated workman MCP config for one agent runtime"
    )]
    async fn agent_tool_configure_preview(
        &self,
        Parameters(args): Parameters<AgentToolConfigArgs>,
    ) -> CallToolResult {
        let tool = {
            let registry = self.registry.lock().await;
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
        description = "Write a previously previewed agent MCP config after explicit confirmation"
    )]
    async fn agent_tool_configure(
        &self,
        Parameters(args): Parameters<AgentToolConfigWriteArgs>,
    ) -> CallToolResult {
        let tool = {
            let registry = self.registry.lock().await;
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
        let mut registry = self.registry.lock().await;
        let (project, actor) = match scoped_project(&mut registry, &parts, args.project_id) {
            Ok(scoped) => scoped,
            Err(error) => return failure("project_scope_error", error),
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
                process_name(&registry, project.id, args.name, "terminal").and_then(|name| {
                    spawn(
                        &mut registry,
                        &project,
                        ProcessKind::Terminal,
                        name,
                        default_shell(),
                        None,
                        None,
                        BTreeMap::new(),
                        actor.process_id,
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
                    &mut registry,
                    &project,
                    agent_tool_id,
                    args.name,
                    args.extra_args,
                    &self.mcp_url,
                    args.auto_acknowledge_dialogs,
                    actor.process_id,
                )
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
        let mut registry = self.registry.lock().await;
        let (project, actor) = match scoped_project(&mut registry, &parts, args.project_id) {
            Ok(scoped) => scoped,
            Err(error) => return failure("project_scope_error", error),
        };
        match spawn_registered_agent(
            &mut registry,
            &project,
            args.agent_tool_id,
            args.name,
            args.extra_args,
            &self.mcp_url,
            args.auto_acknowledge_dialogs,
            actor.process_id,
        ) {
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
    if let Some(id) = id {
        let existing = registry
            .store()
            .get_agent_tool(id)
            .map_err(|error| error.to_string())?;
        if existing.is_some_and(|tool| tool.source == AgentToolSource::Config) {
            return crate::user_config::update_config_managed_agent_tool(
                registry.store(),
                id,
                name,
                command,
                tool_type,
                enabled,
            )
            .map_err(|error| error.to_string());
        }
    }
    save_agent_tool(registry, id, name, command, tool_type, enabled)
}

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
    registry
        .store()
        .delete_agent_tool(agent_tool_id)
        .map_err(|error| error.to_string())
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

pub(crate) fn spawn_registered_agent(
    registry: &mut ProcessRegistry,
    project: &Project,
    agent_tool_id: AgentToolId,
    name: Option<String>,
    extra_args: Vec<String>,
    mcp_url: &str,
    auto_acknowledge_dialogs: bool,
    spawned_by_process_id: Option<ProcessId>,
) -> Result<SpawnResult, String> {
    let tool = load_agent_tool(registry, agent_tool_id)?;
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
    let command = command_with_mcp_wiring(&tool.command, &tool.tool_type, mcp_url, &extra_args)?;
    let name = process_name(registry, project.id, name, &tool.name)?;
    let env = BTreeMap::from([(WORKMAN_MCP_URL_ENV.to_owned(), mcp_url.to_owned())]);
    let tool_type = tool.tool_type;
    let result = spawn(
        registry,
        project,
        ProcessKind::Agent,
        name,
        command,
        Some(tool.id),
        Some(tool_type.clone()),
        env,
        spawned_by_process_id,
    )?;
    if auto_acknowledge_dialogs && supports_first_run_dialog_ack(&tool_type) {
        auto_acknowledge_initial_dialog(registry, result.process_id)?;
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
    let (tool, project) = {
        let registry = registry.lock().await;
        let tool = load_agent_tool(&registry, agent_tool_id)?;
        let project = registry
            .store()
            .get_project(project_id)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| format!("project {project_id} was not found"))?;
        (tool, project)
    };
    let health = crate::runtime_doctor::check_agent_tools(vec![tool.clone()]).await;
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
        _ => (Vec::new(), true),
    };
    let process_id = {
        let mut registry = registry.lock().await;
        let spawned = spawn_registered_agent(
            &mut registry,
            &project,
            agent_tool_id,
            None,
            extra_args,
            mcp_url,
            true,
            spawned_by_process_id,
        )?;
        if submit_prompt {
            if let Err(error) = registry.submit_input(spawned.process_id, prompt.as_bytes()) {
                let _ = registry.close(spawned.process_id);
                return Err(error.to_string());
            }
        }
        spawned.process_id
    };

    let deadline =
        Instant::now() + Duration::from_millis(timeout_ms.unwrap_or(30_000).clamp(1_000, 60_000));
    let mut success = false;
    let mut message = "The agent did not call whoami before the deep-check deadline.".to_owned();
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

fn auto_acknowledge_initial_dialog(
    registry: &mut ProcessRegistry,
    process_id: ProcessId,
) -> Result<(), String> {
    let started = Instant::now();
    let deadline = started + INITIAL_DIALOG_TIMEOUT;
    let mut last_total_bytes = 0_u64;
    let mut last_output_change = started;

    while Instant::now() < deadline {
        if let Some(dialog) = registry
            .pending_dialog(process_id)
            .map_err(|error| error.to_string())?
        {
            if !dialog.known_first_run {
                return Ok(());
            }
            registry
                .acknowledge_known_dialog(process_id)
                .map_err(|error| error.to_string())?;
            let clear_deadline = Instant::now() + DIALOG_CLEAR_TIMEOUT;
            while Instant::now() < clear_deadline {
                if registry
                    .pending_dialog(process_id)
                    .map_err(|error| error.to_string())?
                    .is_none()
                {
                    return Ok(());
                }
                std::thread::sleep(DIALOG_POLL_INTERVAL);
            }
            return Err(format!(
                "process {process_id} did not clear its acknowledged first-run dialog"
            ));
        }

        let raw = registry
            .raw_output(process_id, None, 0)
            .map_err(|error| error.to_string())?;
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
        std::thread::sleep(DIALOG_POLL_INTERVAL);
    }
    Ok(())
}

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

fn default_shell() -> String {
    env::var("SHELL")
        .ok()
        .filter(|shell| !shell.trim().is_empty())
        .unwrap_or_else(|| "/bin/sh".to_owned())
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

fn command_with_mcp_wiring(
    command: &str,
    tool_type: &str,
    mcp_url: &str,
    extra_args: &[String],
) -> Result<String, String> {
    let mut launch_args = mcp_launch_args(tool_type, mcp_url);
    launch_args.extend(extra_args.iter().cloned());
    command_with_args(command, &launch_args)
}

fn mcp_launch_args(tool_type: &str, mcp_url: &str) -> Vec<String> {
    match normalize_tool_type(tool_type).as_str() {
        "claude" | "claude_code" => vec![
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
        ],
        "codex" => vec![
            "-c".to_owned(),
            format!(
                "mcp_servers.workman.url={}",
                serde_json::to_string(mcp_url).expect("MCP URL serializes as a TOML string")
            ),
            "-c".to_owned(),
            "mcp_servers.workman.env_http_headers={\"x-workman-mcp-token\"=\"WORKMAN_MCP_TOKEN\"}"
                .to_owned(),
        ],
        _ => Vec::new(),
    }
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
    let client_wiring = match normalize_tool_type(tool_type).as_str() {
        "claude" | "claude_code" => {
            "This Claude launch already has a strict per-launch MCP config for the server named workman."
        }
        "codex" => {
            "This Codex launch already has per-launch mcp_servers.workman URL and header overrides."
        }
        _ => {
            "If this client does not expose a Workman connector automatically, configure one from WORKMAN_MCP_URL and WORKMAN_MCP_TOKEN before using coordination tools."
        }
    };
    format!(
        "[workman context] You are Workman process ID {process_id} ({process_name}), in project \
         {project_id} ({project_name}, repo {project_path}). Workman set \
         WORKMAN_PROCESS_ID={process_id}, WORKMAN_MCP_URL={mcp_url}, and the secret \
         WORKMAN_MCP_TOKEN environment variable. {client_wiring} The connector must use the exact \
         URL in ${{WORKMAN_MCP_URL}} ({mcp_url}) and send the x-workman-mcp-token header from \
         ${{WORKMAN_MCP_TOKEN}}. Use the MCP server named workman, never a globally configured Solo \
         or unrelated workman server. Call whoami() through workman first to confirm that you \
         auto-identify as process {process_id}. Use \
         identify_session(process_id={process_id}) only if whoami cannot identify you. \
         {worktree_agent_guidance} \
         {scratchpad_handoff_guidance} \
         [END WORKMAN CONTEXT]",
        process_id = process.id,
        process_name = process.name,
        project_id = project.id,
        project_name = project.name,
        project_path = project.path,
        client_wiring = client_wiring,
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
            })
            .unwrap();
        let tools = load_agent_tools(&registry).unwrap();
        assert_eq!(tools.len(), 5);
        assert!(tools.iter().any(|tool| tool.command == "claude"));
        assert!(tools.iter().any(|tool| tool.command == "/tmp/fake-agent"));
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
        let command = command_with_mcp_wiring(
            "claude --dangerously-skip-permissions",
            "claude_code",
            "http://127.0.0.1:43123/mcp",
            &["--model".into(), "opus".into()],
        )
        .unwrap();
        assert!(command.contains("--mcp-config"));
        assert!(command.contains("--strict-mcp-config"));
        assert!(command.contains("http://127.0.0.1:43123/mcp"));
        assert!(command.contains("x-workman-mcp-token"));
        assert!(command.contains("${WORKMAN_MCP_TOKEN}"));
        assert!(command.ends_with("--model opus"));
    }

    #[test]
    fn codex_launch_overrides_workman_url_and_process_token_header() {
        let command = command_with_mcp_wiring(
            "codex --dangerously-bypass-approvals-and-sandbox",
            "codex",
            "http://127.0.0.1:43124/mcp",
            &["--model".into(), "gpt-test".into()],
        )
        .unwrap();
        assert!(command.contains("mcp_servers.workman.url="));
        assert!(command.contains("http://127.0.0.1:43124/mcp"));
        assert!(command.contains("mcp_servers.workman.env_http_headers="));
        assert!(command.contains("WORKMAN_MCP_TOKEN"));
        assert!(command.ends_with("--model gpt-test"));
    }

    #[test]
    fn generic_launch_keeps_command_and_relies_on_injected_environment() {
        assert_eq!(
            command_with_mcp_wiring(
                "future-agent --interactive",
                "future_v9",
                "http://127.0.0.1:43125/mcp",
                &["two words".into()],
            )
            .unwrap(),
            "future-agent --interactive 'two words'"
        );
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
