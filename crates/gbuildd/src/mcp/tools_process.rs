//! MCP process lifecycle and terminal-output tools.

use std::time::Duration;

use axum::http::request::Parts;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use gbuild_core::{Actor, Process, ProcessId, ProjectId};
use rmcp::{
    handler::server::{tool::Extension, wrapper::Parameters},
    model::CallToolResult,
    schemars, tool, tool_router,
};
use serde::Deserialize;
use serde_json::json;

use super::{GbuildMcp, failure, scoped_project, success};
use crate::{ProcessRegistry, RegistryError};

const DEFAULT_OUTPUT_LINES: usize = 50;
const MAX_OUTPUT_LINES: usize = 200;
const DEFAULT_SEARCH_RESULTS: usize = 20;
const MAX_SEARCH_RESULTS: usize = 100;
const DEFAULT_RAW_BYTES: usize = 256 * 1024;
const MAX_RAW_BYTES: usize = 256 * 1024;
const FRESH_RAW_BYTES: usize = 64 * 1024;

#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
struct ProjectScopeArgs {
    /// Explicit project override. Otherwise selected project, then owning project is used.
    #[serde(default)]
    project_id: Option<ProjectId>,
}

#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
struct ProcessTargetArgs {
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
struct CloseProcessArgs {
    #[serde(default)]
    process_id: Option<ProcessId>,
    #[serde(default)]
    process_name: Option<String>,
    #[serde(default)]
    project_id: Option<ProjectId>,
    /// Required only when closing the process that owns this MCP session.
    #[serde(default)]
    confirm_self_close: bool,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct RenameProcessArgs {
    #[serde(default)]
    process_id: Option<ProcessId>,
    #[serde(default)]
    process_name: Option<String>,
    #[serde(default)]
    project_id: Option<ProjectId>,
    /// New process name.
    new_name: String,
}

#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
struct RenderedOutputArgs {
    #[serde(default)]
    process_id: Option<ProcessId>,
    #[serde(default)]
    process_name: Option<String>,
    #[serde(default)]
    project_id: Option<ProjectId>,
    /// Maximum rows to return. Defaults to 50 and is capped at 200.
    #[serde(default)]
    lines: Option<usize>,
    /// Optional zero-based first retained row.
    #[serde(default)]
    start_row: Option<usize>,
    /// Optional zero-based, exclusive final retained row.
    #[serde(default)]
    end_row: Option<usize>,
}

#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
struct RawOutputArgs {
    #[serde(default)]
    process_id: Option<ProcessId>,
    #[serde(default)]
    process_name: Option<String>,
    #[serde(default)]
    project_id: Option<ProjectId>,
    /// Maximum text lines to retain in the readable tail. Defaults to 50, max 200.
    #[serde(default)]
    lines: Option<usize>,
    /// Optional absolute raw stream byte offset.
    #[serde(default)]
    offset: Option<u64>,
    /// Maximum raw bytes to read. Defaults to and is capped at 256 KiB.
    #[serde(default)]
    max_bytes: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SearchOutputArgs {
    #[serde(default)]
    process_id: Option<ProcessId>,
    #[serde(default)]
    process_name: Option<String>,
    #[serde(default)]
    project_id: Option<ProjectId>,
    /// Case-insensitive substring to find.
    pattern: String,
    /// Maximum matches. Defaults to 20 and is capped at 100.
    #[serde(default)]
    max_results: Option<usize>,
}

#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
struct SendInputArgs {
    #[serde(default)]
    process_id: Option<ProcessId>,
    #[serde(default)]
    process_name: Option<String>,
    #[serde(default)]
    project_id: Option<ProjectId>,
    /// UTF-8 text to write. Enter (CR) is appended unless submit=false.
    #[serde(default)]
    input: Option<String>,
    /// Raw PTY bytes. When present this overrides input and submit.
    #[serde(default)]
    bytes: Option<Vec<u8>>,
    /// Submit text with Enter (CR). Defaults to true.
    #[serde(default)]
    submit: Option<bool>,
    /// Wait before returning a rendered tail; clamped to 250-10000ms.
    #[serde(default)]
    wait_ms: Option<u64>,
}

#[tool_router(router = process_tool_router, vis = "pub(crate)")]
impl GbuildMcp {
    #[tool(description = "List processes in the effective project scope")]
    async fn list_processes(
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
            Ok(processes) => success(processes),
            Err(error) => registry_failure(error),
        }
    }

