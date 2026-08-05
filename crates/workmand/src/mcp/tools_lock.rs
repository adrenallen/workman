//! MCP tools for project-scoped advisory lease locks.

use axum::http::request::Parts;
use rmcp::{
    handler::server::{tool::Extension, wrapper::Parameters},
    model::CallToolResult,
    schemars, tool, tool_router,
};
use serde::Deserialize;
use serde_json::json;
use workman_core::{LockService, LockServiceError, MAX_LOCK_LEASE_TTL_MS, ProjectId};

use super::{WorkmanMcp, failure, now_millis, scoped_project, success};

const MAX_LOCK_LEASE_TTL_SECONDS: i64 = MAX_LOCK_LEASE_TTL_MS / 1_000;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct LockAcquireArgs {
    #[serde(default)]
    project_id: Option<ProjectId>,
    lock_key: String,
    lease_ttl_seconds: i64,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct LockTargetArgs {
    #[serde(default)]
    project_id: Option<ProjectId>,
    lock_key: String,
}

#[tool_router(router = lock_tool_router, vis = "pub(crate)")]
impl WorkmanMcp {
    #[tool(description = "Try to acquire a project-scoped lease lock without blocking")]
    async fn lock_acquire(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<LockAcquireArgs>,
    ) -> CallToolResult {
        if !(1..=MAX_LOCK_LEASE_TTL_SECONDS).contains(&args.lease_ttl_seconds) {
            return lock_failure(LockServiceError::InvalidLeaseTtl);
        }
        let lease_ttl_ms = match args.lease_ttl_seconds.checked_mul(1_000) {
            Some(ttl) => ttl,
            None => return lock_failure(LockServiceError::InvalidLeaseTtl),
        };
        let mut registry = self.registry.lock().await;
        let (project, actor) = match scoped_project(&mut registry, &parts, args.project_id) {
            Ok(scoped) => scoped,
            Err(error) => return failure("project_scope_error", error),
        };
        match LockService::new(registry.store()).acquire(
            project.id,
            &args.lock_key,
            &actor.id,
            lease_ttl_ms,
            now_millis(),
        ) {
            Ok(lease) => success(json!({ "acquired": true, "lease": lease })),
            Err(error) => lock_failure(error),
        }
    }

    #[tool(description = "Release a live lease lock owned by this MCP actor")]
    async fn lock_release(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<LockTargetArgs>,
    ) -> CallToolResult {
        let mut registry = self.registry.lock().await;
        let (project, actor) = match scoped_project(&mut registry, &parts, args.project_id) {
            Ok(scoped) => scoped,
            Err(error) => return failure("project_scope_error", error),
        };
        match LockService::new(registry.store()).release(
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
            Err(error) => lock_failure(error),
        }
    }

    #[tool(description = "Return the current live state of one project-scoped lease lock")]
    async fn lock_status(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<LockTargetArgs>,
    ) -> CallToolResult {
        let mut registry = self.registry.lock().await;
        let (project, _) = match scoped_project(&mut registry, &parts, args.project_id) {
            Ok(scoped) => scoped,
            Err(error) => return failure("project_scope_error", error),
        };
        match LockService::new(registry.store()).status(project.id, &args.lock_key, now_millis()) {
            Ok(lease) => success(json!({ "lease": lease })),
            Err(error) => lock_failure(error),
        }
    }
}

fn lock_failure(error: LockServiceError) -> CallToolResult {
    failure(error.code(), error.to_string())
}
