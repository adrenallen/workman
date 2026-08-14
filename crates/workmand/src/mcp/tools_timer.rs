//! Owner-scoped MCP timer tools and idle-watch target resolution.

use std::collections::BTreeSet;

use axum::http::request::Parts;
use rmcp::{
    handler::server::{tool::Extension, wrapper::Parameters},
    model::CallToolResult,
    schemars, tool, tool_router,
};
use serde::Deserialize;
use serde_json::json;
use workman_core::{
    Actor, Process, ProcessId, ProcessKind, Project, ProjectId, TimerId, TimerKind,
};

use super::{WorkmanMcp, failure, scoped_project, success};
use crate::{
    ProcessRegistry,
    timer_events::{TimerLifecycleEvent, TimerLifecycleKind},
    timers::{IdleTimerOutcome, TimerError, TimerService, now_millis},
};

const DEFAULT_TIMER_LIST_LIMIT: usize = 50;
const MAX_TIMER_LIST_LIMIT: usize = 200;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct TimerSetArgs {
    #[serde(default)]
    project_id: Option<ProjectId>,
    /// Delay before the first delivery.
    delay_ms: u64,
    body: String,
    /// Repeat using delay_ms when repeat_every_ms is omitted.
    #[serde(default, rename = "loop")]
    loop_timer: bool,
    /// Optional repeat interval. Supplying it makes the timer repeat.
    #[serde(default)]
    repeat_every_ms: Option<u64>,
    /// Agent process receiving the prompt. Defaults to the calling process.
    #[serde(default)]
    delivery_process_id: Option<ProcessId>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct IdleTimerArgs {
    #[serde(default)]
    project_id: Option<ProjectId>,
    processes: Vec<ProcessReference>,
    max_wait_ms: u64,
    body: String,
    /// Agent process receiving the prompt. Defaults to the calling process.
    #[serde(default)]
    delivery_process_id: Option<ProcessId>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
enum ProcessReference {
    Id(ProcessId),
    Name(String),
    Target(ProcessReferenceObject),
}

#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
struct ProcessReferenceObject {
    #[serde(default)]
    process_id: Option<ProcessId>,
    #[serde(default)]
    process_name: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct TimerTargetArgs {
    #[serde(default)]
    project_id: Option<ProjectId>,
    timer_id: TimerId,
}

#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
struct TimerListArgs {
    #[serde(default)]
    project_id: Option<ProjectId>,
    #[serde(default)]
    limit: Option<usize>,
}

#[tool_router(router = timer_tool_router, vis = "pub(crate)")]
impl WorkmanMcp {
    #[tool(description = "Set a one-shot or repeating delayed prompt delivery")]
    async fn timer_set(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<TimerSetArgs>,
    ) -> CallToolResult {
        let delay_ms = match i64::try_from(args.delay_ms) {
            Ok(delay_ms) => delay_ms,
            Err(_) => return timer_failure(TimerError::InvalidDelay),
        };
        let repeat_every_ms = match args.repeat_every_ms.map(i64::try_from).transpose() {
            Ok(interval) => interval,
            Err(_) => return timer_failure(TimerError::InvalidRepeatInterval),
        };
        let mut registry = self.registry.lock().await;
        let (project, actor) = match scoped_project(&mut registry, &parts, args.project_id) {
            Ok(scoped) => scoped,
            Err(error) => return failure("project_scope_error", error),
        };
        let delivery =
            match delivery_process(&mut registry, &project, &actor, args.delivery_process_id) {
                Ok(process) => process,
                Err((code, message)) => return failure(code, message),
            };
        let now = now_millis();
        match TimerService::new(&mut registry).set_delay(
            actor.id,
            delivery.id,
            args.body,
            delay_ms,
            args.loop_timer,
            repeat_every_ms,
            now,
        ) {
            Ok(timer) => {
                self.timer_events.publish(TimerLifecycleEvent::for_timer(
                    TimerLifecycleKind::Created,
                    project.id,
                    timer.clone(),
                    now,
                    None,
                ));
                success(json!({ "project_id": project.id, "timer": timer }))
            }
            Err(error) => timer_failure(error),
        }
    }

    #[tool(
        description = "Wake when any watched process makes a fresh transition into idle, with a hard timeout"
    )]
    async fn timer_fire_when_idle_any(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<IdleTimerArgs>,
    ) -> CallToolResult {
        self.set_idle_timer(&parts, args, TimerKind::IdleAny).await
    }

    #[tool(description = "Wake when every watched process is or becomes idle, with a hard timeout")]
    async fn timer_fire_when_idle_all(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<IdleTimerArgs>,
    ) -> CallToolResult {
        self.set_idle_timer(&parts, args, TimerKind::IdleAll).await
    }

    #[tool(
        description = "Cancel and delete one timer owned by this MCP process; ownership survives MCP reconnects"
    )]
    async fn timer_cancel(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<TimerTargetArgs>,
    ) -> CallToolResult {
        let mut registry = self.registry.lock().await;
        let (project, actor) = match scoped_project(&mut registry, &parts, args.project_id) {
            Ok(scoped) => scoped,
            Err(error) => return failure("project_scope_error", error),
        };
        match TimerService::new(&mut registry).cancel(&actor.id, project.id, args.timer_id) {
            Ok(timer) => {
                self.timer_events.publish(TimerLifecycleEvent::for_timer(
                    TimerLifecycleKind::Cancelled,
                    project.id,
                    timer.clone(),
                    now_millis(),
                    None,
                ));
                success(json!({
                    "project_id": project.id,
                    "timer_id": timer.timer.id,
                    "cancelled": true,
                }))
            }
            Err(error) => timer_failure(error),
        }
    }

    #[tool(description = "Pause one active timer owned by this MCP process")]
    async fn timer_pause(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<TimerTargetArgs>,
    ) -> CallToolResult {
        self.change_timer_pause(&parts, args, true).await
    }

    #[tool(description = "Resume one paused timer owned by this MCP process")]
    async fn timer_resume(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<TimerTargetArgs>,
    ) -> CallToolResult {
        self.change_timer_pause(&parts, args, false).await
    }

    #[tool(
        description = "List all timers visible in the effective project, including owner process, schedule, watch list, and state; mutations remain owner-process-only"
    )]
    async fn timer_list(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<TimerListArgs>,
    ) -> CallToolResult {
        let mut registry = self.registry.lock().await;
        let (project, _) = match scoped_project(&mut registry, &parts, args.project_id) {
            Ok(scoped) => scoped,
            Err(error) => return failure("project_scope_error", error),
        };
        let limit = args
            .limit
            .unwrap_or(DEFAULT_TIMER_LIST_LIMIT)
            .clamp(1, MAX_TIMER_LIST_LIMIT);
        match TimerService::new(&mut registry).list(project.id, limit, now_millis()) {
            Ok(timers) => success(json!({ "timers": timers })),
            Err(error) => timer_failure(error),
        }
    }
}

