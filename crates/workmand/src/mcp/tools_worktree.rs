//! MCP Git-worktree discovery and lifecycle tools.

use axum::http::request::Parts;
use rmcp::{
    handler::server::{tool::Extension, wrapper::Parameters},
    model::CallToolResult,
    schemars, tool, tool_router,
};
use serde::Deserialize;
use serde_json::json;
use workman_core::ProjectId;

use super::{WorkmanMcp, failure, scoped_project, success};
use crate::worktrees::{self, WorktreeError};

#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
struct WorktreeListArgs {
    /// A project in the repository to inspect. Otherwise uses normal MCP project scope.
    #[serde(default)]
    project_id: Option<ProjectId>,
    /// Bypass the five-minute daemon cache and ask GitHub for fresh PR/check status.
    #[serde(default)]
    refresh_pull_requests: bool,
}

#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
struct WorktreeForgetEnvArgs {
    /// A project in the repository whose remembered .env choice should be cleared.
    #[serde(default)]
    project_id: Option<ProjectId>,
}

#[tool_router(router = worktree_tool_router, vis = "pub(crate)")]
impl WorkmanMcp {
    #[tool(
        description = "List every Git worktree for the effective repository, including registration, branch, cleanliness, ownership, and import/remove capabilities"
    )]
    async fn worktree_list(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<WorktreeListArgs>,
    ) -> CallToolResult {
        let project_id = match scoped_project_id(self, &parts, args.project_id).await {
            Ok(project_id) => project_id,
            Err(result) => return result,
        };
        match worktrees::list_for_project_refresh(
            &self.registry,
            project_id,
            args.refresh_pull_requests,
        )
        .await
        {
            Ok(mut list) => {
                list.worktrees
                    .retain(|worktree| worktree.project_id == Some(project_id));
                success(list)
            }
            Err(error) => worktree_failure(error),
        }
    }

    #[tool(
        description = "Forget the repository's remembered .env copy/skip choice so the next create asks again"
    )]
    async fn worktree_env_forget(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<WorktreeForgetEnvArgs>,
    ) -> CallToolResult {
        let project_id = match scoped_project_id(self, &parts, args.project_id).await {
            Ok(project_id) => project_id,
            Err(result) => return result,
        };
        match worktrees::forget_env_preference(&self.registry, project_id).await {
            Ok(receipt) => success(receipt),
            Err(error) => worktree_failure(error),
        }
    }

    #[tool(
        description = "Check Git, GitHub CLI authentication, Laravel Herd parking, and managed-root readiness with fix hints"
    )]
    async fn worktree_health(&self) -> CallToolResult {
        success(worktrees::health(&self.registry).await)
    }
}

async fn scoped_project_id(
    service: &WorkmanMcp,
    parts: &Parts,
    project_id: Option<ProjectId>,
) -> Result<ProjectId, CallToolResult> {
    let mut registry = service.registry.lock().await;
    scoped_project(&mut registry, parts, project_id)
        .map(|(project, _)| project.id)
        .map_err(|error| failure("project_scope_error", error))
}

fn worktree_failure(error: WorktreeError) -> CallToolResult {
    match error {
        WorktreeError::CreateConflict(conflict) => CallToolResult::structured_error(json!({
            "code": "worktree_create_conflict",
            "message": conflict.to_string(),
            "conflict": conflict,
        })),
        error => failure(error.code(), error.to_string()),
    }
}