    #[tool(description = "Read detailed status for one process")]
    async fn get_process_status(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<ProcessTargetArgs>,
    ) -> CallToolResult {
        let mut registry = self.registry.lock().await;
        let (process, _) = match resolve_process(&mut registry, &parts, target(&args)) {
            Ok(resolved) => resolved,
            Err(error) => return target_failure(error),
        };
        match registry.get_status(process.id) {
            Ok(status) => success(status),
            Err(error) => registry_failure(error),
        }
    }

    #[tool(description = "Start an existing command, terminal, or agent")]
    async fn start_process(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<ProcessTargetArgs>,
    ) -> CallToolResult {
        lifecycle(self, &parts, target(&args), LifecycleAction::Start).await
    }

    #[tool(description = "Gracefully stop one running process")]
    async fn stop_process(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<ProcessTargetArgs>,
    ) -> CallToolResult {
        lifecycle(self, &parts, target(&args), LifecycleAction::Stop).await
    }

    #[tool(description = "Restart an existing command, terminal, or agent")]
    async fn restart_process(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<ProcessTargetArgs>,
    ) -> CallToolResult {
        lifecycle(self, &parts, target(&args), LifecycleAction::Restart).await
    }

    #[tool(description = "Remove a stored process; closing this MCP session requires confirmation")]
    async fn close_process(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<CloseProcessArgs>,
    ) -> CallToolResult {
        let mut registry = self.registry.lock().await;
        let target = ProcessTarget {
            process_id: args.process_id,
            process_name: args.process_name.as_deref(),
            project_id: args.project_id,
        };
        let (process, actor) = match resolve_process(&mut registry, &parts, target) {
            Ok(resolved) => resolved,
            Err(error) => return target_failure(error),
        };
        if actor.process_id == Some(process.id) && !args.confirm_self_close {
            return failure(
                "self_close_confirmation_required",
                "set confirm_self_close=true only when explicitly closing this MCP session's own process",
            );
        }
        match registry.close(process.id) {
            Ok(process) => success(json!({ "closed": true, "process": process })),
            Err(error) => registry_failure(error),
        }
    }

    #[tool(description = "Rename one process")]
    async fn rename_process(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<RenameProcessArgs>,
    ) -> CallToolResult {
        let mut registry = self.registry.lock().await;
        let target = ProcessTarget {
            process_id: args.process_id,
            process_name: args.process_name.as_deref(),
            project_id: args.project_id,
        };
        let (process, _) = match resolve_process(&mut registry, &parts, target) {
            Ok(resolved) => resolved,
            Err(error) => return target_failure(error),
        };
        let result = registry
            .rename(process.id, args.new_name)
            .and_then(|process| registry.get_status(process.id));
        match result {
            Ok(status) => success(status),
            Err(error) => registry_failure(error),
        }
    }

    #[tool(description = "Select one process as the project's focused terminal")]
    async fn select_process(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<ProcessTargetArgs>,
    ) -> CallToolResult {
        let mut registry = self.registry.lock().await;
        let (process, _) = match resolve_process(&mut registry, &parts, target(&args)) {
            Ok(resolved) => resolved,
            Err(error) => return target_failure(error),
        };
        match registry.select(process.id) {
            Ok(process) => success(json!({
                "selected_process_id": process.id,
                "process": process,
            })),
            Err(error) => registry_failure(error),
        }
    }

    #[tool(description = "Start all command processes in the effective project")]
    async fn start_all_commands(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<ProjectScopeArgs>,
    ) -> CallToolResult {
        bulk_commands(self, &parts, args.project_id, BulkAction::Start).await
    }

    #[tool(description = "Stop all running command processes in the effective project")]
    async fn stop_all_commands(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<ProjectScopeArgs>,
    ) -> CallToolResult {
        bulk_commands(self, &parts, args.project_id, BulkAction::Stop).await
    }