impl WorkmanMcp {
    async fn set_idle_timer(
        &self,
        parts: &Parts,
        args: IdleTimerArgs,
        kind: TimerKind,
    ) -> CallToolResult {
        let max_wait_ms = match i64::try_from(args.max_wait_ms) {
            Ok(max_wait_ms) => max_wait_ms,
            Err(_) => return timer_failure(TimerError::InvalidMaxWait),
        };
        let mut registry = self.registry.lock().await;
        let (project, actor) = match scoped_project(&mut registry, parts, args.project_id) {
            Ok(scoped) => scoped,
            Err(error) => return failure("project_scope_error", error),
        };
        let delivery =
            match delivery_process(&mut registry, &project, &actor, args.delivery_process_id) {
                Ok(process) => process,
                Err((code, message)) => return failure(code, message),
            };
        let watch_process_ids =
            match resolve_watch_processes(&mut registry, &project, args.processes) {
                Ok(process_ids) => process_ids,
                Err((code, message)) => return failure(code, message),
            };
        match TimerService::new(&mut registry).set_idle(
            actor.id,
            delivery.id,
            args.body,
            kind,
            watch_process_ids,
            max_wait_ms,
            now_millis(),
        ) {
            Ok(IdleTimerOutcome::Created(timer)) => {
                self.timer_events.publish(TimerLifecycleEvent::for_timer(
                    TimerLifecycleKind::Created,
                    project.id,
                    timer.clone(),
                    timer.timer.created_at,
                    None,
                ));
                success(json!({
                    "project_id": project.id,
                    "already_satisfied": false,
                    "delivered_immediately": false,
                    "timer": timer,
                }))
            }
            Ok(IdleTimerOutcome::AlreadySatisfied {
                watch_process_ids,
                delivery_process_id,
                delivered_at,
            }) => {
                for kind in [TimerLifecycleKind::Fired, TimerLifecycleKind::Delivered] {
                    self.timer_events.publish(TimerLifecycleEvent::immediate(
                        kind,
                        project.id,
                        delivery_process_id,
                        delivered_at,
                    ));
                }
                success(json!({
                    "project_id": project.id,
                    "already_satisfied": true,
                    "delivered_immediately": true,
                    "delivery_process_id": delivery_process_id,
                    "delivered_at": delivered_at,
                    "timer": null,
                    "watch_process_ids": watch_process_ids,
                }))
            }
            Err(error) => timer_failure(error),
        }
    }

