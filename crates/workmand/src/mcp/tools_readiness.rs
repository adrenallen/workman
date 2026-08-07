//! MCP service discovery and process-readiness tools.

use std::time::Duration;

use axum::http::request::Parts;
use rmcp::{
    handler::server::{tool::Extension, wrapper::Parameters},
    model::CallToolResult,
    schemars, tool, tool_router,
};
use serde::Deserialize;
use serde_json::json;
use workman_core::{ProcessId, ProjectId};

use super::{WorkmanMcp, failure, scoped_project, success, tools_process::resolve_process_target};
use crate::{DEFAULT_PORT_WAIT, MAX_PORT_WAIT, ReadinessError, ReadinessService};

#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
struct ServicesListArgs {
    /// Optional project ID; an identified agent may name only its owning project.
    #[serde(default)]
    project_id: Option<ProjectId>,
}

#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
struct ProcessPortsArgs {
    /// Process ID. Omit with process_name to target this MCP session's own process.
    #[serde(default)]
    process_id: Option<ProcessId>,
    /// Exact process name; numeric values and names ending in `--<id>` also resolve by ID.
    #[serde(default)]
    process_name: Option<String>,
    /// Optional project scope override.
    #[serde(default)]
    project_id: Option<ProjectId>,
}

#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
struct WaitForBoundPortArgs {
    /// Process ID. Omit with process_name to target this MCP session's own process.
    #[serde(default)]
    process_id: Option<ProcessId>,
    /// Exact process name; numeric values and names ending in `--<id>` also resolve by ID.
    #[serde(default)]
    process_name: Option<String>,
    /// Optional project scope override.
    #[serde(default)]
    project_id: Option<ProjectId>,
    /// Maximum wait in milliseconds. Defaults to 30000 and is capped at 300000.
    #[serde(default)]
    timeout_ms: Option<u64>,
}

#[tool_router(router = readiness_tool_router, vis = "pub(crate)")]
impl WorkmanMcp {
    #[tool(
        description = "List active project services with readiness, ports, and localhost URLs. Returns { services: [...] }"
    )]
    async fn services_list(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<ServicesListArgs>,
    ) -> CallToolResult {
        let project_id = {
            let mut registry = self.registry.lock().await;
            match scoped_project(&mut registry, &parts, args.project_id) {
                Ok((project, _)) => project.id,
                Err(error) => return failure("project_scope_error", error),
            }
        };
        match ReadinessService::default()
            .services_list(&self.registry, Some(project_id))
            .await
        {
            Ok(services) => success(json!({ "services": services })),
            Err(error) => readiness_failure(error),
        }
    }

    #[tool(description = "Get listeners, ports, localhost URLs, and readiness for one process")]
    async fn get_process_ports(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<ProcessPortsArgs>,
    ) -> CallToolResult {
        let process_id = match resolve_target(
            self,
            &parts,
            args.process_id,
            args.process_name.as_deref(),
            args.project_id,
        )
        .await
        {
            Ok(process_id) => process_id,
            Err(error) => return error,
        };
        match ReadinessService::default()
            .get_process_ports(&self.registry, process_id)
            .await
        {
            Ok(service) => success(service),
            Err(error) => readiness_failure(error),
        }
    }

    #[tool(
        description = "Wait until a process or descendant binds a TCP port; timeout is a normal ready=false result"
    )]
    async fn wait_for_bound_port(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<WaitForBoundPortArgs>,
    ) -> CallToolResult {
        let process_id = match resolve_target(
            self,
            &parts,
            args.process_id,
            args.process_name.as_deref(),
            args.project_id,
        )
        .await
        {
            Ok(process_id) => process_id,
            Err(error) => return error,
        };
        let timeout = args
            .timeout_ms
            .map(Duration::from_millis)
            .unwrap_or(DEFAULT_PORT_WAIT)
            .min(MAX_PORT_WAIT);
        match ReadinessService::default()
            .wait_for_bound_port(&self.registry, process_id, timeout)
            .await
        {
            Ok(result) => success(result),
            Err(error) => readiness_failure(error),
        }
    }
}

async fn resolve_target(
    service: &WorkmanMcp,
    parts: &Parts,
    process_id: Option<ProcessId>,
    process_name: Option<&str>,
    project_id: Option<ProjectId>,
) -> Result<ProcessId, CallToolResult> {
    let mut registry = service.registry.lock().await;
    resolve_process_target(&mut registry, parts, process_id, process_name, project_id)
        .map(|(process, _)| process.id)
        .map_err(|(code, message)| failure(code, message))
}

fn readiness_failure(error: ReadinessError) -> CallToolResult {
    failure(error.code(), error.to_string())
}
