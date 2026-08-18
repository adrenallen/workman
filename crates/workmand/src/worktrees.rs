//! Git worktree discovery and lifecycle management.
//!
//! The behavior intentionally mirrors the standalone SWM tool: a branch is a
//! project, the first porcelain worktree identifies the repository, existing
//! local/remote branches are checked out rather than recreated. Project removal
//! uses the same local-only safety analysis for every registered project type.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    env,
    ffi::OsString,
    fmt, fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process::{Command as StdCommand, Output},
    time::Duration,
};

use rmcp::schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio::{process::Command, time::timeout};
use workman_core::{
    ProcessStatus, Project, ProjectId, ProjectWorktree, Store, StoreError, WorktreeRepository,
};

use crate::{
    RegistryError, SharedProcessRegistry,
    project_titles::normalized_optional_project_title,
    user_config::user_config_path,
    user_environment::UserEnvironmentResolver,
    worktree_integrations::{
        self, HerdView, PullRequestCacheView, PullRequestView, WorktreeHealth,
    },
    worktree_operations::{WorktreeOperationReporter, WorktreeStepId},
};

pub const WORKMAN_WORKTREE_ROOT_ENV: &str = "WORKMAN_WORKTREE_ROOT";
const LEGACY_SWM_ROOT_ENV: &str = "SWM_ROOT";
const SITE_NAME_LIMIT: usize = 63;
const GIT_NETWORK_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug)]
pub enum WorktreeError {
    Store(StoreError),
    Registry(RegistryError),
    Io(io::Error),
    InvalidProject(String),
    InvalidPath(String),
    InvalidBranch(String),
    Git { operation: String, message: String },
    CreateConflict(Box<WorktreeCreateConflict>),
    Conflict(String),
    Confirmation(String),
    Dirty(String),
    Foreign(String),
    EnvPreference(String),
    UnsafeEnvironment(String),
    Integration { code: &'static str, message: String },
}

impl WorktreeError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Store(_) => "store_error",
            Self::Registry(error) => error.code(),
            Self::Io(_) => "io_error",
            Self::InvalidProject(_) => "project_not_found",
            Self::InvalidPath(_) => "invalid_worktree_path",
            Self::InvalidBranch(_) => "invalid_branch",
            Self::Git { .. } => "git_error",
            Self::CreateConflict(_) => "worktree_create_conflict",
            Self::Conflict(_) => "worktree_conflict",
            Self::Confirmation(_) => "confirmation_required",
            Self::Dirty(_) => "dirty_worktree",
            Self::Foreign(_) => "foreign_worktree",
            Self::EnvPreference(_) => "env_preference_required",
            Self::UnsafeEnvironment(_) => "unsafe_env_file",
            Self::Integration { code, .. } => code,
        }
    }
}

impl fmt::Display for WorktreeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(error) => error.fmt(formatter),
            Self::Registry(error) => error.fmt(formatter),
            Self::Io(error) => error.fmt(formatter),
            Self::InvalidProject(message)
            | Self::InvalidPath(message)
            | Self::InvalidBranch(message)
            | Self::Conflict(message)
            | Self::Confirmation(message)
            | Self::Dirty(message)
            | Self::Foreign(message)
            | Self::EnvPreference(message)
            | Self::UnsafeEnvironment(message) => formatter.write_str(message),
            Self::Git { operation, message } => write!(formatter, "{operation}: {message}"),
            Self::CreateConflict(conflict) => conflict.fmt(formatter),
            Self::Integration { message, .. } => formatter.write_str(message),
        }
    }
}

impl std::error::Error for WorktreeError {}

impl From<StoreError> for WorktreeError {
    fn from(error: StoreError) -> Self {
        Self::Store(error)
    }
}

impl From<RegistryError> for WorktreeError {
    fn from(error: RegistryError) -> Self {
        Self::Registry(error)
    }
}

impl From<io::Error> for WorktreeError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

pub type WorktreeResult<T> = Result<T, WorktreeError>;

