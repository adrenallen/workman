//! MCP tools for project-scoped advisory lease locks.

use axum::http::request::Parts;
use rmcp::{
    handler::server::{tool::Extension, wrapper::Parameters},
    model::CallToolResult,
    schemars, tool, tool_router,
};
use serde::Deserialize;
use serde_json::json;
use workman_core::{
    LeaseView, LockService, LockServiceError, MAX_LOCK_LEASE_TTL_MS, ProjectId, Store,
};

use super::{WorkmanMcp, failure, now_millis, scoped_project, success};

const MAX_LOCK_LEASE_TTL_SECONDS: i64 = MAX_LOCK_LEASE_TTL_MS / 1_000;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct LockArgs {
    #[serde(default)]
    project_id: Option<ProjectId>,
    lock_key: String,
    action: LockAction,
    /// Required for action=acquire.
    #[serde(default)]
    lease_ttl_seconds: Option<i64>,
}

#[derive(Debug, Clone, Copy, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
enum LockAction {
    Acquire,
    Release,
    Status,
}

#[tool_router(router = lock_tool_router, vis = "pub(crate)")]
impl WorkmanMcp {
    #[tool(description = "Acquire, release, or inspect a project-scoped lease lock")]
    async fn lock(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<LockArgs>,
    ) -> CallToolResult {
        let mut registry = self.registry.lock().await;
        let (project, actor) = match scoped_project(&mut registry, &parts, args.project_id) {
            Ok(scoped) => scoped,
            Err(error) => return failure("project_scope_error", error),
        };
        match args.action {
            LockAction::Acquire => {
                let Some(lease_ttl_seconds) = args.lease_ttl_seconds else {
                    return failure(
                        "invalid_params",
                        "lease_ttl_seconds is required for action=acquire",
                    );
                };
                if !(1..=MAX_LOCK_LEASE_TTL_SECONDS).contains(&lease_ttl_seconds) {
                    return lock_failure(None, LockServiceError::InvalidLeaseTtl);
                }
                let Some(lease_ttl_ms) = lease_ttl_seconds.checked_mul(1_000) else {
                    return lock_failure(None, LockServiceError::InvalidLeaseTtl);
                };
                match LockService::new(registry.store()).acquire(
                    project.id,
                    &args.lock_key,
                    &actor.id,
                    lease_ttl_ms,
                    now_millis(),
                ) {
                    Ok(lease) => success(json!({
                        "acquired": true,
                        "lease": readable_lease(registry.store(), lease),
                    })),
                    Err(error) => lock_failure(Some(registry.store()), error),
                }
            }
            LockAction::Release => match LockService::new(registry.store()).release(
                project.id,
                &args.lock_key,
                &actor.id,
                now_millis(),
            ) {
                Ok(released) => success(json!({
                    "project_id": project.id,
                    "lock_key": args.lock_key,
                    "released": released,
                })),
                Err(error) => lock_failure(Some(registry.store()), error),
            },
            LockAction::Status => {
                match LockService::new(registry.store()).status(
                    project.id,
                    &args.lock_key,
                    now_millis(),
                ) {
                    Ok(lease) => success(json!({
                        "lease": lease.map(|lease| readable_lease(registry.store(), lease)),
                    })),
                    Err(error) => lock_failure(Some(registry.store()), error),
                }
            }
        }
    }
}

fn readable_lease(store: &Store, lease: LeaseView) -> serde_json::Value {
    json!({
        "project_id": lease.project_id,
        "lock_key": lease.lock_key,
        "owner_actor": store.ownership_display_label(
            &lease.owner_actor_id,
            lease.owner_process_id,
        ),
        "owner_process_id": lease.owner_process_id,
        "acquired_at": lease.acquired_at,
        "expires_at": lease.expires_at,
    })
}

fn lock_failure(store: Option<&Store>, error: LockServiceError) -> CallToolResult {
    let message = match (&error, store) {
        (
            LockServiceError::Held {
                lock_key,
                owner_actor_id,
                expires_at,
            },
            Some(store),
        ) => format!(
            "lock {lock_key:?} is held by {} until {expires_at}",
            store.ownership_display_label(owner_actor_id, None)
        ),
        (
            LockServiceError::NotOwned {
                lock_key,
                owner_actor_id,
            },
            Some(store),
        ) => format!(
            "lock {lock_key:?} is owned by {}, not this process or session",
            store.ownership_display_label(owner_actor_id, None)
        ),
        _ => error.to_string(),
    };
    failure(error.code(), message)
}