    #[tool(description = "Restart all command processes in the effective project")]
    async fn restart_all_commands(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<ProjectScopeArgs>,
    ) -> CallToolResult {
        bulk_commands(self, &parts, args.project_id, BulkAction::Restart).await
    }

    #[tool(
        description = "Return a ranged, escape-free terminal rendering; defaults to the last 50 rows"
    )]
    async fn get_process_output(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<RenderedOutputArgs>,
    ) -> CallToolResult {
        let mut registry = self.registry.lock().await;
        let target = ProcessTarget {
            process_id: args.process_id,
            process_name: args.process_name.as_deref(),
            project_id: args.project_id,
        };
        let (process, _) = match resolve_process(&mut registry, &parts, target) {
            Ok(resolved) => resolved,
            Err(error) => return target_failure(error),
        };
        let lines = args
            .lines
            .unwrap_or(DEFAULT_OUTPUT_LINES)
            .clamp(1, MAX_OUTPUT_LINES);
        let range = if args.start_row.is_some() || args.end_row.is_some() {
            let start = args.start_row.unwrap_or(0);
            let requested_end = args.end_row.unwrap_or_else(|| start.saturating_add(lines));
            if requested_end < start {
                return failure("invalid_row_range", "end_row must not precede start_row");
            }
            start..requested_end.min(start.saturating_add(MAX_OUTPUT_LINES))
        } else {
            let metadata = match registry.rendered_output_range(process.id, usize::MAX..usize::MAX)
            {
                Ok(metadata) => metadata,
                Err(error) => return registry_failure(error),
            };
            let end = meaningful_rendered_end(&metadata);
            end.saturating_sub(lines)..end
        };
        match registry.rendered_output_range(process.id, range) {
            Ok(output) => success(json!({
                "process_id": process.id,
                "process_name": process.name,
                "output": output.text,
                "start_row": output.start,
                "end_row": output.end,
                "total_rows": output.total_rows,
                "viewport_start": output.viewport_start,
                "cursor_row": output.cursor_row,
                "alternate_screen": output.alternate_screen,
                "raw_end_offset": output.raw_end_offset,
                "status": output.status,
            })),
            Err(error) => registry_failure(error),
        }
    }