#[derive(Clone, Debug, Serialize)]
pub struct ProjectEnvelope {
    #[serde(flatten)]
    pub project: Project,
    pub repository_id: Option<i64>,
    pub repository_root: Option<String>,
    pub parent_project_id: Option<ProjectId>,
    pub branch: Option<String>,
    pub worktree_managed: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct RepositoryView {
    pub id: i64,
    pub name: String,
    pub root_path: String,
    pub managed_root: String,
    pub preferences: BTreeMap<String, String>,
    pub herd: HerdView,
}

#[derive(Clone, Debug, Serialize)]
pub struct WorktreeEntry {
    pub project_id: Option<ProjectId>,
    pub parent_project_id: Option<ProjectId>,
    pub path: String,
    pub branch: String,
    pub head: String,
    pub kind: &'static str,
    pub status: &'static str,
    pub managed: bool,
    pub registered: bool,
    pub can_adopt: bool,
    pub can_remove: bool,
    pub delete_safety: Option<WorktreeDeleteSafety>,
    pub locked: bool,
    pub prunable: bool,
    pub site_url: Option<String>,
    pub pull_request: Option<PullRequestView>,
    pub pull_requests: Vec<PullRequestView>,
}

#[derive(Clone, Debug, Serialize)]
pub struct WorktreeDeleteSafety {
    pub dirty_files: usize,
    pub untracked_files: usize,
    pub dirty_paths: Vec<String>,
    pub ignored_files: usize,
    pub ignored_paths: Vec<String>,
    pub unpushed_commits: usize,
    pub unpushed_subjects: Vec<String>,
    pub unmerged_commits: usize,
    pub unmerged_subjects: Vec<String>,
    pub upstream: Option<String>,
    pub push_target: Option<String>,
    pub merge_target: String,
    pub dependent_worktrees: Vec<String>,
    pub requires_force: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct WorktreeList {
    pub repository: RepositoryView,
    pub worktrees: Vec<WorktreeEntry>,
    pub pull_requests: PullRequestCacheView,
}

#[derive(Clone, Debug, Serialize)]
pub struct OriginBranchList {
    pub repository_id: i64,
    pub branches: Vec<String>,
    pub options: Vec<WorktreeBranchOption>,
    pub default_ref: Option<String>,
    pub ref_options: Vec<WorktreeRefOption>,
}

#[derive(Clone, Debug, Serialize)]
pub struct WorktreeBranchOption {
    pub name: String,
    pub source: &'static str,
}

#[derive(Clone, Debug, Serialize)]
pub struct WorktreeRefOption {
    pub name: String,
    pub source: &'static str,
}

#[derive(Clone, Debug, Serialize)]
pub struct WorktreeRefValidation {
    pub repository_id: i64,
    pub requested_ref: String,
    pub resolved_ref: String,
    pub commit: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[schemars(crate = "rmcp::schemars")]
#[serde(rename_all = "snake_case")]
pub enum WorktreeCreateResolution {
    UseExistingBranch,
    LoadFromRemote,
}

impl WorktreeCreateResolution {
    fn action(self) -> &'static str {
        match self {
            Self::UseExistingBranch => "use_existing_branch",
            Self::LoadFromRemote => "load_from_remote",
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct WorktreeCreateConflict {
    pub kind: &'static str,
    pub branch: String,
    pub path: String,
    pub project_id: Option<ProjectId>,
    pub message: String,
    pub actions: Vec<&'static str>,
}

impl fmt::Display for WorktreeCreateConflict {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} Available actions: {}.",
            self.message,
            self.actions.join(", ")
        )
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct WorktreeCreateCheck {
    pub status: &'static str,
    pub branch: String,
    pub destination: String,
    pub conflict: Option<WorktreeCreateConflict>,
}

#[derive(Clone, Debug)]
pub struct InspectWorktreeCreate {
    pub source_project_id: ProjectId,
    pub branch: String,
    pub from_ref: Option<String>,
    pub managed_root: Option<PathBuf>,
    pub resolution: Option<WorktreeCreateResolution>,
}

#[derive(Clone, Debug, Serialize)]
pub struct WorktreeMutation {
    pub repository: RepositoryView,
    pub project: ProjectEnvelope,
    pub worktree: WorktreeEntry,
    pub environment: Option<EnvironmentPortResult>,
}

#[derive(Clone, Debug, Serialize)]
pub struct WorktreeRemoval {
    pub project_id: ProjectId,
    pub path: String,
    pub branch: String,
    pub removed: bool,
    pub project_unregistered: bool,
    pub deleted_from_disk: bool,
    pub metadata_pruned: bool,
    pub branch_kept: bool,
    pub files_removed: bool,
    pub files_untouched: bool,
    pub registration_issue: Option<String>,
    pub selected_project_id: Option<ProjectId>,
}

#[derive(Clone, Debug, Serialize)]
pub struct WorktreePreferenceMutation {
    pub repository_id: i64,
    pub key: &'static str,
    pub cleared: bool,
}

#[derive(Clone, Debug)]
pub struct CreateWorktree {
    pub source_project_id: ProjectId,
    pub branch: String,
    pub display_name: Option<String>,
    pub from_ref: Option<String>,
    pub resolution: Option<WorktreeCreateResolution>,
    pub managed_root: Option<PathBuf>,
    pub preferences: BTreeMap<String, String>,
    pub env_policy: Option<EnvPortPolicy>,
    pub remember_env_policy: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[schemars(crate = "rmcp::schemars")]
#[serde(rename_all = "snake_case")]
pub enum EnvPortPolicy {
    Copy,
    Skip,
}

impl EnvPortPolicy {
    fn preference(self) -> &'static str {
        match self {
            Self::Copy => "yes",
            Self::Skip => "no",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Copy => "copy",
            Self::Skip => "skip",
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct EnvironmentPortResult {
    pub source_present: bool,
    pub policy: &'static str,
    pub copied: bool,
    pub remembered: bool,
    pub app_name_rewritten: bool,
    pub app_url_rewritten: bool,
}

#[derive(Clone, Debug)]
pub struct ForkWorktree {
    pub source_project_id: ProjectId,
    pub branch: String,
    pub display_name: Option<String>,
    pub resolution: Option<WorktreeCreateResolution>,
    pub managed_root: Option<PathBuf>,
    pub preferences: BTreeMap<String, String>,
    pub env_policy: Option<EnvPortPolicy>,
    pub remember_env_policy: bool,
}

#[derive(Clone, Debug)]
pub struct AdoptWorktree {
    pub path: PathBuf,
    pub display_name: Option<String>,
    pub preferences: BTreeMap<String, String>,
}

#[derive(Clone, Debug)]
pub struct RemoveWorktree {
    pub project_id: ProjectId,
    pub confirm_remove: bool,
    pub confirm_stop_running: bool,
    pub delete_from_disk: bool,
    pub force_dirty: bool,
    pub confirm_branch: Option<String>,
}

#[derive(Clone, Debug)]
struct RepositorySnapshot {
    root_path: PathBuf,
    name: String,
    worktrees: Vec<GitWorktree>,
}

#[derive(Clone, Debug)]
struct GitWorktree {
    path: PathBuf,
    head: String,
    branch: Option<String>,
    bare: bool,
    locked: bool,
    prunable: bool,
}

#[derive(Clone, Debug)]
struct VerifiedDeleteTarget {
    path: PathBuf,
    repository_root: Option<PathBuf>,
    branch: String,
    kind: DeleteTargetKind,
    dependent_worktrees: Vec<String>,
    recovering_partial_removal: bool,
}

#[derive(Clone, Debug)]
struct DeleteRecoveryHint {
    repository_root: PathBuf,
    branch: String,
    linked_worktree: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RegistrationIssueKind {
    Duplicate,
    MissingPath,
    ParentUnavailable,
    NotRecordedByParent,
}

#[derive(Clone, Debug)]
struct RegistrationIssue {
    kind: RegistrationIssueKind,
    message: String,
}

impl RegistrationIssue {
    fn remove_everywhere(&self) -> bool {
        matches!(
            self.kind,
            RegistrationIssueKind::Duplicate | RegistrationIssueKind::MissingPath
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DeleteTargetKind {
    LinkedWorktree,
    PrimaryCheckout,
    Folder,
}

#[derive(Clone, Debug)]
struct EnvironmentPlan {
    source: Option<PathBuf>,
    contents: Option<Vec<u8>>,
    permissions: Option<fs::Permissions>,
    policy: Option<EnvPortPolicy>,
    remembered: bool,
}

/// Root convention shared with SWM, with a Workman-native override.
pub fn default_worktree_root() -> PathBuf {
    if let Some(root) = env::var_os(WORKMAN_WORKTREE_ROOT_ENV).filter(|value| !value.is_empty()) {
        return absolute_path(PathBuf::from(root));
    }
    if let Some(root) = env::var_os(LEGACY_SWM_ROOT_ENV).filter(|value| !value.is_empty()) {
        return absolute_path(PathBuf::from(root));
    }
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let herd = home.join("Herd");
    if herd.is_dir() {
        herd
    } else {
        home.join("worktrees")
    }
}

/// SWM's DNS-safe folder transform. Branch names themselves are never shortened.
pub fn site_slug(value: &str) -> String {
    let mut result = String::new();
    let mut separator = false;
    for character in value.chars().flat_map(char::to_lowercase) {
        if character.is_ascii_alphanumeric() {
            if separator && !result.is_empty() {
                result.push('-');
            }
            separator = false;
            if result.len() < SITE_NAME_LIMIT {
                result.push(character);
            }
        } else {
            separator = true;
        }
        if result.len() == SITE_NAME_LIMIT {
            break;
        }
    }
    while result.ends_with('-') {
        result.pop();
    }
    result
}

pub fn project_envelope(store: &Store, project: Project) -> WorktreeResult<ProjectEnvelope> {
    let link = store.get_project_worktree(project.id)?;
    let repository = match &link {
        Some(link) => store.get_worktree_repository(link.repository_id)?,
        None => None,
    };
    Ok(ProjectEnvelope {
        repository_id: link.as_ref().map(|link| link.repository_id),
        repository_root: repository.map(|repository| repository.root_path),
        parent_project_id: link.as_ref().and_then(|link| link.parent_project_id),
        branch: link.as_ref().map(|link| link.branch.clone()),
        worktree_managed: link.as_ref().is_some_and(|link| link.managed),
        project,
    })
}

pub fn project_envelopes(
    store: &Store,
    projects: Vec<Project>,
) -> WorktreeResult<Vec<ProjectEnvelope>> {
    projects
        .into_iter()
        .map(|project| project_envelope(store, project))
        .collect()
}

/// Populate metadata for pre-existing SWM/workman projects without touching Git or project rows.
pub fn reconcile_existing_projects(store: &Store) -> WorktreeResult<()> {
    let environment = UserEnvironmentResolver::new(user_config_path())
        .resolve()
        .command_environment();
    reconcile_existing_projects_with_environment(store, &environment)
}

pub(crate) fn reconcile_existing_projects_with_environment(
    store: &Store,
    environment: &BTreeMap<OsString, OsString>,
) -> WorktreeResult<()> {
    let projects = store.list_projects()?;
    for project in &projects {
        if store.get_project_worktree(project.id)?.is_some() {
            continue;
        }
        let Ok(snapshot) = snapshot_sync(Path::new(&project.path), environment) else {
            continue;
        };
        let Some(record) = matching_record(&snapshot, Path::new(&project.path)) else {
            continue;
        };
        let repository = ensure_repository(store, &snapshot, None)?;
        let parent_project_id = projects
            .iter()
            .find(|candidate| same_path(Path::new(&candidate.path), &snapshot.root_path))
            .map(|candidate| candidate.id)
            .filter(|id| *id != project.id);
        let branch = display_branch(record);
        let managed = parent_project_id.is_some()
            && is_swm_managed_path(Path::new(&project.path), &repository.managed_root, &branch);
        store.put_project_worktree(&ProjectWorktree {
            project_id: project.id,
            repository_id: repository.id,
            parent_project_id,
            branch,
            managed,
        })?;
    }

    // A child can be encountered before its main project. Repair parent links
    // after every project has had a chance to create its repository metadata.
    for repository in store.list_worktree_repositories()? {
        let root_project_id = projects
            .iter()
            .find(|project| same_path(Path::new(&project.path), Path::new(&repository.root_path)))
            .map(|project| project.id);
        for mut link in store.list_project_worktrees(repository.id)? {
            let project = projects
                .iter()
                .find(|project| project.id == link.project_id);
            link.parent_project_id = project
                .filter(|project| {
                    !same_path(Path::new(&project.path), Path::new(&repository.root_path))
                })
                .and(root_project_id);
            store.put_project_worktree(&link)?;
        }
    }
    Ok(())
}

pub async fn list_for_project(
    registry: &SharedProcessRegistry,
    project_id: ProjectId,
) -> WorktreeResult<WorktreeList> {
    list_for_project_refresh(registry, project_id, false).await
}

pub async fn list_for_project_refresh(
    registry: &SharedProcessRegistry,
    project_id: ProjectId,
    refresh_pull_requests: bool,
) -> WorktreeResult<WorktreeList> {
    let environment = command_environment(registry).await;
    let (project, repository, registered, links) = {
        let registry = registry.lock().await;
        let project = registry.store().get_project(project_id)?.ok_or_else(|| {
            WorktreeError::InvalidProject(format!("project {project_id} was not found"))
        })?;
        if registry.store().get_project_worktree(project_id)?.is_none() {
            reconcile_existing_projects_with_environment(registry.store(), &environment)?;
        }
        let link = registry
            .store()
            .get_project_worktree(project_id)?
            .ok_or_else(|| {
                WorktreeError::InvalidPath(format!("{} is not a Git worktree", project.path))
            })?;
        let repository = registry
            .store()
            .get_worktree_repository(link.repository_id)?
            .ok_or_else(|| {
                WorktreeError::InvalidPath("worktree repository metadata is missing".into())
            })?;
        let registered = registry.store().list_projects()?;
        let links = registry.store().list_project_worktrees(repository.id)?;
        (project, repository, registered, links)
    };

    let snapshot = snapshot_async(Path::new(&project.path), &environment).await?;
    list_from_snapshot(
        registry,
        repository,
        snapshot,
        registered,
        links,
        refresh_pull_requests,
        &environment,
    )
    .await
}

/// Lists local and origin branches that are not currently checked out in any
/// linked worktree. This is intentionally fetched on demand for the desktop
/// branch picker so normal project/worktree polling never adds remote traffic.
pub async fn origin_branches_for_project(
    registry: &SharedProcessRegistry,
    project_id: ProjectId,
) -> WorktreeResult<OriginBranchList> {
    let environment = command_environment(registry).await;
    let (project, repository_id) = {
        let registry = registry.lock().await;
        let project = registry.store().get_project(project_id)?.ok_or_else(|| {
            WorktreeError::InvalidProject(format!("project {project_id} was not found"))
        })?;
        if registry.store().get_project_worktree(project_id)?.is_none() {
            reconcile_existing_projects_with_environment(registry.store(), &environment)?;
        }
        let link = registry
            .store()
            .get_project_worktree(project_id)?
            .ok_or_else(|| {
                WorktreeError::InvalidPath(format!("{} is not a Git worktree", project.path))
            })?;
        (project, link.repository_id)
    };

    let snapshot = snapshot_async(Path::new(&project.path), &environment).await?;
    let checked_out = snapshot
        .worktrees
        .iter()
        .map(display_branch)
        .collect::<HashSet<_>>();
    let local_output = git_required(
        &snapshot.root_path,
        [
            "for-each-ref",
            "--sort=-committerdate",
            "--format=%(refname:strip=2)",
            "refs/heads",
        ],
        "list local branches",
        &environment,
    )
    .await?;
    let all_local = local_output
        .lines()
        .map(str::trim)
        .filter(|branch| !branch.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let mut local = all_local
        .iter()
        .filter(|branch| !checked_out.contains(branch.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    local.sort();
    local.dedup();

    let mut all_origin = if git_success(
        &snapshot.root_path,
        ["remote", "get-url", "origin"],
        &environment,
    )
    .await?
    {
        let output = git_required(
            &snapshot.root_path,
            ["ls-remote", "--heads", "origin"],
            "list origin branches",
            &environment,
        )
        .await?;
        parse_origin_branches(&output)
    } else {
        Vec::new()
    };
    let default_ref = detect_origin_default_ref(&snapshot.root_path, &environment).await?;
    let mut origin = all_origin.clone();
    origin.retain(|branch| !checked_out.contains(branch));
    origin.sort();
    origin.dedup();

    let local_names = local.iter().cloned().collect::<HashSet<_>>();
    let mut options = local
        .into_iter()
        .map(|name| WorktreeBranchOption {
            name,
            source: "local",
        })
        .collect::<Vec<_>>();
    options.extend(
        origin
            .into_iter()
            .filter(|name| !local_names.contains(name))
            .map(|name| WorktreeBranchOption {
                name,
                source: "origin",
            }),
    );
    let branches = options.iter().map(|option| option.name.clone()).collect();

    let mut ref_options = vec![WorktreeRefOption {
        name: "HEAD".into(),
        source: "current",
    }];
    if let Some(default_ref) = default_ref.as_ref() {
        ref_options.push(WorktreeRefOption {
            name: default_ref.clone(),
            source: "default",
        });
    }
    ref_options.extend(all_local.into_iter().take(8).map(|name| WorktreeRefOption {
        name,
        source: "local",
    }));

    let remote_order = git_optional(
        &snapshot.root_path,
        [
            "for-each-ref",
            "--sort=-committerdate",
            "--format=%(refname:short)",
            "refs/remotes/origin",
        ],
        &environment,
    )
    .await?
    .unwrap_or_default();
    let mut recent_remote = remote_order
        .lines()
        .map(str::trim)
        .filter(|name| !name.is_empty() && *name != "origin/HEAD")
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let known_remote = recent_remote.iter().cloned().collect::<HashSet<_>>();
    all_origin.sort();
    recent_remote.extend(all_origin.into_iter().filter_map(|branch| {
        let name = format!("origin/{branch}");
        (!known_remote.contains(&name)).then_some(name)
    }));
    ref_options.extend(
        recent_remote
            .into_iter()
            .filter(|name| Some(name) != default_ref.as_ref())
            .take(8)
            .map(|name| WorktreeRefOption {
                name,
                source: "remote",
            }),
    );

    let mut seen_refs = HashSet::new();
    ref_options.retain(|option| seen_refs.insert(option.name.clone()));
    Ok(OriginBranchList {
        repository_id,
        branches,
        options,
        default_ref,
        ref_options,
    })
}

pub async fn validate_ref_for_project(
    registry: &SharedProcessRegistry,
    project_id: ProjectId,
    requested_ref: &str,
) -> WorktreeResult<WorktreeRefValidation> {
    let environment = command_environment(registry).await;
    let (project, repository_id) = {
        let registry = registry.lock().await;
        let project = registry.store().get_project(project_id)?.ok_or_else(|| {
            WorktreeError::InvalidProject(format!("project {project_id} was not found"))
        })?;
        if registry.store().get_project_worktree(project_id)?.is_none() {
            reconcile_existing_projects_with_environment(registry.store(), &environment)?;
        }
        let link = registry
            .store()
            .get_project_worktree(project_id)?
            .ok_or_else(|| {
                WorktreeError::InvalidPath(format!("{} is not a Git worktree", project.path))
            })?;
        (project, link.repository_id)
    };
    let snapshot = snapshot_async(Path::new(&project.path), &environment).await?;
    let requested_ref = requested_ref.trim();
    if requested_ref.is_empty() {
        return Err(WorktreeError::InvalidBranch(
            "Enter a branch, tag, or commit to start from.".into(),
        ));
    }
    let resolved_ref = resolve_start_point(&snapshot.root_path, Some(requested_ref), &environment)
        .await
        .map_err(|error| match error {
            WorktreeError::InvalidBranch(_) => WorktreeError::InvalidBranch(format!(
                "Ref {requested_ref:?} was not found in this repository or origin."
            )),
            error => error,
        })?;
    let commit = git_required(
        &snapshot.root_path,
        [
            "rev-parse",
            "--verify",
            format!("{resolved_ref}^{{commit}}").as_str(),
        ],
        "resolve starting ref commit",
        &environment,
    )
    .await?;
    Ok(WorktreeRefValidation {
        repository_id,
        requested_ref: requested_ref.into(),
        resolved_ref,
        commit,
    })
}

pub async fn create(
    registry: &SharedProcessRegistry,
    request: CreateWorktree,
) -> WorktreeResult<WorktreeMutation> {
    create_with_progress(registry, request, None).await
}

pub async fn inspect_create(
    registry: &SharedProcessRegistry,
    request: InspectWorktreeCreate,
) -> WorktreeResult<WorktreeCreateCheck> {
    let command_environment = command_environment(registry).await;
    validate_branch(&request.branch, &command_environment).await?;
    let source_project = {
        let registry = registry.lock().await;
        registry
            .store()
            .get_project(request.source_project_id)?
            .ok_or_else(|| {
                WorktreeError::InvalidProject(format!(
                    "project {} was not found",
                    request.source_project_id
                ))
            })?
    };
    let snapshot = snapshot_async(Path::new(&source_project.path), &command_environment).await?;
    let managed_root = if let Some(managed_root) = request.managed_root {
        absolute_path(managed_root)
    } else {
        let registry = registry.lock().await;
        registry
            .store()
            .get_worktree_repository_by_root(&canonical_display(&snapshot.root_path))?
            .map(|repository| PathBuf::from(repository.managed_root))
            .unwrap_or_else(default_worktree_root)
    };
    let projects = {
        let registry = registry.lock().await;
        registry.store().list_all_projects()?
    };
    inspect_create_snapshot(
        &snapshot,
        &projects,
        &request.branch,
        request.from_ref.as_deref(),
        &managed_root,
        request.resolution,
        &command_environment,
    )
    .await
}

async fn inspect_create_snapshot(
    snapshot: &RepositorySnapshot,
    projects: &[Project],
    branch: &str,
    from_ref: Option<&str>,
    managed_root: &Path,
    resolution: Option<WorktreeCreateResolution>,
    command_environment: &BTreeMap<OsString, OsString>,
) -> WorktreeResult<WorktreeCreateCheck> {
    if let (Some(from_ref), Some(resolution)) = (from_ref, resolution) {
        return Err(WorktreeError::Conflict(format!(
            "branch {branch:?} cannot use {} while also being created from {from_ref:?}; omit the starting ref to choose the existing branch",
            resolution.action()
        )));
    }
    let slug = site_slug(branch);
    if slug.is_empty() {
        return Err(WorktreeError::InvalidBranch(format!(
            "branch {branch:?} has no characters usable in a folder name"
        )));
    }
    let destination = absolute_path(managed_root.to_path_buf()).join(&slug);
    let destination_text = canonical_display(&destination);
    let registered_at = |path: &Path| {
        projects
            .iter()
            .find(|project| project.path == canonical_display(path))
            .or_else(|| {
                projects
                    .iter()
                    .find(|project| same_path(Path::new(&project.path), path))
            })
    };

    if let Some(project) = registered_at(&destination) {
        return Ok(create_conflict_check(
            "registered_project",
            branch,
            &destination,
            Some(project.id),
            format!(
                "{} is already registered as project {}. Open that project or choose a different branch name.",
                destination.display(),
                project.id
            ),
            vec!["open_registered_project", "choose_different_name"],
        ));
    }
    if let Some(record) = snapshot
        .worktrees
        .iter()
        .find(|record| same_path(&record.path, &destination))
    {
        return Ok(create_conflict_check(
            "existing_worktree",
            branch,
            &record.path,
            None,
            format!(
                "A Git worktree already exists at {}. Import it or choose a different branch name.",
                record.path.display()
            ),
            vec!["import_existing_worktree", "choose_different_name"],
        ));
    }
    if let Some(record) = snapshot
        .worktrees
        .iter()
        .find(|record| record.branch.as_deref() == Some(branch))
    {
        if let Some(project) = registered_at(&record.path) {
            return Ok(create_conflict_check(
                "registered_project",
                branch,
                &record.path,
                Some(project.id),
                format!(
                    "Branch {branch:?} is already open in registered project {} at {}.",
                    project.id,
                    record.path.display()
                ),
                vec!["open_registered_project", "choose_different_name"],
            ));
        }
        return Ok(create_conflict_check(
            "existing_worktree",
            branch,
            &record.path,
            None,
            format!(
                "Branch {branch:?} is already checked out at {}. Import that worktree or choose a different branch name.",
                record.path.display()
            ),
            vec!["import_existing_worktree", "choose_different_name"],
        ));
    }
    if destination.exists() {
        return Ok(create_conflict_check(
            "existing_path",
            branch,
            &destination,
            None,
            format!(
                "Destination {} already exists but is not a Git worktree for this repository. Choose a different branch name.",
                destination.display()
            ),
            vec!["choose_different_name"],
        ));
    }

    let state = branch_state(snapshot, branch, command_environment).await?;
    let conflict = match state {
        BranchState::Local if resolution != Some(WorktreeCreateResolution::UseExistingBranch) => {
            Some(create_conflict_check(
                "local_branch",
                branch,
                &destination,
                None,
                format!("Branch {branch:?} already exists locally."),
                vec!["use_existing_branch", "choose_different_name"],
            ))
        }
        BranchState::Remote | BranchState::RemoteUnfetched
            if resolution != Some(WorktreeCreateResolution::LoadFromRemote) =>
        {
            Some(create_conflict_check(
                "remote_branch",
                branch,
                &destination,
                None,
                format!("Branch {branch:?} exists on origin but not locally."),
                vec!["load_from_remote", "choose_different_name"],
            ))
        }
        BranchState::Missing if resolution.is_some() => {
            return Err(WorktreeError::Conflict(format!(
                "the requested {} action is no longer available because branch {branch:?} no longer exists there",
                resolution.expect("checked above").action()
            )));
        }
        _ => None,
    };
    Ok(conflict.unwrap_or(WorktreeCreateCheck {
        status: "ready",
        branch: branch.to_owned(),
        destination: destination_text,
        conflict: None,
    }))
}

fn create_conflict_check(
    kind: &'static str,
    branch: &str,
    path: &Path,
    project_id: Option<ProjectId>,
    message: String,
    actions: Vec<&'static str>,
) -> WorktreeCreateCheck {
    let conflict = WorktreeCreateConflict {
        kind,
        branch: branch.to_owned(),
        path: canonical_display(path),
        project_id,
        message,
        actions,
    };
    WorktreeCreateCheck {
        status: "conflict",
        branch: branch.to_owned(),
        destination: canonical_display(path),
        conflict: Some(conflict),
    }
}

pub(crate) async fn create_with_progress(
    registry: &SharedProcessRegistry,
    request: CreateWorktree,
    progress: Option<&WorktreeOperationReporter>,
) -> WorktreeResult<WorktreeMutation> {
    let command_environment = command_environment(registry).await;
    if let Some(progress) = progress {
        progress.running(
            WorktreeStepId::Branch,
            Some(format!("Checking {}", request.branch)),
        );
    }
    validate_branch(&request.branch, &command_environment).await?;
    for key in request.preferences.keys() {
        validate_preference_key(key)?;
    }
    let source_project = {
        let registry = registry.lock().await;
        registry
            .store()
            .get_project(request.source_project_id)?
            .ok_or_else(|| {
                WorktreeError::InvalidProject(format!(
                    "project {} was not found",
                    request.source_project_id
                ))
            })?
    };
    let snapshot = snapshot_async(Path::new(&source_project.path), &command_environment).await?;

    let mut repository = {
        let registry = registry.lock().await;
        ensure_repository(registry.store(), &snapshot, None)?
    };

    let remembered_preferences = {
        let registry = registry.lock().await;
        registry.store().worktree_preferences(repository.id)?
    };
    let requested_env_policy = request
        .env_policy
        .or(parse_env_preference(request.preferences.get("copy_env"))?);
    let remembered_env_policy = parse_env_preference(remembered_preferences.get("copy_env"))?;
    let env_plan = prepare_environment(
        Path::new(&source_project.path),
        requested_env_policy.or(remembered_env_policy),
        request.remember_env_policy || remembered_env_policy.is_some(),
        &command_environment,
    )
    .await?;

    // Resolve the per-request root only after every `.env` safety check and
    // one-time preference decision, so a rejected port leaves no directory.
    let managed_root = request
        .managed_root
        .unwrap_or_else(|| PathBuf::from(&repository.managed_root));
    let projects = {
        let registry = registry.lock().await;
        registry.store().list_all_projects()?
    };
    let check = inspect_create_snapshot(
        &snapshot,
        &projects,
        &request.branch,
        request.from_ref.as_deref(),
        &managed_root,
        request.resolution,
        &command_environment,
    )
    .await?;
    if let Some(conflict) = check.conflict {
        return Err(WorktreeError::CreateConflict(Box::new(conflict)));
    }
    std::fs::create_dir_all(&managed_root)?;
    let managed_root = workman_core::canonical_path(&managed_root)?;
    repository.managed_root = managed_root.to_string_lossy().into_owned();

    let slug = site_slug(&request.branch);
    let destination = managed_root.join(&slug);

    let branch_state = branch_state(&snapshot, &request.branch, &command_environment).await?;

    if let Some(progress) = progress {
        let detail = match branch_state {
            BranchState::Missing => format!("Creating {}", request.branch),
            BranchState::Local => format!("Using local branch {}", request.branch),
            BranchState::Remote | BranchState::RemoteUnfetched => {
                format!("Tracking origin/{}", request.branch)
            }
        };
        progress.completed(WorktreeStepId::Branch, Some(detail));
    }

    // SWM asks Herd to park the managed parent once, then relies on Herd's
    // wildcard `<folder>.<tld>` routing. It never writes per-site vhosts.
    let herd_enabled = request
        .preferences
        .get("herd_enabled")
        .or_else(|| remembered_preferences.get("herd_enabled"))
        .is_none_or(|value| !matches!(value.as_str(), "no" | "false" | "off"));
    let herd = if herd_enabled {
        match worktree_integrations::herd_for_root(&managed_root, true, &command_environment).await
        {
            Ok(herd) => herd,
            Err(error) => {
                if let Some(progress) = progress {
                    progress.running(WorktreeStepId::Herd, Some(error.message.clone()));
                }
                return Err(WorktreeError::Integration {
                    code: error.code,
                    message: error.message,
                });
            }
        }
    } else {
        HerdView {
            available: false,
            parked: false,
            tld: None,
            error: Some("disabled by the repository herd_enabled preference".into()),
        }
    };
    let site_url = worktree_integrations::site_url(&slug, &herd);

    if let Some(progress) = progress {
        progress.running(
            WorktreeStepId::Worktree,
            Some(destination.to_string_lossy().into_owned()),
        );
    }

    match branch_state {
        BranchState::Local => {
            git_required(
                &snapshot.root_path,
                [
                    "worktree",
                    "add",
                    destination.to_str().unwrap_or_default(),
                    &request.branch,
                ],
                "check out existing local branch",
                &command_environment,
            )
            .await?;
        }
        BranchState::Remote | BranchState::RemoteUnfetched => {
            if branch_state == BranchState::RemoteUnfetched {
                let refspec = format!("refs/heads/{0}:refs/remotes/origin/{0}", request.branch);
                git_required(
                    &snapshot.root_path,
                    ["fetch", "--quiet", "origin", refspec.as_str()],
                    "fetch remote branch",
                    &command_environment,
                )
                .await?;
            } else {
                let refspec = format!("refs/heads/{0}:refs/remotes/origin/{0}", request.branch);
                let _ = git_output(
                    &snapshot.root_path,
                    ["fetch", "--quiet", "origin", refspec.as_str()],
                    GIT_NETWORK_TIMEOUT,
                    &command_environment,
                )
                .await;
            }
            let remote = format!("origin/{}", request.branch);
            git_required(
                &snapshot.root_path,
                [
                    "worktree",
                    "add",
                    "--track",
                    "-b",
                    request.branch.as_str(),
                    destination.to_str().unwrap_or_default(),
                    remote.as_str(),
                ],
                "check out remote branch",
                &command_environment,
            )
            .await?;
        }
        BranchState::Missing => {
            let start = resolve_start_point(
                &snapshot.root_path,
                request.from_ref.as_deref(),
                &command_environment,
            )
            .await?;
            git_required(
                &snapshot.root_path,
                [
                    "worktree",
                    "add",
                    "-b",
                    request.branch.as_str(),
                    destination.to_str().unwrap_or_default(),
                    start.as_str(),
                ],
                "create worktree branch",
                &command_environment,
            )
            .await?;
        }
    }

    if let Some(progress) = progress {
        progress.completed(
            WorktreeStepId::Worktree,
            Some(destination.to_string_lossy().into_owned()),
        );
        progress.running(WorktreeStepId::Environment, None);
    }

    if env_plan.policy == Some(EnvPortPolicy::Copy) && env_plan.source.is_some() {
        match git_success(
            &destination,
            ["check-ignore", "-q", "--", ".env"],
            &command_environment,
        )
        .await
        {
            Ok(true) => {}
            Ok(false) => {
                rollback_created_worktree(
                    &snapshot.root_path,
                    &destination,
                    &request.branch,
                    branch_state != BranchState::Local,
                    &command_environment,
                )
                .await;
                return Err(WorktreeError::UnsafeEnvironment(
                    "refusing to copy .env because the target branch does not ignore it".into(),
                ));
            }
            Err(error) => {
                rollback_created_worktree(
                    &snapshot.root_path,
                    &destination,
                    &request.branch,
                    branch_state != BranchState::Local,
                    &command_environment,
                )
                .await;
                return Err(error);
            }
        }
    }

    let environment = match port_environment(
        &env_plan,
        &destination,
        &slug,
        site_url.as_deref(),
        request.remember_env_policy,
    ) {
        Ok(result) => result,
        Err(error) => {
            rollback_created_worktree(
                &snapshot.root_path,
                &destination,
                &request.branch,
                branch_state != BranchState::Local,
                &command_environment,
            )
            .await;
            return Err(error);
        }
    };

    if let Some(progress) = progress {
        if environment.policy == "copy" {
            progress.completed(
                WorktreeStepId::Environment,
                Some(if environment.copied {
                    ".env copied safely".into()
                } else {
                    "No source .env to copy".into()
                }),
            );
        } else {
            progress.skipped(
                WorktreeStepId::Environment,
                Some("Environment copy skipped".into()),
            );
        }
        if herd.parked {
            progress.completed(
                WorktreeStepId::Herd,
                herd.tld
                    .as_ref()
                    .map(|tld| format!("Wildcard .{tld} routing ready")),
            );
        } else {
            progress.skipped(
                WorktreeStepId::Herd,
                herd.error.clone().or(Some("Herd is unavailable".into())),
            );
        }
        progress.running(WorktreeStepId::Registered, None);
    }

    let registration = {
        let registry = registry.lock().await;
        (|| -> WorktreeResult<Project> {
            registry.store().put_worktree_repository(&repository)?;
            for (key, value) in &request.preferences {
                validate_preference_key(key)?;
                registry
                    .store()
                    .set_worktree_preference(repository.id, key, Some(value))?;
            }
            if request.remember_env_policy
                && let Some(policy) = env_plan.policy
            {
                registry.store().set_worktree_preference(
                    repository.id,
                    "copy_env",
                    Some(policy.preference()),
                )?;
            }
            let project = register_project(
                registry.store(),
                &repository,
                &destination,
                &request.branch,
                request.display_name.as_deref(),
                true,
            )?;
            Ok(project)
        })()
    };
    let project = match registration {
        Ok(registration) => registration,
        Err(error) => {
            rollback_created_worktree(
                &snapshot.root_path,
                &destination,
                &request.branch,
                branch_state != BranchState::Local,
                &command_environment,
            )
            .await;
            return Err(error);
        }
    };
    if let Some(progress) = progress {
        progress.completed(
            WorktreeStepId::Registered,
            Some(format!("Project {} registered", project.id)),
        );
    }
    mutation_for_project(registry, project.id, Some(environment)).await
}

/// SWM's "fork again": create from the selected worktree's exact HEAD, not
/// from its branch name (which may have advanced elsewhere).
pub async fn fork(
    registry: &SharedProcessRegistry,
    request: ForkWorktree,
) -> WorktreeResult<WorktreeMutation> {
    fork_with_progress(registry, request, None).await
}

pub(crate) async fn fork_with_progress(
    registry: &SharedProcessRegistry,
    request: ForkWorktree,
    progress: Option<&WorktreeOperationReporter>,
) -> WorktreeResult<WorktreeMutation> {
    let command_environment = command_environment(registry).await;
    let source = {
        let registry = registry.lock().await;
        registry
            .store()
            .get_project(request.source_project_id)?
            .ok_or_else(|| {
                WorktreeError::InvalidProject(format!(
                    "project {} was not found",
                    request.source_project_id
                ))
            })?
    };
    let snapshot = snapshot_async(Path::new(&source.path), &command_environment).await?;
    let record = matching_record(&snapshot, Path::new(&source.path)).ok_or_else(|| {
        WorktreeError::InvalidPath(format!("{} is not a listed Git worktree", source.path))
    })?;
    create_with_progress(
        registry,
        CreateWorktree {
            source_project_id: request.source_project_id,
            branch: request.branch,
            display_name: request.display_name,
            from_ref: request.resolution.is_none().then(|| record.head.clone()),
            resolution: request.resolution,
            managed_root: request.managed_root,
            preferences: request.preferences,
            env_policy: request.env_policy,
            remember_env_policy: request.remember_env_policy,
        },
        progress,
    )
    .await
}

pub async fn adopt(
    registry: &SharedProcessRegistry,
    request: AdoptWorktree,
) -> WorktreeResult<WorktreeMutation> {
    adopt_with_progress(registry, request, None).await
}

pub(crate) async fn adopt_with_progress(
    registry: &SharedProcessRegistry,
    request: AdoptWorktree,
    progress: Option<&WorktreeOperationReporter>,
) -> WorktreeResult<WorktreeMutation> {
    let command_environment = command_environment(registry).await;
    if let Some(progress) = progress {
        progress.running(
            WorktreeStepId::Branch,
            Some(request.path.to_string_lossy().into_owned()),
        );
    }
    let canonical_input = workman_core::canonical_path(&request.path).map_err(|error| {
        WorktreeError::InvalidPath(format!(
            "could not open {}: {error}",
            request.path.display()
        ))
    })?;
    let top = git_required(
        &canonical_input,
        ["rev-parse", "--show-toplevel"],
        "resolve worktree root",
        &command_environment,
    )
    .await?;
    let top = workman_core::canonical_path(top.trim())?;
    let snapshot = snapshot_async(&top, &command_environment).await?;
    let record = matching_record(&snapshot, &top).ok_or_else(|| {
        WorktreeError::InvalidPath(format!("{} is not a listed Git worktree", top.display()))
    })?;
    let branch = display_branch(record);
    if let Some(progress) = progress {
        progress.completed(
            WorktreeStepId::Branch,
            Some(format!("Verified branch {branch}")),
        );
        progress.running(
            WorktreeStepId::Worktree,
            Some("Linking repository metadata".into()),
        );
    }
    let repository = {
        let registry = registry.lock().await;
        let repository = ensure_repository(registry.store(), &snapshot, None)?;
        for (key, value) in &request.preferences {
            validate_preference_key(key)?;
            registry
                .store()
                .set_worktree_preference(repository.id, key, Some(value))?;
        }
        repository
    };
    let project = {
        let registry = registry.lock().await;
        register_project(
            registry.store(),
            &repository,
            &top,
            &branch,
            request.display_name.as_deref(),
            false,
        )?
    };
    if let Some(progress) = progress {
        progress.completed(
            WorktreeStepId::Worktree,
            Some(top.to_string_lossy().into_owned()),
        );
        progress.skipped(
            WorktreeStepId::Environment,
            Some("Existing environment left unchanged".into()),
        );
        progress.skipped(
            WorktreeStepId::Herd,
            Some("Existing site configuration left unchanged".into()),
        );
        progress.running(WorktreeStepId::Registered, None);
        progress.completed(
            WorktreeStepId::Registered,
            Some(format!("Project {} registered", project.id)),
        );
    }
    mutation_for_project(registry, project.id, None).await
}

pub async fn remove(
    registry: &SharedProcessRegistry,
    request: RemoveWorktree,
) -> WorktreeResult<WorktreeRemoval> {
    let command_environment = command_environment(registry).await;
    if !request.confirm_remove {
        return Err(WorktreeError::Confirmation(
            "set confirm_remove=true to unregister the worktree project; the checkout is kept unless delete_from_disk=true".into(),
        ));
    }
    let (project, processes, all_projects, recovery_hint) = {
        let mut registry = registry.lock().await;
        let project = registry
            .store()
            .get_project(request.project_id)?
            .ok_or_else(|| {
                WorktreeError::InvalidProject(format!(
                    "project {} was not found",
                    request.project_id
                ))
            })?;
        let processes = registry.list(Some(project.id))?;
        let all_projects = registry.store().list_all_projects()?;
        let recovery_hint = match registry.store().get_project_worktree(project.id)? {
            Some(link) => registry
                .store()
                .get_worktree_repository(link.repository_id)?
                .map(|repository| DeleteRecoveryHint {
                    linked_worktree: link.parent_project_id.is_some()
                        || !same_path(Path::new(&project.path), Path::new(&repository.root_path)),
                    repository_root: PathBuf::from(repository.root_path),
                    branch: link.branch,
                }),
            None => None,
        };
        (project, processes, all_projects, recovery_hint)
    };
    let has_running = processes.iter().any(|process| {
        matches!(
            process.status,
            ProcessStatus::Starting | ProcessStatus::Running
        )
    });
    if has_running && !request.confirm_stop_running {
        return Err(WorktreeError::Confirmation(
            "worktree project has running processes; also set confirm_stop_running=true".into(),
        ));
    }

    let path = absolute_path(PathBuf::from(&project.path));
    let registration_issue = broken_registration_issue(
        &project,
        &all_projects,
        recovery_hint.as_ref(),
        &command_environment,
    )
    .await;
    let mut branch = project
        .display_name
        .clone()
        .unwrap_or_else(|| project.name.clone());
    let mut deleted_from_disk = false;
    let mut metadata_pruned = false;
    let mut branch_kept = true;
    let mut post_delete_issue = None;
    if request.delete_from_disk && registration_issue.is_none() {
        let verified = verify_project_delete_target(
            &project,
            &all_projects,
            recovery_hint.as_ref(),
            &command_environment,
        )
        .await?;
        branch.clone_from(&verified.branch);
        branch_kept = verified.kind == DeleteTargetKind::LinkedWorktree;
        let mut safety = delete_target_safety(&verified, &command_environment).await?;
        require_delete_confirmation(&verified, safety.as_ref(), &request)?;

        // Quiesce Workman-owned processes before removing their working directory.
        {
            let mut registry = registry.lock().await;
            for process in &processes {
                if registry.store().get_process(process.id)?.is_some() {
                    registry.close(process.id)?;
                }
            }
        }
        // A stopped process can flush or create files. Recompute immediately
        // before deletion so the final force decision uses the quiesced path.
        safety = delete_target_safety(&verified, &command_environment).await?;
        require_delete_confirmation(&verified, safety.as_ref(), &request)?;

        match verified.kind {
            DeleteTargetKind::LinkedWorktree => {
                let repository_root = verified
                    .repository_root
                    .as_ref()
                    .expect("linked worktree has repository root");
                let mut args = vec![OsString::from("worktree"), OsString::from("remove")];
                if request.force_dirty
                    || safety.as_ref().is_some_and(|safety| safety.requires_force)
                {
                    args.push(OsString::from("--force"));
                }
                args.push(OsString::from("--"));
                args.push(verified.path.as_os_str().to_owned());
                let git_error = git_required(
                    repository_root,
                    args,
                    "remove local Git worktree",
                    &command_environment,
                )
                .await
                .err();
                ensure_verified_directory_removed(&verified.path, git_error.as_ref())?;
            }
            DeleteTargetKind::PrimaryCheckout | DeleteTargetKind::Folder => {
                ensure_verified_directory_removed(&verified.path, None)?;
            }
        }
        if verified.kind == DeleteTargetKind::LinkedWorktree {
            let repository_root = verified
                .repository_root
                .as_ref()
                .expect("linked worktree has repository root");
            match git_required(
                repository_root,
                ["worktree", "prune"],
                "prune local worktree metadata",
                &command_environment,
            )
            .await
            {
                Ok(_) => metadata_pruned = true,
                Err(error) => post_delete_issue = Some(error),
            }
        }
        if verified.kind == DeleteTargetKind::LinkedWorktree {
            let repository_root = verified
                .repository_root
                .as_ref()
                .expect("linked worktree has repository root");
            match git_success(
                repository_root,
                [
                    "show-ref",
                    "--verify",
                    "--quiet",
                    format!("refs/heads/{}", verified.branch).as_str(),
                ],
                &command_environment,
            )
            .await
            {
                Ok(kept) => {
                    branch_kept = kept;
                    if !kept {
                        post_delete_issue.get_or_insert_with(|| WorktreeError::Git {
                            operation: "verify preserved branch".into(),
                            message: format!(
                                "branch {:?} disappeared during local worktree removal",
                                verified.branch
                            ),
                        });
                    }
                }
                Err(error) => {
                    branch_kept = false;
                    post_delete_issue.get_or_insert(error);
                }
            }
        }
        // Git cleanup and other local actors can recreate the checkout after
        // the first deletion check. Verify absence again at the registration
        // boundary; never unregister a project whose path has reappeared.
        ensure_verified_directory_still_absent(&verified.path)?;
        // From this point onward the checkout is verified gone. Any
        // finalization error is reported only after the canonical Workman
        // project is removed, so a successful disk mutation cannot leave a
        // stale registration.
        deleted_from_disk = true;
    } else {
        // Registration-only removal still stops owned processes before the
        // active profile loses the project.
        let mut registry = registry.lock().await;
        for process in &processes {
            if registry.store().get_process(process.id)?.is_some() {
                registry.close(process.id)?;
            }
        }
    }

    // Project deletion is intentionally last: even a self-targeting agent has
    // already finished the Git operation before its process can disappear.
    let (project_unregistered, selected_project_id) = {
        let registry = registry.lock().await;
        let project_unregistered = if deleted_from_disk
            || registration_issue
                .as_ref()
                .is_some_and(RegistrationIssue::remove_everywhere)
        {
            registry.store().delete_project_everywhere(project.id)?
        } else {
            registry.store().delete_project(project.id)?
        };
        let mut selected_project_id = None;
        if project.selected
            && let Some(mut next) = registry.store().list_projects()?.into_iter().next()
        {
            next.selected = true;
            selected_project_id = Some(next.id);
            registry.store().put_project(&next)?;
        }
        (project_unregistered, selected_project_id)
    };
    if let Some(error) = post_delete_issue {
        return Err(WorktreeError::Git {
            operation: "finalize worktree deletion".into(),
            message: format!(
                "the checkout at {} and its Workman registration were removed, but final verification failed: {error}",
                path.display()
            ),
        });
    }
    Ok(WorktreeRemoval {
        project_id: project.id,
        path: path.to_string_lossy().into_owned(),
        branch,
        removed: true,
        project_unregistered,
        deleted_from_disk,
        metadata_pruned,
        branch_kept,
        files_removed: deleted_from_disk,
        files_untouched: !deleted_from_disk,
        registration_issue: registration_issue.map(|issue| issue.message),
        selected_project_id,
    })
}

async fn broken_registration_issue(
    project: &Project,
    all_projects: &[Project],
    recovery_hint: Option<&DeleteRecoveryHint>,
    command_environment: &BTreeMap<OsString, OsString>,
) -> Option<RegistrationIssue> {
    let path = Path::new(&project.path);
    let canonical = canonical_display(path);
    let duplicate_owner = all_projects
        .iter()
        .filter(|candidate| {
            candidate.id != project.id && same_path(Path::new(&candidate.path), path)
        })
        .min_by_key(|candidate| (candidate.path != canonical, candidate.id));
    if project.path != canonical
        && let Some(owner) = duplicate_owner
    {
        return Some(RegistrationIssue {
            kind: RegistrationIssueKind::Duplicate,
            message: format!(
                "duplicate registration: project {} owns the same canonical path; files were left untouched",
                owner.id
            ),
        });
    }
    if !path.exists() {
        return Some(RegistrationIssue {
            kind: RegistrationIssueKind::MissingPath,
            message: "broken registration: the project path is missing; files were left untouched"
                .into(),
        });
    }
    let recovery_hint = recovery_hint?;
    let snapshot = match snapshot_async(&recovery_hint.repository_root, command_environment).await {
        Ok(snapshot) => snapshot,
        Err(_) => {
            return Some(RegistrationIssue {
                kind: RegistrationIssueKind::ParentUnavailable,
                message:
                    "broken registration: the parent Git repository is unavailable; files were left untouched"
                        .into(),
            });
        }
    };
    if matching_record(&snapshot, path).is_none() {
        return Some(RegistrationIssue {
            kind: RegistrationIssueKind::NotRecordedByParent,
            message:
                "broken registration: the path is not a worktree of its recorded parent; files were left untouched"
                    .into(),
        });
    }
    None
}

pub async fn forget_env_preference(
    registry: &SharedProcessRegistry,
    project_id: ProjectId,
) -> WorktreeResult<WorktreePreferenceMutation> {
    let command_environment = command_environment(registry).await;
    let repository_id = {
        let registry = registry.lock().await;
        if registry.store().get_project_worktree(project_id)?.is_none() {
            reconcile_existing_projects_with_environment(registry.store(), &command_environment)?;
        }
        registry
            .store()
            .get_project_worktree(project_id)?
            .map(|link| link.repository_id)
            .ok_or_else(|| {
                WorktreeError::InvalidPath(format!("project {project_id} is not a Git worktree"))
            })?
    };
    {
        let registry = registry.lock().await;
        set_preference(registry.store(), repository_id, "copy_env", None)?;
    }
    Ok(WorktreePreferenceMutation {
        repository_id,
        key: "copy_env",
        cleared: true,
    })
}

pub async fn health(registry: &SharedProcessRegistry) -> WorktreeHealth {
    let (roots, command_environment) = {
        let registry = registry.lock().await;
        let roots = registry
            .store()
            .list_worktree_repositories()
            .unwrap_or_default()
            .into_iter()
            .map(|repository| PathBuf::from(repository.managed_root))
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        (roots, registry.resolved_user_environment())
    };
    let command_environment = command_environment.command_environment();
    worktree_integrations::health(roots, &command_environment).await
}

async fn prepare_environment(
    source_worktree: &Path,
    policy: Option<EnvPortPolicy>,
    remembered: bool,
    command_environment: &BTreeMap<OsString, OsString>,
) -> WorktreeResult<EnvironmentPlan> {
    let source = source_worktree.join(".env");
    let metadata = match fs::symlink_metadata(&source) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(EnvironmentPlan {
                source: None,
                contents: None,
                permissions: None,
                policy,
                remembered,
            });
        }
        Err(error) => return Err(error.into()),
    };
    let Some(policy) = policy else {
        return Err(WorktreeError::EnvPreference(format!(
            "{} has an ignored .env; choose env_policy=copy or env_policy=skip and set remember_env_policy=true to ask only once for this repository",
            source_worktree.display()
        )));
    };
    if policy == EnvPortPolicy::Skip {
        return Ok(EnvironmentPlan {
            source: Some(source),
            contents: None,
            permissions: None,
            policy: Some(policy),
            remembered,
        });
    }
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(WorktreeError::UnsafeEnvironment(format!(
            "refusing to copy {} because it is not a regular file",
            source.display()
        )));
    }
    if git_success(
        source_worktree,
        ["ls-files", "--error-unmatch", "--", ".env"],
        command_environment,
    )
    .await?
    {
        return Err(WorktreeError::UnsafeEnvironment(
            "refusing to copy .env because Git tracks it".into(),
        ));
    }
    if !git_success(
        source_worktree,
        ["check-ignore", "-q", "--", ".env"],
        command_environment,
    )
    .await?
    {
        return Err(WorktreeError::UnsafeEnvironment(
            "refusing to copy .env because Git does not ignore it".into(),
        ));
    }
    let contents = fs::read(&source)?;
    if std::str::from_utf8(&contents).is_err() {
        return Err(WorktreeError::UnsafeEnvironment(
            "refusing to rewrite .env because it is not valid UTF-8".into(),
        ));
    }
    Ok(EnvironmentPlan {
        source: Some(source),
        contents: Some(contents),
        permissions: Some(metadata.permissions()),
        policy: Some(policy),
        remembered,
    })
}

fn port_environment(
    plan: &EnvironmentPlan,
    destination: &Path,
    app_name: &str,
    app_url: Option<&str>,
    remember_requested: bool,
) -> WorktreeResult<EnvironmentPortResult> {
    let source_present = plan.source.is_some();
    if plan.policy != Some(EnvPortPolicy::Copy) || !source_present {
        return Ok(EnvironmentPortResult {
            source_present,
            policy: plan
                .policy
                .map(EnvPortPolicy::label)
                .unwrap_or("not_present"),
            copied: false,
            remembered: plan.remembered || remember_requested,
            app_name_rewritten: false,
            app_url_rewritten: false,
        });
    }
    let contents = plan.contents.as_deref().ok_or_else(|| {
        WorktreeError::UnsafeEnvironment("the approved .env contents disappeared".into())
    })?;
    let source = std::str::from_utf8(contents).map_err(|_| {
        WorktreeError::UnsafeEnvironment("the approved .env is not valid UTF-8".into())
    })?;
    let (rewritten, app_name_rewritten, app_url_rewritten) =
        rewrite_environment(source, app_name, app_url);
    let target = destination.join(".env");
    if target.exists() {
        return Err(WorktreeError::UnsafeEnvironment(format!(
            "refusing to overwrite existing {}",
            target.display()
        )));
    }
    let temporary = destination.join(format!(".env.workman-{}.tmp", std::process::id()));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    let mut file = options.open(&temporary)?;
    file.write_all(rewritten.as_bytes())?;
    file.sync_all()?;
    if let Some(permissions) = &plan.permissions {
        fs::set_permissions(&temporary, permissions.clone())?;
    }
    fs::rename(&temporary, &target)?;
    Ok(EnvironmentPortResult {
        source_present: true,
        policy: "copy",
        copied: true,
        remembered: plan.remembered || remember_requested,
        app_name_rewritten,
        app_url_rewritten,
    })
}

fn rewrite_environment(
    source: &str,
    app_name: &str,
    app_url: Option<&str>,
) -> (String, bool, bool) {
    let trailing_newline = source.ends_with('\n');
    let mut lines = source.lines().map(str::to_owned).collect::<Vec<_>>();
    set_environment_value(&mut lines, "APP_NAME", &format!("\"{app_name}\""));
    let app_name_rewritten = true;
    let app_url_rewritten = if let Some(app_url) = app_url {
        set_environment_value(&mut lines, "APP_URL", app_url);
        true
    } else {
        false
    };
    let mut lines = lines.join("\n");
    if trailing_newline {
        lines.push('\n');
    }
    (lines, app_name_rewritten, app_url_rewritten)
}

fn set_environment_value(lines: &mut Vec<String>, key: &str, value: &str) {
    let prefix = format!("{key}=");
    let mut found = false;
    lines.retain_mut(|line| {
        if !line.starts_with(&prefix) {
            return true;
        }
        if found {
            return false;
        }
        *line = format!("{prefix}{value}");
        found = true;
        true
    });
    if !found {
        lines.push(format!("{prefix}{value}"));
    }
}

fn parse_env_preference(value: Option<&String>) -> WorktreeResult<Option<EnvPortPolicy>> {
    match value.map(|value| value.trim().to_ascii_lowercase()) {
        None => Ok(None),
        Some(value) if matches!(value.as_str(), "yes" | "true" | "copy") => {
            Ok(Some(EnvPortPolicy::Copy))
        }
        Some(value) if matches!(value.as_str(), "no" | "false" | "skip") => {
            Ok(Some(EnvPortPolicy::Skip))
        }
        Some(value) => Err(WorktreeError::InvalidPath(format!(
            "copy_env preference must be yes/copy or no/skip, got {value:?}"
        ))),
    }
}

async fn rollback_created_worktree(
    repository: &Path,
    destination: &Path,
    branch: &str,
    branch_was_created: bool,
    environment: &BTreeMap<OsString, OsString>,
) {
    let destination = destination.to_string_lossy().into_owned();
    let _ = git_output(
        repository,
        ["worktree", "remove", "--force", destination.as_str()],
        GIT_NETWORK_TIMEOUT,
        environment,
    )
    .await;
    if branch_was_created {
        let _ = git_output(
            repository,
            ["branch", "-D", branch],
            GIT_NETWORK_TIMEOUT,
            environment,
        )
        .await;
    }
}

pub fn set_preference(
    store: &Store,
    repository_id: i64,
    key: &str,
    value: Option<&str>,
) -> WorktreeResult<()> {
    validate_preference_key(key)?;
    if store.get_worktree_repository(repository_id)?.is_none() {
        return Err(WorktreeError::InvalidPath(format!(
            "worktree repository {repository_id} was not found"
        )));
    }
    store.set_worktree_preference(repository_id, key, value)?;
    Ok(())
}

async fn mutation_for_project(
    registry: &SharedProcessRegistry,
    project_id: ProjectId,
    environment: Option<EnvironmentPortResult>,
) -> WorktreeResult<WorktreeMutation> {
    let list = list_for_project(registry, project_id).await?;
    let worktree = list
        .worktrees
        .iter()
        .find(|worktree| worktree.project_id == Some(project_id))
        .cloned()
        .ok_or_else(|| WorktreeError::InvalidPath("registered worktree disappeared".into()))?;
    let project = {
        let registry = registry.lock().await;
        let project = registry.store().get_project(project_id)?.ok_or_else(|| {
            WorktreeError::InvalidProject(format!("project {project_id} was not found"))
        })?;
        project_envelope(registry.store(), project)?
    };
    Ok(WorktreeMutation {
        repository: list.repository,
        project,
        worktree,
        environment,
    })
}

async fn list_from_snapshot(
    registry: &SharedProcessRegistry,
    repository: WorktreeRepository,
    snapshot: RepositorySnapshot,
    projects: Vec<Project>,
    links: Vec<ProjectWorktree>,
    refresh_pull_requests: bool,
    command_environment: &BTreeMap<OsString, OsString>,
) -> WorktreeResult<WorktreeList> {
    let herd = worktree_integrations::herd_for_root(
        Path::new(&repository.managed_root),
        false,
        command_environment,
    )
    .await
    .unwrap_or_else(|error| HerdView {
        available: true,
        parked: false,
        tld: None,
        error: Some(error.message),
    });
    let (pull_requests, pull_request_cache) = worktree_integrations::pull_requests(
        &snapshot.root_path,
        refresh_pull_requests,
        command_environment,
    )
    .await;
    let project_by_path = projects
        .iter()
        .map(|project| (canonical_display(Path::new(&project.path)), project))
        .collect::<HashMap<_, _>>();
    let link_by_project = links
        .iter()
        .map(|link| (link.project_id, link))
        .collect::<HashMap<_, _>>();
    let root_project_id = project_by_path
        .get(&canonical_display(&snapshot.root_path))
        .map(|project| project.id);
    let dependent_worktrees = snapshot
        .worktrees
        .iter()
        .filter(|record| {
            !record.bare
                && !record.prunable
                && record.path.exists()
                && !same_path(&record.path, &snapshot.root_path)
        })
        .map(|record| record.path.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let mut entries = Vec::with_capacity(snapshot.worktrees.len());
    for record in snapshot.worktrees {
        let path_key = canonical_display(&record.path);
        let project = project_by_path.get(&path_key).copied();
        let link = project.and_then(|project| link_by_project.get(&project.id).copied());
        let branch = display_branch(&record);
        let site_name = record
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .map(site_slug)
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| site_slug(&branch));
        let is_herd_site = record
            .path
            .parent()
            .is_some_and(|parent| same_path(parent, Path::new(&repository.managed_root)));
        let branch_pull_requests = pull_requests.get(&branch).cloned().unwrap_or_default();
        let pull_request = branch_pull_requests.first().cloned();
        let is_main = same_path(&record.path, &snapshot.root_path);
        let registered = project.is_some();
        let managed = link.is_some_and(|link| link.managed);
        let kind = if is_main {
            "main"
        } else if managed {
            "managed"
        } else if registered {
            "adopted"
        } else {
            "external"
        };
        let delete_safety =
            if registered && !record.bare && !record.prunable && record.path.exists() {
                let mut safety = worktree_delete_safety(
                    &record.path,
                    &snapshot.root_path,
                    &branch,
                    command_environment,
                )
                .await?;
                if is_main && !dependent_worktrees.is_empty() {
                    safety.dependent_worktrees = dependent_worktrees.clone();
                    safety.requires_force = true;
                }
                Some(safety)
            } else {
                None
            };
        let status = if let Some(safety) = &delete_safety {
            if safety.dirty_files == 0 {
                "clean"
            } else {
                "dirty"
            }
        } else {
            worktree_status(&record, command_environment).await?
        };
        entries.push(WorktreeEntry {
            project_id: project.map(|project| project.id),
            parent_project_id: link
                .and_then(|link| link.parent_project_id)
                .or_else(|| (!is_main).then_some(root_project_id).flatten()),
            path: record.path.to_string_lossy().into_owned(),
            branch,
            head: record.head,
            kind,
            status,
            managed,
            registered,
            can_adopt: !registered && !record.bare,
            can_remove: registered,
            delete_safety,
            locked: record.locked,
            prunable: record.prunable,
            site_url: is_herd_site
                .then(|| worktree_integrations::site_url(&site_name, &herd))
                .flatten(),
            pull_request,
            pull_requests: branch_pull_requests,
        });
    }
    let preferences = {
        let registry = registry.lock().await;
        registry.store().worktree_preferences(repository.id)?
    };
    Ok(WorktreeList {
        repository: RepositoryView {
            id: repository.id,
            name: repository.name,
            root_path: repository.root_path,
            managed_root: repository.managed_root,
            preferences,
            herd,
        },
        worktrees: entries,
        pull_requests: pull_request_cache,
    })
}

fn register_project(
    store: &Store,
    repository: &WorktreeRepository,
    path: &Path,
    branch: &str,
    display_name: Option<&str>,
    managed: bool,
) -> WorktreeResult<Project> {
    let canonical = workman_core::canonical_path(path)?;
    let canonical_string = canonical.to_string_lossy().into_owned();
    let existing = store.get_project_by_path_any(&canonical_string)?;
    let display_name = normalized_optional_project_title(display_name);
    let project = if let Some(project) = existing {
        project
    } else {
        Project {
            id: store.next_project_id()?,
            path: canonical_string,
            name: format!("{}: {branch}", repository.name),
            display_name,
            icon: None,
            selected: false,
            sort_order: store.next_project_sort_order()?,
        }
    };
    let root_project_id = store
        .get_project_by_path_any(&repository.root_path)?
        .map(|candidate| candidate.id);
    let existing_link = store.get_project_worktree(project.id)?;
    let link = ProjectWorktree {
        project_id: project.id,
        repository_id: repository.id,
        parent_project_id: (!same_path(&canonical, Path::new(&repository.root_path)))
            .then_some(root_project_id)
            .flatten(),
        branch: branch.to_owned(),
        managed: existing_link.map(|link| link.managed).unwrap_or(managed),
    };
    store.put_project_with_worktree(&project, &link)?;
    Ok(project)
}

fn ensure_repository(
    store: &Store,
    snapshot: &RepositorySnapshot,
    managed_root_override: Option<&Path>,
) -> WorktreeResult<WorktreeRepository> {
    let root_path = canonical_display(&snapshot.root_path);
    let managed_root = managed_root_override
        .map(|path| absolute_path(path.to_path_buf()))
        .unwrap_or_else(default_worktree_root);
    if let Some(mut repository) = store.get_worktree_repository_by_root(&root_path)? {
        repository.name = snapshot.name.clone();
        if managed_root_override.is_some() {
            repository.managed_root = managed_root.to_string_lossy().into_owned();
        }
        store.put_worktree_repository(&repository)?;
        return Ok(repository);
    }
    let repository = WorktreeRepository {
        id: store.next_worktree_repository_id()?,
        root_path,
        name: snapshot.name.clone(),
        managed_root: managed_root.to_string_lossy().into_owned(),
    };
    store.put_worktree_repository(&repository)?;
    Ok(repository)
}

fn validate_preference_key(key: &str) -> WorktreeResult<()> {
    if key.is_empty()
        || key.len() > 64
        || !key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        return Err(WorktreeError::InvalidPath(format!(
            "invalid worktree preference key {key:?}"
        )));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BranchState {
    Missing,
    Local,
    Remote,
    RemoteUnfetched,
}

async fn branch_state(
    snapshot: &RepositorySnapshot,
    branch: &str,
    environment: &BTreeMap<OsString, OsString>,
) -> WorktreeResult<BranchState> {
    if git_success(
        &snapshot.root_path,
        [
            "show-ref",
            "--verify",
            "--quiet",
            format!("refs/heads/{branch}").as_str(),
        ],
        environment,
    )
    .await?
    {
        return Ok(BranchState::Local);
    }
    if git_success(
        &snapshot.root_path,
        [
            "show-ref",
            "--verify",
            "--quiet",
            format!("refs/remotes/origin/{branch}").as_str(),
        ],
        environment,
    )
    .await?
    {
        return Ok(BranchState::Remote);
    }
    if !git_success(
        &snapshot.root_path,
        ["remote", "get-url", "origin"],
        environment,
    )
    .await?
    {
        return Ok(BranchState::Missing);
    }
    let output = git_output(
        &snapshot.root_path,
        [
            "ls-remote",
            "--exit-code",
            "--heads",
            "origin",
            format!("refs/heads/{branch}").as_str(),
        ],
        GIT_NETWORK_TIMEOUT,
        environment,
    )
    .await?;
    Ok(if output.status.success() {
        BranchState::RemoteUnfetched
    } else {
        BranchState::Missing
    })
}

async fn resolve_start_point(
    repository: &Path,
    requested: Option<&str>,
    environment: &BTreeMap<OsString, OsString>,
) -> WorktreeResult<String> {
    let from_ref = if let Some(requested) = requested.filter(|value| !value.trim().is_empty()) {
        requested.trim().to_owned()
    } else {
        let remote_head = git_output(
            repository,
            [
                "symbolic-ref",
                "--quiet",
                "--short",
                "refs/remotes/origin/HEAD",
            ],
            Duration::from_secs(5),
            environment,
        )
        .await?;
        if remote_head.status.success() {
            String::from_utf8_lossy(&remote_head.stdout)
                .trim()
                .strip_prefix("origin/")
                .unwrap_or("HEAD")
                .to_owned()
        } else {
            "HEAD".to_owned()
        }
    };

    let looks_like_commit = (7..=64).contains(&from_ref.len())
        && from_ref
            .chars()
            .all(|character| character.is_ascii_hexdigit());
    let simple_branch = !from_ref.contains('/') && !looks_like_commit;
    if simple_branch
        && git_success(repository, ["remote", "get-url", "origin"], environment).await?
    {
        let _ = git_output(
            repository,
            ["fetch", "--quiet", "origin", from_ref.as_str()],
            GIT_NETWORK_TIMEOUT,
            environment,
        )
        .await;
    }
    if let Some(branch) = from_ref.strip_prefix("origin/")
        && !branch.is_empty()
        && git_success(repository, ["remote", "get-url", "origin"], environment).await?
    {
        let refspec = format!("refs/heads/{branch}:refs/remotes/origin/{branch}");
        git_required(
            repository,
            ["fetch", "--quiet", "origin", refspec.as_str()],
            "fetch remote starting ref",
            environment,
        )
        .await?;
    }
    let remote = format!("origin/{from_ref}");
    if !from_ref.contains('/')
        && git_success(
            repository,
            [
                "show-ref",
                "--verify",
                "--quiet",
                format!("refs/remotes/{remote}").as_str(),
            ],
            environment,
        )
        .await?
    {
        return Ok(remote);
    }
    if git_success(
        repository,
        [
            "rev-parse",
            "--verify",
            "--quiet",
            format!("{from_ref}^{{commit}}").as_str(),
        ],
        environment,
    )
    .await?
    {
        return Ok(from_ref);
    }
    if git_success(
        repository,
        [
            "rev-parse",
            "--verify",
            "--quiet",
            format!("{remote}^{{commit}}").as_str(),
        ],
        environment,
    )
    .await?
    {
        return Ok(remote);
    }
    Err(WorktreeError::InvalidBranch(format!(
        "cannot find branch-from ref {from_ref:?}"
    )))
}

async fn detect_origin_default_ref(
    repository: &Path,
    environment: &BTreeMap<OsString, OsString>,
) -> WorktreeResult<Option<String>> {
    if let Some(reference) = git_optional(
        repository,
        [
            "symbolic-ref",
            "--quiet",
            "--short",
            "refs/remotes/origin/HEAD",
        ],
        environment,
    )
    .await?
    .filter(|reference| {
        reference.starts_with("origin/") && !reference.contains(char::is_whitespace)
    }) {
        return Ok(Some(reference));
    }
    if !git_success(repository, ["remote", "get-url", "origin"], environment).await? {
        return Ok(None);
    }
    let output = git_output(
        repository,
        ["ls-remote", "--symref", "origin", "HEAD"],
        GIT_NETWORK_TIMEOUT,
        environment,
    )
    .await?;
    if !output.status.success() {
        return Ok(None);
    }
    Ok(parse_origin_default_ref(&String::from_utf8_lossy(
        &output.stdout,
    )))
}

async fn validate_branch(
    branch: &str,
    environment: &BTreeMap<OsString, OsString>,
) -> WorktreeResult<()> {
    if branch.trim() != branch || branch.is_empty() {
        return Err(WorktreeError::InvalidBranch(
            "worktree branch must not be empty or padded with whitespace".into(),
        ));
    }
    let output = command_output(
        Command::new(git_executable(environment)?).args(["check-ref-format", "--branch", branch]),
        Duration::from_secs(5),
        environment,
    )
    .await?;
    if output.status.success() {
        Ok(())
    } else {
        Err(WorktreeError::InvalidBranch(format!(
            "invalid Git branch name {branch:?}"
        )))
    }
}

async fn snapshot_async(
    path: &Path,
    environment: &BTreeMap<OsString, OsString>,
) -> WorktreeResult<RepositorySnapshot> {
    let top = git_required(
        path,
        ["rev-parse", "--show-toplevel"],
        "resolve repository",
        environment,
    )
    .await?;
    let top = workman_core::canonical_path(top.trim())?;
    let porcelain = git_required_bytes(
        &top,
        ["worktree", "list", "--porcelain", "-z"],
        "list worktrees",
        environment,
    )
    .await?;
    snapshot_from_porcelain(top, &porcelain)
}

fn snapshot_sync(
    path: &Path,
    environment: &BTreeMap<OsString, OsString>,
) -> WorktreeResult<RepositorySnapshot> {
    let top = std_git_required(
        path,
        ["rev-parse", "--show-toplevel"],
        "resolve repository",
        environment,
    )?;
    let top = workman_core::canonical_path(top.trim())?;
    let porcelain = std_git_output(&top, ["worktree", "list", "--porcelain", "-z"], environment)?;
    if !porcelain.status.success() {
        return Err(git_failure("list worktrees", &porcelain));
    }
    snapshot_from_porcelain(top, &porcelain.stdout)
}

fn snapshot_from_porcelain(root_hint: PathBuf, bytes: &[u8]) -> WorktreeResult<RepositorySnapshot> {
    let worktrees = parse_porcelain(bytes)?;
    let root_path = worktrees
        .first()
        .map(|record| record.path.clone())
        .unwrap_or(root_hint);
    let root_path = workman_core::canonical_path(&root_path).unwrap_or(root_path);
    let name = root_path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("repository")
        .to_owned();
    Ok(RepositorySnapshot {
        root_path,
        name,
        worktrees,
    })
}

fn parse_porcelain(bytes: &[u8]) -> WorktreeResult<Vec<GitWorktree>> {
    let mut result = Vec::new();
    let mut current: Option<GitWorktree> = None;
    for token in bytes.split(|byte| *byte == 0) {
        if token.is_empty() {
            if let Some(record) = current.take() {
                result.push(record);
            }
            continue;
        }
        let line = String::from_utf8_lossy(token);
        if let Some(path) = line.strip_prefix("worktree ") {
            if let Some(record) = current.take() {
                result.push(record);
            }
            current = Some(GitWorktree {
                path: PathBuf::from(path),
                head: String::new(),
                branch: None,
                bare: false,
                locked: false,
                prunable: false,
            });
        } else if let Some(record) = current.as_mut() {
            if let Some(head) = line.strip_prefix("HEAD ") {
                record.head = head.to_owned();
            } else if let Some(branch) = line.strip_prefix("branch refs/heads/") {
                record.branch = Some(branch.to_owned());
            } else if line == "bare" {
                record.bare = true;
            } else if line == "locked" || line.starts_with("locked ") {
                record.locked = true;
            } else if line == "prunable" || line.starts_with("prunable ") {
                record.prunable = true;
            }
        }
    }
    if let Some(record) = current {
        result.push(record);
    }
    if result.is_empty() {
        return Err(WorktreeError::Git {
            operation: "parse worktree list".into(),
            message: "Git returned no worktrees".into(),
        });
    }
    Ok(result)
}

async fn verify_project_delete_target(
    project: &Project,
    all_projects: &[Project],
    recovery_hint: Option<&DeleteRecoveryHint>,
    environment: &BTreeMap<OsString, OsString>,
) -> WorktreeResult<VerifiedDeleteTarget> {
    // A trailing separator makes symlink_metadata follow a final symlink on
    // Unix. Strip redundant lexical components before the lstat-style check
    // so legacy/imported paths such as `/path/to/link/` cannot bypass it.
    let registered_path = normalized_registered_path(Path::new(&project.path));
    let metadata = fs::symlink_metadata(&registered_path).map_err(|error| {
        WorktreeError::InvalidPath(format!("could not open {}: {error}", project.path))
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(WorktreeError::InvalidPath(format!(
            "refusing to delete {}; the registered path is not a real directory",
            project.path
        )));
    }
    let path = workman_core::canonical_path(&registered_path)?;
    let home = dirs::home_dir().map(|home| workman_core::canonical_path(&home).unwrap_or(home));
    if path.parent().is_none() || home.as_ref().is_some_and(|home| path == *home) {
        return Err(WorktreeError::InvalidPath(format!(
            "refusing to delete suspicious project path {}",
            path.display()
        )));
    }
    for other in all_projects
        .iter()
        .filter(|candidate| candidate.id != project.id)
    {
        let Ok(other_path) = workman_core::canonical_path(&other.path) else {
            continue;
        };
        if other_path.starts_with(&path) {
            return Err(WorktreeError::InvalidPath(format!(
                "refusing to delete {}; it contains registered project {} at {}",
                path.display(),
                other.id,
                other_path.display()
            )));
        }
    }

    let project_label = project
        .display_name
        .clone()
        .unwrap_or_else(|| project.name.clone());
    let Some(top) = git_optional(&path, ["rev-parse", "--show-toplevel"], environment).await?
    else {
        if let Some(hint) = recovery_hint.filter(|hint| hint.linked_worktree) {
            let repository_root = workman_core::canonical_path(&hint.repository_root).map_err(|error| {
                WorktreeError::InvalidPath(format!(
                    "could not verify the repository for partially removed worktree {}: {error}",
                    path.display()
                ))
            })?;
            if repository_root != path {
                return Ok(VerifiedDeleteTarget {
                    path,
                    repository_root: Some(repository_root),
                    branch: hint.branch.clone(),
                    kind: DeleteTargetKind::LinkedWorktree,
                    dependent_worktrees: Vec::new(),
                    recovering_partial_removal: true,
                });
            }
        }
        if path.join(".git").exists() {
            return Err(WorktreeError::InvalidPath(format!(
                "refusing to delete {}; it looks like a Git checkout but Git could not verify it",
                path.display()
            )));
        }
        return Ok(VerifiedDeleteTarget {
            path,
            repository_root: None,
            branch: project_label.clone(),
            kind: DeleteTargetKind::Folder,
            dependent_worktrees: Vec::new(),
            recovering_partial_removal: false,
        });
    };
    let top = workman_core::canonical_path(top.trim())?;
    if top != path {
        // A project may intentionally point at a subdirectory inside a larger
        // checkout. Deleting that exact folder must not be broadened to the
        // surrounding repository.
        return Ok(VerifiedDeleteTarget {
            path,
            repository_root: None,
            branch: project_label.clone(),
            kind: DeleteTargetKind::Folder,
            dependent_worktrees: Vec::new(),
            recovering_partial_removal: false,
        });
    }

    let snapshot = snapshot_async(&path, environment).await?;
    let record = matching_record(&snapshot, &path).ok_or_else(|| {
        WorktreeError::InvalidPath(format!(
            "refusing to delete {}; Git does not list the exact canonical checkout",
            path.display()
        ))
    })?;
    if record.bare {
        return Err(WorktreeError::InvalidPath(format!(
            "refusing to delete bare Git repository {}",
            path.display()
        )));
    }
    let branch = display_branch(record);
    let primary = same_path(&record.path, &snapshot.root_path);
    let dependent_worktrees = if primary {
        snapshot
            .worktrees
            .iter()
            .filter(|candidate| {
                !candidate.bare
                    && !candidate.prunable
                    && candidate.path.exists()
                    && !same_path(&candidate.path, &snapshot.root_path)
            })
            .map(|candidate| candidate.path.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    Ok(VerifiedDeleteTarget {
        path,
        repository_root: Some(snapshot.root_path),
        branch,
        kind: if primary {
            DeleteTargetKind::PrimaryCheckout
        } else {
            DeleteTargetKind::LinkedWorktree
        },
        dependent_worktrees,
        recovering_partial_removal: false,
    })
}

fn ensure_verified_directory_removed(
    path: &Path,
    git_error: Option<&WorktreeError>,
) -> WorktreeResult<()> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(WorktreeError::InvalidPath(format!(
                "could not inspect verified deletion target {}: {error}",
                path.display()
            )));
        }
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(WorktreeError::InvalidPath(format!(
            "refusing filesystem deletion fallback for {}; the verified path is no longer a real directory",
            path.display()
        )));
    }
    let current = workman_core::canonical_path(path).map_err(|error| {
        WorktreeError::InvalidPath(format!(
            "could not re-verify deletion target {}: {error}",
            path.display()
        ))
    })?;
    if current != path {
        return Err(WorktreeError::InvalidPath(format!(
            "refusing filesystem deletion fallback because {} now resolves to {}",
            path.display(),
            current.display()
        )));
    }
    fs::remove_dir_all(path).map_err(|error| {
        let prior = git_error
            .map(|error| format!("Git removal failed ({error}); "))
            .unwrap_or_default();
        WorktreeError::InvalidPath(format!(
            "{prior}direct deletion of verified path {} also failed: {error}. The project remains registered and can be retried",
            path.display()
        ))
    })?;
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Ok(_) => Err(WorktreeError::InvalidPath(format!(
            "direct deletion returned successfully but {} still exists; the project remains registered and can be retried",
            path.display()
        ))),
        Err(error) => Err(WorktreeError::InvalidPath(format!(
            "could not verify deletion of {}; the project remains registered and can be retried: {error}",
            path.display()
        ))),
    }
}

fn ensure_verified_directory_still_absent(path: &Path) -> WorktreeResult<()> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Ok(_) => Err(WorktreeError::InvalidPath(format!(
            "the deleted project path {} reappeared during local Git cleanup; refusing to delete the replacement. The project remains registered and can be retried",
            path.display()
        ))),
        Err(error) => Err(WorktreeError::InvalidPath(format!(
            "could not perform the final deletion check for {}; the project remains registered and can be retried: {error}",
            path.display()
        ))),
    }
}

fn normalized_registered_path(path: &Path) -> PathBuf {
    let normalized = path.components().collect::<PathBuf>();
    if normalized.as_os_str().is_empty() {
        path.to_path_buf()
    } else {
        normalized
    }
}

async fn delete_target_safety(
    target: &VerifiedDeleteTarget,
    environment: &BTreeMap<OsString, OsString>,
) -> WorktreeResult<Option<WorktreeDeleteSafety>> {
    let Some(repository_root) = target.repository_root.as_deref() else {
        return Ok(None);
    };
    if target.recovering_partial_removal {
        return Ok(Some(WorktreeDeleteSafety {
            dirty_files: 0,
            untracked_files: 0,
            dirty_paths: Vec::new(),
            ignored_files: 0,
            ignored_paths: Vec::new(),
            unpushed_commits: 0,
            unpushed_subjects: Vec::new(),
            unmerged_commits: 0,
            unmerged_subjects: Vec::new(),
            upstream: None,
            push_target: None,
            merge_target: target.branch.clone(),
            dependent_worktrees: Vec::new(),
            requires_force: true,
        }));
    }
    let mut safety =
        worktree_delete_safety(&target.path, repository_root, &target.branch, environment).await?;
    safety
        .dependent_worktrees
        .clone_from(&target.dependent_worktrees);
    safety.requires_force |= !safety.dependent_worktrees.is_empty();
    Ok(Some(safety))
}

fn require_delete_confirmation(
    target: &VerifiedDeleteTarget,
    safety: Option<&WorktreeDeleteSafety>,
    request: &RemoveWorktree,
) -> WorktreeResult<()> {
    if safety.is_some_and(|safety| safety.requires_force) && !request.force_dirty {
        if target.recovering_partial_removal {
            return Err(WorktreeError::Dirty(format!(
                "Git already dropped metadata for a partially removed worktree at {}; explicitly force deletion to remove the remaining verified directory",
                target.path.display()
            )));
        }
        return Err(WorktreeError::Dirty(delete_safety_warning(
            &target.path,
            &target.branch,
            safety.expect("force requirement came from safety"),
        )));
    }
    Ok(())
}

async fn worktree_delete_safety(
    path: &Path,
    repository_root: &Path,
    branch: &str,
    environment: &BTreeMap<OsString, OsString>,
) -> WorktreeResult<WorktreeDeleteSafety> {
    let status = git_required_bytes(
        path,
        [
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--ignored=matching",
        ],
        "inspect worktree",
        environment,
    )
    .await?;
    let (dirty_paths, untracked_files, ignored_paths) = parse_status_paths(&status);
    let merge_target = default_merge_target(repository_root, environment).await?;
    let upstream = git_optional(
        path,
        [
            "rev-parse",
            "--abbrev-ref",
            "--symbolic-full-name",
            "@{upstream}",
        ],
        environment,
    )
    .await?
    .filter(|value| !value.is_empty());
    let remote_branch = format!("origin/{branch}");
    let push_target = if let Some(upstream) = upstream.as_deref() {
        Some(upstream.to_owned())
    } else if git_success(
        repository_root,
        [
            "show-ref",
            "--verify",
            "--quiet",
            format!("refs/remotes/{remote_branch}").as_str(),
        ],
        environment,
    )
    .await?
    {
        Some(remote_branch)
    } else {
        None
    };
    let unpushed_commits = rev_list_count(
        path,
        push_target.as_deref().unwrap_or(&merge_target),
        environment,
    )
    .await?;
    let unpushed_subjects = rev_list_subjects(
        path,
        push_target.as_deref().unwrap_or(&merge_target),
        environment,
    )
    .await?;
    let unmerged_commits = rev_list_count(path, &merge_target, environment).await?;
    let unmerged_subjects = rev_list_subjects(path, &merge_target, environment).await?;
    let dirty_files = dirty_paths.len();
    let ignored_files = ignored_paths.len();
    Ok(WorktreeDeleteSafety {
        dirty_files,
        untracked_files,
        dirty_paths,
        ignored_files,
        ignored_paths,
        unpushed_commits,
        unpushed_subjects,
        unmerged_commits,
        unmerged_subjects,
        upstream,
        push_target,
        merge_target,
        dependent_worktrees: Vec::new(),
        requires_force: dirty_files > 0
            || ignored_files > 0
            || unpushed_commits > 0
            || unmerged_commits > 0,
    })
}

fn parse_status_paths(status: &[u8]) -> (Vec<String>, usize, Vec<String>) {
    let mut paths = Vec::new();
    let mut untracked = 0;
    let mut ignored_paths = Vec::new();
    let mut skip_rename_source = false;
    for record in status
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
    {
        if skip_rename_source {
            skip_rename_source = false;
            continue;
        }
        if record.len() < 4 || record[2] != b' ' {
            continue;
        }
        let x = record[0];
        let y = record[1];
        if !is_porcelain_status(x) || !is_porcelain_status(y) {
            continue;
        }
        if x == b'!' && y == b'!' {
            ignored_paths.push(String::from_utf8_lossy(&record[3..]).into_owned());
            continue;
        }
        if x == b'?' && y == b'?' {
            untracked += 1;
        }
        skip_rename_source = matches!(x, b'R' | b'C') || matches!(y, b'R' | b'C');
        paths.push(String::from_utf8_lossy(&record[3..]).into_owned());
    }
    (paths, untracked, ignored_paths)
}

fn is_porcelain_status(value: u8) -> bool {
    matches!(
        value,
        b' ' | b'M' | b'T' | b'A' | b'D' | b'R' | b'C' | b'U' | b'?' | b'!'
    )
}

async fn default_merge_target(
    repository_root: &Path,
    environment: &BTreeMap<OsString, OsString>,
) -> WorktreeResult<String> {
    // The symbolic ref can survive after its target was deleted, so verify
    // the actual commit before using it as a safety base.
    if let Some(remote_head) = git_optional(
        repository_root,
        [
            "symbolic-ref",
            "--quiet",
            "--short",
            "refs/remotes/origin/HEAD",
        ],
        environment,
    )
    .await?
    .filter(|value| !value.is_empty())
    .filter(|remote_head| !remote_head.chars().any(char::is_whitespace))
        && git_success(
            repository_root,
            ["rev-parse", "--verify", "--quiet", remote_head.as_str()],
            environment,
        )
        .await?
    {
        return Ok(remote_head);
    }
    let primary_branch = git_optional(
        repository_root,
        ["symbolic-ref", "--quiet", "--short", "HEAD"],
        environment,
    )
    .await?
    .filter(|value| !value.is_empty());
    if let Some(primary_branch) = primary_branch {
        let remote_primary = format!("origin/{primary_branch}");
        if git_success(
            repository_root,
            [
                "show-ref",
                "--verify",
                "--quiet",
                format!("refs/remotes/{remote_primary}").as_str(),
            ],
            environment,
        )
        .await?
        {
            Ok(remote_primary)
        } else {
            Ok(primary_branch)
        }
    } else {
        git_required(
            repository_root,
            ["rev-parse", "--verify", "HEAD"],
            "resolve primary checkout commit",
            environment,
        )
        .await
    }
}

async fn rev_list_count(
    path: &Path,
    base: &str,
    environment: &BTreeMap<OsString, OsString>,
) -> WorktreeResult<usize> {
    let range = format!("{base}..HEAD");
    let count = git_required(
        path,
        ["rev-list", "--count", range.as_str()],
        "inspect worktree commits",
        environment,
    )
    .await?;
    count.parse::<usize>().map_err(|_| WorktreeError::Git {
        operation: "inspect worktree commits".into(),
        message: format!("Git returned an invalid commit count {count:?}"),
    })
}

async fn rev_list_subjects(
    path: &Path,
    base: &str,
    environment: &BTreeMap<OsString, OsString>,
) -> WorktreeResult<Vec<String>> {
    let range = format!("{base}..HEAD");
    let subjects = git_required(
        path,
        ["log", "--format=%s", "--max-count=3", range.as_str()],
        "inspect worktree commit subjects",
        environment,
    )
    .await?;
    Ok(subjects.lines().map(str::to_owned).collect())
}

async fn git_optional<I, S>(
    directory: &Path,
    args: I,
    environment: &BTreeMap<OsString, OsString>,
) -> WorktreeResult<Option<String>>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    let output = git_output(directory, args, Duration::from_secs(10), environment).await?;
    Ok(output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned()))
}

fn delete_safety_warning(path: &Path, branch: &str, safety: &WorktreeDeleteSafety) -> String {
    let mut reasons = Vec::new();
    if safety.dirty_files > 0 {
        let mut paths = safety
            .dirty_paths
            .iter()
            .take(8)
            .cloned()
            .collect::<Vec<_>>();
        if safety.dirty_paths.len() > paths.len() {
            paths.push(format!(
                "… and {} more",
                safety.dirty_paths.len() - paths.len()
            ));
        }
        reasons.push(format!(
            "{} dirty file(s), including {} untracked: {}",
            safety.dirty_files,
            safety.untracked_files,
            paths.join(", ")
        ));
    }
    if safety.ignored_files > 0 {
        let mut paths = safety
            .ignored_paths
            .iter()
            .take(8)
            .cloned()
            .collect::<Vec<_>>();
        if safety.ignored_paths.len() > paths.len() {
            paths.push(format!(
                "… and {} more",
                safety.ignored_paths.len() - paths.len()
            ));
        }
        reasons.push(format!(
            "{} ignored local path(s) that Git would delete: {}",
            safety.ignored_files,
            paths.join(", ")
        ));
    }
    if safety.unpushed_commits > 0 {
        let mut reason = if let Some(push_target) = &safety.push_target {
            format!(
                "{} commit(s) not pushed to {push_target}",
                safety.unpushed_commits
            )
        } else {
            format!(
                "{} commit(s) have no branch upstream and are not present in {}",
                safety.unpushed_commits, safety.merge_target
            )
        };
        if !safety.unpushed_subjects.is_empty() {
            reason.push_str(&format!(": {}", safety.unpushed_subjects.join("; ")));
        }
        reasons.push(reason);
    }
    if safety.unmerged_commits > 0 {
        let mut reason = format!(
            "{} commit(s) not merged into {}",
            safety.unmerged_commits, safety.merge_target
        );
        if !safety.unmerged_subjects.is_empty() {
            reason.push_str(&format!(": {}", safety.unmerged_subjects.join("; ")));
        }
        reasons.push(reason);
    }
    if !safety.dependent_worktrees.is_empty() {
        reasons.push(format!(
            "{} linked worktree(s) depend on this primary checkout: {}",
            safety.dependent_worktrees.len(),
            safety.dependent_worktrees.join(", ")
        ));
    }
    format!(
        "refusing to delete project {branch:?} at {}: {}. Local files, commits, or dependent worktrees may be permanently lost; explicitly force deletion to proceed",
        path.display(),
        reasons.join("; ")
    )
}

async fn worktree_status(
    record: &GitWorktree,
    environment: &BTreeMap<OsString, OsString>,
) -> WorktreeResult<&'static str> {
    if record.bare {
        return Ok("bare");
    }
    if record.prunable || !record.path.exists() {
        return Ok("missing");
    }
    let status = git_required(
        &record.path,
        ["status", "--porcelain", "--untracked-files=all"],
        "inspect worktree status",
        environment,
    )
    .await?;
    Ok(if status.is_empty() { "clean" } else { "dirty" })
}

fn matching_record<'a>(snapshot: &'a RepositorySnapshot, path: &Path) -> Option<&'a GitWorktree> {
    snapshot
        .worktrees
        .iter()
        .find(|record| same_path(&record.path, path))
}

fn display_branch(record: &GitWorktree) -> String {
    record.branch.clone().unwrap_or_else(|| {
        let short = record.head.chars().take(8).collect::<String>();
        format!(
            "detached@{}",
            if short.is_empty() { "unknown" } else { &short }
        )
    })
}

fn parse_origin_branches(output: &str) -> Vec<String> {
    output
        .lines()
        .filter_map(|line| line.split_whitespace().nth(1))
        .filter_map(|reference| reference.strip_prefix("refs/heads/"))
        .filter(|branch| !branch.is_empty())
        .map(str::to_owned)
        .collect()
}

fn parse_origin_default_ref(output: &str) -> Option<String> {
    output.lines().find_map(|line| {
        let reference = line
            .strip_prefix("ref: ")?
            .split_whitespace()
            .next()?
            .strip_prefix("refs/heads/")?;
        (!reference.is_empty() && !reference.chars().any(char::is_whitespace))
            .then(|| format!("origin/{reference}"))
    })
}

fn is_swm_managed_path(path: &Path, managed_root: &str, branch: &str) -> bool {
    let managed_root = absolute_path(PathBuf::from(managed_root));
    let path =
        workman_core::canonical_path(path).unwrap_or_else(|_| absolute_path(path.to_path_buf()));
    path == managed_root.join(site_slug(branch))
}

fn same_path(left: &Path, right: &Path) -> bool {
    canonical_display(left) == canonical_display(right)
}

fn canonical_display(path: &Path) -> String {
    workman_core::canonical_path(path)
        .unwrap_or_else(|_| absolute_path(path.to_path_buf()))
        .to_string_lossy()
        .into_owned()
}

fn absolute_path(path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        path
    } else {
        env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    }
}

async fn git_success<I, S>(
    directory: &Path,
    args: I,
    environment: &BTreeMap<OsString, OsString>,
) -> WorktreeResult<bool>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    Ok(
        git_output(directory, args, Duration::from_secs(10), environment)
            .await?
            .status
            .success(),
    )
}

async fn git_required<I, S>(
    directory: &Path,
    args: I,
    operation: &str,
    environment: &BTreeMap<OsString, OsString>,
) -> WorktreeResult<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    let output = git_output(directory, args, GIT_NETWORK_TIMEOUT, environment).await?;
    if !output.status.success() {
        return Err(git_failure(operation, &output));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

async fn git_required_bytes<I, S>(
    directory: &Path,
    args: I,
    operation: &str,
    environment: &BTreeMap<OsString, OsString>,
) -> WorktreeResult<Vec<u8>>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    let output = git_output(directory, args, GIT_NETWORK_TIMEOUT, environment).await?;
    if !output.status.success() {
        return Err(git_failure(operation, &output));
    }
    Ok(output.stdout)
}

async fn git_output<I, S>(
    directory: &Path,
    args: I,
    deadline: Duration,
    environment: &BTreeMap<OsString, OsString>,
) -> WorktreeResult<Output>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    let mut command = Command::new(git_executable(environment)?);
    command
        .arg("-C")
        .arg(directory)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .kill_on_drop(true);
    command_output(&mut command, deadline, environment).await
}

async fn command_output(
    command: &mut Command,
    deadline: Duration,
    environment: &BTreeMap<OsString, OsString>,
) -> WorktreeResult<Output> {
    command
        .env_clear()
        .envs(environment)
        .env("GIT_TERMINAL_PROMPT", "0")
        .kill_on_drop(true);
    timeout(deadline, command.output())
        .await
        .map_err(|_| WorktreeError::Git {
            operation: "run Git command".into(),
            message: format!("timed out after {}s", deadline.as_secs()),
        })?
        .map_err(WorktreeError::Io)
}

fn std_git_required<I, S>(
    directory: &Path,
    args: I,
    operation: &str,
    environment: &BTreeMap<OsString, OsString>,
) -> WorktreeResult<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    let output = std_git_output(directory, args, environment)?;
    if !output.status.success() {
        return Err(git_failure(operation, &output));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn std_git_output<I, S>(
    directory: &Path,
    args: I,
    environment: &BTreeMap<OsString, OsString>,
) -> WorktreeResult<Output>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    Ok(StdCommand::new(git_executable(environment)?)
        .arg("-C")
        .arg(directory)
        .args(args)
        .env_clear()
        .envs(environment)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()?)
}

fn git_executable(environment: &BTreeMap<OsString, OsString>) -> WorktreeResult<PathBuf> {
    // Windows reports the variable as `Path` and launches `git.exe` through
    // PATHEXT, so resolution goes through the runtime doctor's shared lookup.
    let path = crate::runtime_doctor::path_variable(environment);
    if path.is_empty() {
        return Err(WorktreeError::Git {
            operation: "resolve Git executable".into(),
            message: "resolved user environment has no PATH".into(),
        });
    }
    crate::runtime_doctor::resolve_executable("git", &path).ok_or_else(|| WorktreeError::Git {
        operation: "resolve Git executable".into(),
        message: "git was not found in the resolved user PATH".into(),
    })
}

async fn command_environment(registry: &SharedProcessRegistry) -> BTreeMap<OsString, OsString> {
    let resolved = {
        let registry = registry.lock().await;
        registry.resolved_user_environment()
    };
    resolved.command_environment()
}

fn git_failure(operation: &str, output: &Output) -> WorktreeError {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    WorktreeError::Git {
        operation: operation.to_owned(),
        message: if stderr.is_empty() { stdout } else { stderr },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swm_slug_semantics_keep_branch_but_bound_folder() {
        assert_eq!(site_slug("Feature/Foo_Bar"), "feature-foo-bar");
        assert_eq!(site_slug("---"), "");
        assert_eq!(site_slug(&format!("feature/{}", "a".repeat(100))).len(), 63);
    }

    #[test]
    fn porcelain_parser_preserves_branch_slashes_and_flags() {
        let rows = b"worktree /tmp/main\0HEAD aaaa\0branch refs/heads/main\0\0worktree /tmp/wt\0HEAD bbbb\0branch refs/heads/feature/x\0locked reason\0\0";
        let parsed = parse_porcelain(rows).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[1].branch.as_deref(), Some("feature/x"));
        assert!(parsed[1].locked);
    }

    #[test]
    fn origin_branch_parser_preserves_nested_names() {
        let parsed = parse_origin_branches(
            "aaaa\trefs/heads/main\nbbbb\trefs/heads/feature/worktree-ui\ninvalid\trefs/tags/v1\n",
        );
        assert_eq!(parsed, vec!["main", "feature/worktree-ui"]);
    }

    #[test]
    fn origin_default_parser_reads_symbolic_head_without_hardcoding_branch() {
        assert_eq!(
            parse_origin_default_ref(
                "ref: refs/heads/trunk\tHEAD\naaaa\tHEAD\nref: refs/heads/ignored\tOTHER\n"
            ),
            Some("origin/trunk".into())
        );
        assert_eq!(parse_origin_default_ref("aaaa\tHEAD\n"), None);
    }

    #[test]
    fn status_parser_counts_untracked_files_and_skips_rename_sources() {
        let status = b" M tracked.txt\0 T executable.txt\0?? untracked.txt\0R  renamed.txt\0old-name.txt\0!! .env\0";
        let (paths, untracked, ignored) = parse_status_paths(status);
        assert_eq!(
            paths,
            [
                "tracked.txt",
                "executable.txt",
                "untracked.txt",
                "renamed.txt"
            ]
        );
        assert_eq!(untracked, 1);
        assert_eq!(ignored, [".env"]);
    }

    #[test]
    fn env_rewrite_adds_missing_keys_and_deduplicates_existing_ones() {
        let (rewritten, name, url) = rewrite_environment(
            "SECRET=fixture\nAPP_NAME=old\nAPP_NAME=duplicate\n",
            "feature-name",
            Some("http://feature-name.test"),
        );
        assert!(name && url);
        assert_eq!(
            rewritten,
            "SECRET=fixture\nAPP_NAME=\"feature-name\"\nAPP_URL=http://feature-name.test\n"
        );
    }
}
