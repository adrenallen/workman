//! MCP Git-worktree discovery and lifecycle tools.

use std::{collections::BTreeMap, path::PathBuf};

use awm_core::ProjectId;
use axum::http::request::Parts;
use rmcp::{
    handler::server::{tool::Extension, wrapper::Parameters},
    model::CallToolResult,
    schemars, tool, tool_router,
};
use serde::Deserialize;

use super::{AwmMcp, failure, scoped_project, success};
use crate::worktrees::{self, AdoptWorktree, CreateWorktree, RemoveWorktree, WorktreeError};

#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
struct WorktreeListArgs {
    /// A project in the repository to inspect. Otherwise uses normal MCP project scope.
    #[serde(default)]
    project_id: Option<ProjectId>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct WorktreeCreateArgs {
    /// A project in the source repository. Otherwise uses normal MCP project scope.
    #[serde(default)]
    project_id: Option<ProjectId>,
    /// Exact branch name. The branch is never slugged or shortened.
    branch: String,
    /// Starting ref for a new branch. Omit when checking out an existing branch.
    #[serde(default)]
    from_ref: Option<String>,
    /// Optional repository-specific managed root override.
    #[serde(default)]
    managed_root: Option<String>,
    /// Optional generic preferences remembered for this repository.
    #[serde(default)]
    preferences: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct WorktreeAdoptArgs {
    /// Existing Git worktree (or a directory inside it) to register without moving it.
    path: String,
    /// Optional generic preferences remembered for this repository.
    #[serde(default)]
    preferences: BTreeMap<String, String>,
}

#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
struct WorktreeRemoveArgs {
    /// Managed worktree project to remove. Otherwise uses normal MCP project scope.
    #[serde(default)]
    project_id: Option<ProjectId>,
    /// Required before deleting the linked worktree and unregistering its awm project.
    #[serde(default)]
    confirm_remove: bool,
    /// Also required when the worktree project has running processes.
    #[serde(default)]
    confirm_stop_running: bool,
    /// Permit deleting local changes only with a matching confirm_branch value.
    #[serde(default)]
    force_dirty: bool,
    /// Must exactly match the branch when force_dirty=true on a dirty worktree.
    #[serde(default)]
    confirm_branch: Option<String>,
}

#[tool_router(router = worktree_tool_router, vis = "pub(crate)")]
impl AwmMcp {
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
        match worktrees::list_for_project(&self.registry, project_id).await {
            Ok(list) => success(list),
            Err(error) => worktree_failure(error),
        }
    }

    #[tool(
        description = "Create or check out a branch as a managed linked worktree and register it as an awm project named <repo>: <branch>"
    )]
    async fn worktree_create(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<WorktreeCreateArgs>,
    ) -> CallToolResult {
        let project_id = match scoped_project_id(self, &parts, args.project_id).await {
            Ok(project_id) => project_id,
            Err(result) => return result,
        };
        match worktrees::create(
            &self.registry,
            CreateWorktree {
                source_project_id: project_id,
                branch: args.branch,
                from_ref: args.from_ref,
                managed_root: args.managed_root.map(PathBuf::from),
                preferences: args.preferences,
            },
        )
        .await
        {
            Ok(created) => success(created),
            Err(error) => worktree_failure(error),
        }
    }

    #[tool(
        description = "Register an existing Git worktree as an adopted awm project without creating a branch or moving files"
    )]
    async fn worktree_adopt(
        &self,
        Parameters(args): Parameters<WorktreeAdoptArgs>,
    ) -> CallToolResult {
        match worktrees::adopt(
            &self.registry,
            AdoptWorktree {
                path: PathBuf::from(args.path),
                preferences: args.preferences,
            },
        )
        .await
        {
            Ok(adopted) => success(adopted),
            Err(error) => worktree_failure(error),
        }
    }

    #[tool(
        description = "Remove an awm/SWM-managed linked worktree and unregister its project while preserving the Git branch; adopted or foreign worktrees are refused"
    )]
    async fn worktree_remove(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<WorktreeRemoveArgs>,
    ) -> CallToolResult {
        let project_id = match scoped_project_id(self, &parts, args.project_id).await {
            Ok(project_id) => project_id,
            Err(result) => return result,
        };
        match worktrees::remove(
            &self.registry,
            RemoveWorktree {
                project_id,
                confirm_remove: args.confirm_remove,
                confirm_stop_running: args.confirm_stop_running,
                force_dirty: args.force_dirty,
                confirm_branch: args.confirm_branch,
            },
        )
        .await
        {
            Ok(removed) => success(removed),
            Err(error) => worktree_failure(error),
        }
    }
}

async fn scoped_project_id(
    service: &AwmMcp,
    parts: &Parts,
    project_id: Option<ProjectId>,
) -> Result<ProjectId, CallToolResult> {
    let mut registry = service.registry.lock().await;
    scoped_project(&mut registry, parts, project_id)
        .map(|(project, _)| project.id)
        .map_err(|error| failure("project_scope_error", error))
}

fn worktree_failure(error: WorktreeError) -> CallToolResult {
    failure(error.code(), error.to_string())
}