    #[tool(
        description = "Return retained raw PTY output, including cleared and alternate-screen bytes"
    )]
    async fn get_process_raw_output(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<RawOutputArgs>,
    ) -> CallToolResult {
        let mut registry = self.registry.lock().await;
        let target = ProcessTarget {
            process_id: args.process_id,
            process_name: args.process_name.as_deref(),
            project_id: args.project_id,
        };
        let (process, _) = match resolve_process(&mut registry, &parts, target) {
            Ok(resolved) => resolved,
            Err(error) => return target_failure(error),
        };
        let max_bytes = args
            .max_bytes
            .unwrap_or(DEFAULT_RAW_BYTES)
            .clamp(1, MAX_RAW_BYTES);
        let offset = match args.offset {
            Some(offset) => offset,
            None => match registry.raw_output(process.id, None, 0) {
                Ok(metadata) => metadata.total_bytes.saturating_sub(max_bytes as u64),
                Err(error) => return registry_failure(error),
            },
        };
        match registry.raw_output(process.id, Some(offset), max_bytes) {
            Ok(output) => {
                let text = String::from_utf8_lossy(&output.data);
                let lines = args
                    .lines
                    .unwrap_or(DEFAULT_OUTPUT_LINES)
                    .clamp(1, MAX_OUTPUT_LINES);
                success(json!({
                    "process_id": process.id,
                    "process_name": process.name,
                    "output": tail_lines(&text, lines),
                    "data_base64": BASE64.encode(&output.data),
                    "start_offset": output.start_offset,
                    "end_offset": output.end_offset,
                    "total_bytes": output.total_bytes,
                    "truncated": output.truncated,
                    "status": output.status,
                }))
            }
            Err(error) => registry_failure(error),
        }
    }

    #[tool(description = "Search rendered terminal rows with a case-insensitive substring")]
    async fn search_output(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<SearchOutputArgs>,
    ) -> CallToolResult {
        search(self, &parts, args, false).await
    }

    #[tool(description = "Search the retained raw PTY stream with a case-insensitive substring")]
    async fn search_raw_output(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<SearchOutputArgs>,
    ) -> CallToolResult {
        search(self, &parts, args, true).await
    }

    #[tool(description = "Clear retained raw and rendered output without stopping the process")]
    async fn clear_output(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<ProcessTargetArgs>,
    ) -> CallToolResult {
        let mut registry = self.registry.lock().await;
        let (process, _) = match resolve_process(&mut registry, &parts, target(&args)) {
            Ok(resolved) => resolved,
            Err(error) => return target_failure(error),
        };
        match registry.clear_output(process.id) {
            Ok(process) => success(json!({
                "process_id": process.id,
                "cleared": true,
                "status": process.status,
            })),
            Err(error) => registry_failure(error),
        }
    }

    #[tool(
        description = "Send text or raw bytes to a running process; wait_ms returns a fresh tail"
    )]
    async fn send_input(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<SendInputArgs>,
    ) -> CallToolResult {
        let input = match prepared_input(&args) {
            Ok(input) => input,
            Err(error) => return failure("invalid_input", error),
        };
        let bytes_sent = input.data.len() + usize::from(input.submit);
        let (process_id, process_name, cursor) = {
            let mut registry = self.registry.lock().await;
            let target = ProcessTarget {
                process_id: args.process_id,
                process_name: args.process_name.as_deref(),
                project_id: args.project_id,
            };
            let (process, _) = match resolve_process(&mut registry, &parts, target) {
                Ok(resolved) => resolved,
                Err(error) => return target_failure(error),
            };
            let cursor = match registry.raw_output(process.id, None, 0) {
                Ok(output) => output.total_bytes,
                Err(error) => return registry_failure(error),
            };
            let sent = if input.submit {
                registry.submit_input(process.id, &input.data)
            } else {
                registry.send_input(process.id, &input.data)
            };
            if let Err(error) = sent {
                return registry_failure(error);
            }
            (process.id, process.name, cursor)
        };

        let waited_ms = args.wait_ms.map(|wait| wait.clamp(250, 10_000));
        if let Some(waited_ms) = waited_ms {
            tokio::time::sleep(Duration::from_millis(waited_ms)).await;
        }

        let mut registry = self.registry.lock().await;
        let status = match registry.get_status(process_id) {
            Ok(status) => status,
            Err(error) => return registry_failure(error),
        };
        let (output, fresh_raw_output, raw_end_offset) = if waited_ms.is_some() {
            let rendered = match rendered_tail(&mut registry, process_id, DEFAULT_OUTPUT_LINES) {
                Ok(rendered) => rendered,
                Err(error) => return registry_failure(error),
            };
            let raw = match registry.raw_output(process_id, Some(cursor), FRESH_RAW_BYTES) {
                Ok(raw) => raw,
                Err(error) => return registry_failure(error),
            };
            (
                Some(rendered.text),
                Some(String::from_utf8_lossy(&raw.data).into_owned()),
                raw.end_offset,
            )
        } else {
            (None, None, cursor)
        };
        success(json!({
            "process_id": process_id,
            "process_name": process_name,
            "bytes_sent": bytes_sent,
            "waited_ms": waited_ms,
            "output": output,
            "fresh_raw_output": fresh_raw_output,
            "raw_end_offset": raw_end_offset,
            "status": status,
        }))
    }
}

#[derive(Clone, Copy)]
struct ProcessTarget<'a> {
    process_id: Option<ProcessId>,
    process_name: Option<&'a str>,
    project_id: Option<ProjectId>,
}

fn target(args: &ProcessTargetArgs) -> ProcessTarget<'_> {
    ProcessTarget {
        process_id: args.process_id,
        process_name: args.process_name.as_deref(),
        project_id: args.project_id,
    }
}