    async fn change_timer_pause(
        &self,
        parts: &Parts,
        args: TimerTargetArgs,
        pause: bool,
    ) -> CallToolResult {
        let mut registry = self.registry.lock().await;
        let (project, actor) = match scoped_project(&mut registry, parts, args.project_id) {
            Ok(scoped) => scoped,
            Err(error) => return failure("project_scope_error", error),
        };
        let now = now_millis();
        let result = if pause {
            TimerService::new(&mut registry).pause(&actor.id, project.id, args.timer_id, now)
        } else {
            TimerService::new(&mut registry).resume(&actor.id, project.id, args.timer_id, now)
        };
        match result {
            Ok(timer) => {
                self.timer_events.publish(TimerLifecycleEvent::for_timer(
                    if pause {
                        TimerLifecycleKind::Paused
                    } else {
                        TimerLifecycleKind::Resumed
                    },
                    project.id,
                    timer.clone(),
                    now,
                    None,
                ));
                success(json!({ "project_id": project.id, "timer": timer }))
            }
            Err(error) => timer_failure(error),
        }
    }
}

type TargetError = (&'static str, String);

fn delivery_process(
    registry: &mut ProcessRegistry,
    project: &Project,
    actor: &Actor,
    explicit_process_id: Option<ProcessId>,
) -> Result<Process, TargetError> {
    let process_id = explicit_process_id.or(actor.process_id).ok_or_else(|| {
        (
            "delivery_process_required",
            "delivery_process_id is required when the MCP session has no owning process".into(),
        )
    })?;
    let process = registry
        .get(process_id)
        .map_err(|error| (error.code(), error.to_string()))?;
    if process.project_id != project.id {
        return Err((
            "project_scope_error",
            format!(
                "agent identities are scoped to project {}; delivery process {} belongs to project {}",
                project.id, process.id, process.project_id
            ),
        ));
    }
    if process.kind != ProcessKind::Agent {
        return Err((
            "delivery_process_not_agent",
            format!("delivery process {} is not an agent", process.id),
        ));
    }
    Ok(process)
}

fn resolve_watch_processes(
    registry: &mut ProcessRegistry,
    project: &Project,
    targets: Vec<ProcessReference>,
) -> Result<Vec<ProcessId>, TargetError> {
    let processes = registry
        .list(Some(project.id))
        .map_err(|error| (error.code(), error.to_string()))?;
    let mut resolved = BTreeSet::new();
    for target in targets {
        let (process_id, process_name) = match target {
            ProcessReference::Id(process_id) => (Some(process_id), None),
            ProcessReference::Name(process_name) => (None, Some(process_name)),
            ProcessReference::Target(target) => {
                if target.process_id.is_some() && target.process_name.is_some() {
                    return Err((
                        "ambiguous_process_target",
                        "watch target must contain process_id or process_name, not both".into(),
                    ));
                }
                (target.process_id, target.process_name)
            }
        };
        let process = if let Some(process_id) = process_id {
            if let Ok(process) = registry.get(process_id)
                && process.project_id != project.id
            {
                return Err((
                    "project_scope_error",
                    format!(
                        "agent identities are scoped to project {}; watched process {} belongs to project {}",
                        project.id, process.id, process.project_id
                    ),
                ));
            }
            processes.iter().find(|process| process.id == process_id)
        } else if let Some(process_name) = process_name.as_deref() {
            processes
                .iter()
                .find(|process| process.name == process_name)
                .or_else(|| {
                    parse_process_id(process_name)
                        .and_then(|id| processes.iter().find(|process| process.id == id))
                })
        } else {
            return Err((
                "process_target_required",
                "watch target must contain process_id or process_name".into(),
            ));
        };
        let process = process.ok_or_else(|| {
            (
                "process_not_found",
                format!("watched process was not found in project {}", project.id),
            )
        })?;
        resolved.insert(process.id);
    }
    Ok(resolved.into_iter().collect())
}

fn parse_process_id(value: &str) -> Option<ProcessId> {
    value.parse().ok().or_else(|| {
        value
            .rsplit_once("--")
            .and_then(|(_, suffix)| suffix.parse().ok())
    })
}

fn timer_failure(error: TimerError) -> CallToolResult {
    failure(error.code(), error.to_string())
}
