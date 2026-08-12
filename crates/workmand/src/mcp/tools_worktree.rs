//! MCP Git-worktree discovery and lifecycle tools.

use std::collections::BTreeMap;

use axum::http::request::Parts;
use rmcp::{
    handler::server::{tool::Extension, wrapper::Parameters},
    model::CallToolResult,
    schemars, tool, tool_router,
};
use serde::Deserialize;
use workman_core::ProjectId;

use super::{WorkmanMcp, ensure_actor, failure, process_project_id, scoped_project, success};
use crate::worktrees::{self, EnvPortPolicy, RemoveWorktree, WorktreeError};

#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
struct WorktreeListArgs {
    /// A project in the repository to inspect. Otherwise uses normal MCP project scope.
    #[serde(default)]
    project_id: Option<ProjectId>,
    /// Bypass the five-minute daemon cache and ask GitHub for fresh PR/check status.
    #[serde(default)]
    refresh_pull_requests: bool,
}

#[allow(dead_code)]
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
    /// Copy or skip an ignored source .env. Required once when an .env exists and no repository preference is stored.
    #[serde(default)]
    env_policy: Option<EnvPortPolicy>,
    /// Persist env_policy for this repository so future creates do not ask again.
    #[serde(default)]
    remember_env_policy: bool,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct WorktreeForkArgs {
    /// Selected source worktree. The new branch starts at this worktree's exact HEAD.
    #[serde(default)]
    project_id: Option<ProjectId>,
    /// Exact new branch name.
    branch: String,
    /// Optional repository-specific managed root override.
    #[serde(default)]
    managed_root: Option<String>,
    #[serde(default)]
    preferences: BTreeMap<String, String>,
    #[serde(default)]
    env_policy: Option<EnvPortPolicy>,
    #[serde(default)]
    remember_env_policy: bool,
}

#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
struct WorktreeForgetEnvArgs {
    /// A project in the repository whose remembered .env choice should be cleared.
    #[serde(default)]
    project_id: Option<ProjectId>,
}

#[allow(dead_code)]
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
    /// Project to remove. Otherwise uses normal MCP project scope.
    #[serde(default)]
    project_id: Option<ProjectId>,
    /// Required before unregistering the worktree project from Workman.
    #[serde(default)]
    confirm_remove: bool,
    /// Also required when the worktree project has running processes.
    #[serde(default)]
    confirm_stop_running: bool,
    /// Also delete the exact local project directory. Linked worktrees use Git removal and local metadata pruning. Defaults to false.
    #[serde(default)]
    delete_from_disk: bool,
    /// Permit deleting dirty, untracked, or ignored local paths, unpublished commits, or a primary checkout with dependent worktrees.
    #[serde(default)]
    force_dirty: bool,
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
        description = "Create a managed worktree project through user control; project-jailed agent identities are rejected"
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
        failure(
            "project_scope_error",
            format!(
                "agent identities are scoped to project {project_id}; creating a worktree project is outside that scope"
            ),
        )
    }

    #[tool(
        description = "Fork a managed worktree project through user control; project-jailed agent identities are rejected"
    )]
    async fn worktree_fork(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<WorktreeForkArgs>,
    ) -> CallToolResult {
        let project_id = match scoped_project_id(self, &parts, args.project_id).await {
            Ok(project_id) => project_id,
            Err(result) => return result,
        };
        failure(
            "project_scope_error",
            format!(
                "agent identities are scoped to project {project_id}; forking a worktree project is outside that scope"
            ),
        )
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

    #[tool(
        description = "Adopt a Git worktree project through user control; project-jailed agent identities are rejected"
    )]
    async fn worktree_adopt(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(_args): Parameters<WorktreeAdoptArgs>,
    ) -> CallToolResult {
        let mut registry = self.registry.lock().await;
        let (actor, _) = match ensure_actor(&mut registry, &parts) {
            Ok(identity) => identity,
            Err(error) => return failure("identity_error", error),
        };
        match process_project_id(&registry, &actor) {
            Ok(Some(project_id)) => failure(
                "project_scope_error",
                format!(
                    "agent identities are scoped to project {project_id}; adopting another worktree project is outside that scope"
                ),
            ),
            Ok(None) => failure(
                "identity_required",
                "MCP session has no process identity; call identify_session before project-scoped actions",
            ),
            Err(error) => failure("project_scope_error", error),
        }
    }

    #[tool(
        description = "Remove any project from Workman while preserving local files by default; set delete_from_disk=true for guarded local-only deletion. Linked worktrees use Git removal and pruning, local branches are kept, and no remote operation is performed"
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
                delete_from_disk: args.delete_from_disk,
                force_dirty: args.force_dirty,
                confirm_branch: None,
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
    failure(error.code(), error.to_string())
}