type TargetError = (&'static str, String);

fn resolve_process(
    registry: &mut ProcessRegistry,
    parts: &Parts,
    target: ProcessTarget<'_>,
) -> Result<(Process, Actor), TargetError> {
    if target.process_id.is_some() && target.process_name.is_some() {
        return Err((
            "ambiguous_process_target",
            "pass process_id or process_name, not both".into(),
        ));
    }
    let (project, actor) = scoped_project(registry, parts, target.project_id)
        .map_err(|error| ("project_scope_error", error))?;

    let process = if let Some(process_id) = target.process_id {
        registry.get(process_id).map_err(registry_target_error)?
    } else if let Some(process_name) = target.process_name {
        let processes = registry
            .list(Some(project.id))
            .map_err(registry_target_error)?;
        if let Some(process) = processes
            .iter()
            .find(|process| process.name == process_name)
            .cloned()
        {
            process
        } else if let Some(process_id) = parse_process_id(process_name) {
            registry.get(process_id).map_err(registry_target_error)?
        } else {
            return Err((
                "process_not_found",
                format!(
                    "process {process_name:?} was not found in project {}",
                    project.id
                ),
            ));
        }
    } else {
        let process_id = actor.process_id.ok_or_else(|| {
            (
                "process_target_required",
                "pass process_id or process_name; this MCP session has no owning process".into(),
            )
        })?;
        registry.get(process_id).map_err(registry_target_error)?
    };

    if process.project_id != project.id {
        return Err((
            "process_not_in_project",
            format!(
                "process {} belongs to project {}, not effective project {}",
                process.id, process.project_id, project.id
            ),
        ));
    }
    Ok((process, actor))
}

pub(super) fn resolve_process_target(
    registry: &mut ProcessRegistry,
    parts: &Parts,
    process_id: Option<ProcessId>,
    process_name: Option<&str>,
    project_id: Option<ProjectId>,
) -> Result<(Process, Actor), (&'static str, String)> {
    resolve_process(
        registry,
        parts,
        ProcessTarget {
            process_id,
            process_name,
            project_id,
        },
    )
}

fn parse_process_id(value: &str) -> Option<ProcessId> {
    value.parse().ok().or_else(|| {
        value
            .rsplit_once("--")
            .and_then(|(_, suffix)| suffix.parse().ok())
    })
}

fn registry_target_error(error: RegistryError) -> TargetError {
    (error.code(), error.to_string())
}

fn target_failure((code, message): TargetError) -> CallToolResult {
    failure(code, message)
}

fn registry_failure(error: RegistryError) -> CallToolResult {
    failure(error.code(), error.to_string())
}

#[derive(Clone, Copy)]
enum LifecycleAction {
    Start,
    Stop,
    Restart,
}

async fn lifecycle(
    service: &GbuildMcp,
    parts: &Parts,
    target: ProcessTarget<'_>,
    action: LifecycleAction,
) -> CallToolResult {
    let mut registry = service.registry.lock().await;
    let (process, _) = match resolve_process(&mut registry, parts, target) {
        Ok(resolved) => resolved,
        Err(error) => return target_failure(error),
    };
    let result = match action {
        LifecycleAction::Start => registry.start(process.id),
        LifecycleAction::Stop => registry.stop(process.id),
        LifecycleAction::Restart => registry.restart(process.id),
    }
    .and_then(|process| registry.get_status(process.id));
    match result {
        Ok(status) => success(status),
        Err(error) => registry_failure(error),
    }
}

#[derive(Clone, Copy)]
enum BulkAction {
    Start,
    Stop,
    Restart,
}

async fn bulk_commands(
    service: &GbuildMcp,
    parts: &Parts,
    project_id: Option<ProjectId>,
    action: BulkAction,
) -> CallToolResult {
    let mut registry = service.registry.lock().await;
    let (project, _) = match scoped_project(&mut registry, parts, project_id) {
        Ok(scoped) => scoped,
        Err(error) => return failure("project_scope_error", error),
    };
    let result = match action {
        BulkAction::Start => registry.start_all_commands(project.id),
        BulkAction::Stop => registry.stop_all_commands(project.id),
        BulkAction::Restart => registry.restart_all_commands(project.id),
    };
    success(result)
}

async fn search(
    service: &GbuildMcp,
    parts: &Parts,
    args: SearchOutputArgs,
    raw: bool,
) -> CallToolResult {
    if args.pattern.is_empty() {
        return failure("invalid_search_pattern", "pattern must not be empty");
    }
    let mut registry = service.registry.lock().await;
    let target = ProcessTarget {
        process_id: args.process_id,
        process_name: args.process_name.as_deref(),
        project_id: args.project_id,
    };
    let (process, _) = match resolve_process(&mut registry, parts, target) {
        Ok(resolved) => resolved,
        Err(error) => return target_failure(error),
    };
    let max_results = args
        .max_results
        .unwrap_or(DEFAULT_SEARCH_RESULTS)
        .clamp(1, MAX_SEARCH_RESULTS);
    if raw {
        match registry.search_raw_output(process.id, &args.pattern, max_results) {
            Ok(matches) => success(json!({
                "process_id": process.id,
                "pattern": args.pattern,
                "matches": matches,
            })),
            Err(error) => registry_failure(error),
        }
    } else {
        match registry.search_rendered_output(process.id, &args.pattern, max_results) {
            Ok(matches) => success(json!({
                "process_id": process.id,
                "pattern": args.pattern,
                "matches": matches,
            })),
            Err(error) => registry_failure(error),
        }
    }
}

struct PreparedInput {
    data: Vec<u8>,
    submit: bool,
}

fn prepared_input(args: &SendInputArgs) -> Result<PreparedInput, String> {
    if let Some(bytes) = &args.bytes {
        return Ok(PreparedInput {
            data: bytes.clone(),
            submit: false,
        });
    }
    let Some(input) = &args.input else {
        return Err("pass input text or raw bytes".into());
    };
    Ok(PreparedInput {
        data: input.as_bytes().to_vec(),
        submit: args.submit.unwrap_or(true),
    })
}

fn rendered_tail(
    registry: &mut ProcessRegistry,
    process_id: ProcessId,
    lines: usize,
) -> Result<crate::process_registry::RenderedOutputRange, RegistryError> {
    let metadata = registry.rendered_output_range(process_id, usize::MAX..usize::MAX)?;
    let end = meaningful_rendered_end(&metadata);
    registry.rendered_output_range(process_id, end.saturating_sub(lines)..end)
}

fn meaningful_rendered_end(output: &crate::process_registry::RenderedOutputRange) -> usize {
    if output.alternate_screen {
        output.total_rows
    } else if output.total_rows == 0 {
        0
    } else {
        output.cursor_row.saturating_add(1).min(output.total_rows)
    }
}

fn tail_lines(text: &str, lines: usize) -> String {
    let mut tail = text.lines().rev().take(lines).collect::<Vec<_>>();
    tail.reverse();
    tail.join("\n")
}

#[cfg(test)]
mod tests {
    use super::{SendInputArgs, prepared_input};

    #[test]
    fn submitted_text_preserves_multiline_content_and_ends_with_carriage_return() {
        let args = SendInputArgs {
            input: Some("first line\nsecond line\n".into()),
            submit: Some(true),
            ..SendInputArgs::default()
        };

        let input = prepared_input(&args).unwrap();
        assert_eq!(input.data, b"first line\nsecond line\n");
        assert!(input.submit);
    }

    #[test]
    fn raw_bytes_and_unsubmitted_text_remain_byte_exact() {
        let raw = SendInputArgs {
            bytes: Some(vec![0x0a, 0x0d]),
            submit: Some(true),
            ..SendInputArgs::default()
        };
        let raw = prepared_input(&raw).unwrap();
        assert_eq!(raw.data, vec![0x0a, 0x0d]);
        assert!(!raw.submit);

        let text = SendInputArgs {
            input: Some("partial\ntext".into()),
            submit: Some(false),
            ..SendInputArgs::default()
        };
        let text = prepared_input(&text).unwrap();
        assert_eq!(text.data, b"partial\ntext");
        assert!(!text.submit);
    }
}
