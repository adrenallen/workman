//! Agent-tool discovery and local terminal/agent spawning MCP tools.

use std::{collections::BTreeMap, env};

use axum::http::request::Parts;
use gbuild_core::{
    AgentTool, AgentToolId, AgentToolSource, Process, ProcessId, ProcessKind, ProcessSource,
    ProcessStatus, Project, ProjectId,
};
use rmcp::{
    handler::server::{tool::Extension, wrapper::Parameters},
    model::CallToolResult,
    schemars, tool, tool_router,
};
use serde::{Deserialize, Serialize};

use super::{GbuildMcp, failure, scoped_project, success};
use crate::ProcessRegistry;

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
impl GbuildMcp {
    #[tool(description = "List enabled and disabled built-in or custom agent command presets")]
    async fn list_agent_tools(&self) -> CallToolResult {
        let registry = self.registry.lock().await;
        match load_agent_tools(&registry) {
            Ok(tools) => success(tools),
            Err(error) => failure("store_error", error),
        }
    }

    #[tool(description = "Spawn a managed interactive terminal or registered agent process")]
    async fn spawn_process(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<SpawnProcessArgs>,
    ) -> CallToolResult {
        let mut registry = self.registry.lock().await;
        let (project, _) = match scoped_project(&mut registry, &parts, args.project_id) {
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
        let (project, _) = match scoped_project(&mut registry, &parts, args.project_id) {
            Ok(scoped) => scoped,
            Err(error) => return failure("project_scope_error", error),
        };
        match spawn_registered_agent(
            &mut registry,
            &project,
            args.agent_tool_id,
            args.name,
            args.extra_args,
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

fn load_agent_tool(
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
    let command = command_with_args(&tool.command, &extra_args)?;
    let name = process_name(registry, project.id, name, &tool.name)?;
    spawn(
        registry,
        project,
        ProcessKind::Agent,
        name,
        command,
        Some(tool.id),
    )
}

fn spawn(
    registry: &mut ProcessRegistry,
    project: &Project,
    kind: ProcessKind,
    name: String,
    command: String,
    agent_tool_id: Option<AgentToolId>,
) -> Result<SpawnResult, String> {
    let created = registry
        .create(Process {
            id: 0,
            project_id: project.id,
            kind,
            name,
            command: Some(command),
            working_dir: project.path.clone(),
            env: BTreeMap::new(),
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
        })
        .map_err(|error| error.to_string())?;
    let running = match registry.start(created.id) {
        Ok(process) => process,
        Err(error) => {
            let _ = registry.close(created.id);
            return Err(error.to_string());
        }
    };
    let agent_instructions =
        (kind == ProcessKind::Agent).then(|| agent_instructions(&running, project));
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

fn agent_instructions(process: &Process, project: &Project) -> String {
    format!(
        "[gbuild context] You are gbuild process ID {process_id} ({process_name}), in project \
         {project_id} ({project_name}, repo {project_path}). gbuild set \
         GBUILD_PROCESS_ID={process_id} and injected the secret GBUILD_MCP_TOKEN environment \
         variable. Use the gbuild MCP tools at /mcp: configure the \
         x-gbuild-mcp-token header from ${{GBUILD_MCP_TOKEN}}, then call whoami() first to \
         confirm that you auto-identify as process {process_id}. Use \
         identify_session(process_id={process_id}) only if whoami cannot identify you. \
         [END GBUILD CONTEXT]",
        process_id = process.id,
        process_name = process.name,
        project_id = project.id,
        project_name = project.name,
        project_path = project.path,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use gbuild_core::Store;

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
    fn preamble_carries_identity_project_and_mcp_hints_without_the_secret() {
        let project = Project {
            id: 7,
            path: "/tmp/workspace".into(),
            name: "demo".into(),
            display_name: None,
            icon: None,
            selected: false,
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
        };
        let preamble = agent_instructions(&process, &project);
        assert!(preamble.contains("process ID 41 (worker)"));
        assert!(preamble.contains("project 7 (demo, repo /tmp/workspace)"));
        assert!(preamble.contains("GBUILD_PROCESS_ID=41"));
        assert!(preamble.contains("${GBUILD_MCP_TOKEN}"));
        assert!(preamble.contains("call whoami() first"));
        assert!(!preamble.contains("secret-token"));
    }
}
