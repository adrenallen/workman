//! Git worktree discovery and lifecycle management.
//!
//! The behavior intentionally mirrors the standalone SWM tool: a branch is a
//! project, the first porcelain worktree identifies the repository, existing
//! local/remote branches are checked out rather than recreated, and removal is
//! reserved for worktrees awm (or a faithfully detected SWM predecessor) owns.

use std::{
    collections::{BTreeMap, HashMap},
    env, fmt, io,
    path::{Path, PathBuf},
    process::{Command as StdCommand, Output},
    time::Duration,
};

use awm_core::{
    ProcessStatus, Project, ProjectId, ProjectWorktree, Store, StoreError, WorktreeRepository,
};
use serde::Serialize;
use tokio::{process::Command, time::timeout};

use crate::{RegistryError, SharedProcessRegistry};

pub const AWM_WORKTREE_ROOT_ENV: &str = "AWM_WORKTREE_ROOT";
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
    Conflict(String),
    Confirmation(String),
    Dirty(String),
    Foreign(String),
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
            Self::Conflict(_) => "worktree_conflict",
            Self::Confirmation(_) => "confirmation_required",
            Self::Dirty(_) => "dirty_worktree",
            Self::Foreign(_) => "foreign_worktree",
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
            | Self::Foreign(message) => formatter.write_str(message),
            Self::Git { operation, message } => write!(formatter, "{operation}: {message}"),
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
    pub locked: bool,
    pub prunable: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct WorktreeList {
    pub repository: RepositoryView,
    pub worktrees: Vec<WorktreeEntry>,
}

#[derive(Clone, Debug, Serialize)]
pub struct WorktreeMutation {
    pub repository: RepositoryView,
    pub project: ProjectEnvelope,
    pub worktree: WorktreeEntry,
}

#[derive(Clone, Debug, Serialize)]
pub struct WorktreeRemoval {
    pub project_id: ProjectId,
    pub path: String,
    pub branch: String,
    pub removed: bool,
    pub project_unregistered: bool,
    pub branch_kept: bool,
}

#[derive(Clone, Debug)]
pub struct CreateWorktree {
    pub source_project_id: ProjectId,
    pub branch: String,
    pub from_ref: Option<String>,
    pub managed_root: Option<PathBuf>,
    pub preferences: BTreeMap<String, String>,
}

#[derive(Clone, Debug)]
pub struct AdoptWorktree {
    pub path: PathBuf,
    pub preferences: BTreeMap<String, String>,
}

#[derive(Clone, Debug)]
pub struct RemoveWorktree {
    pub project_id: ProjectId,
    pub confirm_remove: bool,
    pub confirm_stop_running: bool,
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

/// Root convention shared with SWM, with an awm-native override.
pub fn default_worktree_root() -> PathBuf {
    if let Some(root) = env::var_os(AWM_WORKTREE_ROOT_ENV).filter(|value| !value.is_empty()) {
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

/// Populate metadata for pre-existing SWM/awm projects without touching Git or project rows.
pub fn reconcile_existing_projects(store: &Store) -> WorktreeResult<()> {
    let projects = store.list_projects()?;
    for project in &projects {
        if store.get_project_worktree(project.id)?.is_some() {
            continue;
        }
        let Ok(snapshot) = snapshot_sync(Path::new(&project.path)) else {
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
    let (project, repository, registered, links) = {
        let registry = registry.lock().await;
        let project = registry.store().get_project(project_id)?.ok_or_else(|| {
            WorktreeError::InvalidProject(format!("project {project_id} was not found"))
        })?;
        if registry.store().get_project_worktree(project_id)?.is_none() {
            reconcile_existing_projects(registry.store())?;
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

    let snapshot = snapshot_async(Path::new(&project.path)).await?;
    list_from_snapshot(registry, repository, snapshot, registered, links).await
}

pub async fn create(
    registry: &SharedProcessRegistry,
    request: CreateWorktree,
) -> WorktreeResult<WorktreeMutation> {
    validate_branch(&request.branch).await?;
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
    let snapshot = snapshot_async(Path::new(&source_project.path)).await?;

    let mut repository = {
        let registry = registry.lock().await;
        ensure_repository(registry.store(), &snapshot, request.managed_root.as_deref())?
    };
    let managed_root = if let Some(root) = request.managed_root {
        root
    } else {
        PathBuf::from(&repository.managed_root)
    };
    std::fs::create_dir_all(&managed_root)?;
    let managed_root = std::fs::canonicalize(&managed_root)?;
    repository.managed_root = managed_root.to_string_lossy().into_owned();

    let slug = site_slug(&request.branch);
    if slug.is_empty() {
        return Err(WorktreeError::InvalidBranch(format!(
            "branch {:?} has no characters usable in a folder name",
            request.branch
        )));
    }
    let destination = managed_root.join(&slug);
    if destination.exists() {
        return Err(WorktreeError::Conflict(format!(
            "destination already exists: {}",
            destination.display()
        )));
    }

    let branch_state = branch_state(&snapshot, &request.branch).await?;
    if branch_state != BranchState::Missing && request.from_ref.is_some() {
        return Err(WorktreeError::Conflict(format!(
            "branch {:?} already exists and cannot also be created from another ref",
            request.branch
        )));
    }
    if let Some(existing) = snapshot
        .worktrees
        .iter()
        .find(|worktree| worktree.branch.as_deref() == Some(request.branch.as_str()))
    {
        return Err(WorktreeError::Conflict(format!(
            "branch {:?} is already checked out at {}",
            request.branch,
            existing.path.display()
        )));
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
                )
                .await?;
            } else {
                let refspec = format!("refs/heads/{0}:refs/remotes/origin/{0}", request.branch);
                let _ = git_output(
                    &snapshot.root_path,
                    ["fetch", "--quiet", "origin", refspec.as_str()],
                    GIT_NETWORK_TIMEOUT,
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
            )
            .await?;
        }
        BranchState::Missing => {
            let start =
                resolve_start_point(&snapshot.root_path, request.from_ref.as_deref()).await?;
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
            )
            .await?;
        }
    }

    let project = {
        let registry = registry.lock().await;
        registry.store().put_worktree_repository(&repository)?;
        for (key, value) in &request.preferences {
            validate_preference_key(key)?;
            registry
                .store()
                .set_worktree_preference(repository.id, key, Some(value))?;
        }
        register_project(
            registry.store(),
            &repository,
            &destination,
            &request.branch,
            true,
        )?
    };
    mutation_for_project(registry, project.id).await
}

pub async fn adopt(
    registry: &SharedProcessRegistry,
    request: AdoptWorktree,
) -> WorktreeResult<WorktreeMutation> {
    let canonical_input = std::fs::canonicalize(&request.path).map_err(|error| {
        WorktreeError::InvalidPath(format!(
            "could not open {}: {error}",
            request.path.display()
        ))
    })?;
    let top = git_required(
        &canonical_input,
        ["rev-parse", "--show-toplevel"],
        "resolve worktree root",
    )
    .await?;
    let top = std::fs::canonicalize(top.trim())?;
    let snapshot = snapshot_async(&top).await?;
    let record = matching_record(&snapshot, &top).ok_or_else(|| {
        WorktreeError::InvalidPath(format!("{} is not a listed Git worktree", top.display()))
    })?;
    let branch = display_branch(record);
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
        register_project(registry.store(), &repository, &top, &branch, false)?
    };
    mutation_for_project(registry, project.id).await
}

pub async fn remove(
    registry: &SharedProcessRegistry,
    request: RemoveWorktree,
) -> WorktreeResult<WorktreeRemoval> {
    if !request.confirm_remove {
        return Err(WorktreeError::Confirmation(
            "set confirm_remove=true to remove the worktree and unregister its project".into(),
        ));
    }
    let (project, link, repository, processes) = {
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
        let link = registry
            .store()
            .get_project_worktree(project.id)?
            .ok_or_else(|| WorktreeError::Foreign("project is not a managed worktree".into()))?;
        let repository = registry
            .store()
            .get_worktree_repository(link.repository_id)?
            .ok_or_else(|| {
                WorktreeError::InvalidPath("worktree repository metadata is missing".into())
            })?;
        let processes = registry.list(Some(project.id))?;
        (project, link, repository, processes)
    };
    if !link.managed {
        return Err(WorktreeError::Foreign(
            "refusing to remove an adopted or external worktree; only awm/SWM-managed worktrees can be deleted".into(),
        ));
    }

    let path = std::fs::canonicalize(&project.path).map_err(|error| {
        WorktreeError::InvalidPath(format!(
            "could not open managed worktree {}: {error}",
            project.path
        ))
    })?;
    let repository_root = std::fs::canonicalize(&repository.root_path)?;
    let managed_root = std::fs::canonicalize(&repository.managed_root)?;
    let expected = managed_root.join(site_slug(&link.branch));
    if path == repository_root || path != expected || !path.starts_with(&managed_root) {
        return Err(WorktreeError::Foreign(format!(
            "refusing to remove {}; it does not match awm's managed path for branch {:?}",
            path.display(),
            link.branch
        )));
    }
    let actual_top =
        git_required(&path, ["rev-parse", "--show-toplevel"], "verify worktree").await?;
    let actual_top = std::fs::canonicalize(actual_top.trim())?;
    if actual_top != path {
        return Err(WorktreeError::Foreign(format!(
            "refusing to remove unexpected Git directory {}",
            path.display()
        )));
    }

    let dirty = !git_required(
        &path,
        ["status", "--porcelain", "--untracked-files=all"],
        "inspect worktree",
    )
    .await?
    .is_empty();
    if dirty
        && (!request.force_dirty || request.confirm_branch.as_deref() != Some(link.branch.as_str()))
    {
        return Err(WorktreeError::Dirty(format!(
            "worktree has local changes; set force_dirty=true and confirm_branch={:?} to delete them",
            link.branch
        )));
    }
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

    // Quiesce awm-owned processes before removing their working directory.
    {
        let mut registry = registry.lock().await;
        for process in processes {
            registry.close(process.id)?;
        }
    }
    git_required(
        &repository_root,
        [
            "worktree",
            "remove",
            "--force",
            path.to_str().unwrap_or_default(),
        ],
        "remove managed worktree",
    )
    .await?;
    let branch_kept = git_success(
        &repository_root,
        [
            "show-ref",
            "--verify",
            "--quiet",
            format!("refs/heads/{}", link.branch).as_str(),
        ],
    )
    .await?;
    if !branch_kept {
        return Err(WorktreeError::Git {
            operation: "verify preserved branch".into(),
            message: format!(
                "branch {:?} disappeared during worktree removal",
                link.branch
            ),
        });
    }

    // Project deletion is intentionally last: even a self-targeting agent has
    // already finished the Git operation before its process can disappear.
    let project_unregistered = {
        let registry = registry.lock().await;
        registry.store().delete_project(project.id)?
    };
    Ok(WorktreeRemoval {
        project_id: project.id,
        path: path.to_string_lossy().into_owned(),
        branch: link.branch,
        removed: true,
        project_unregistered,
        branch_kept,
    })
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
    })
}

async fn list_from_snapshot(
    registry: &SharedProcessRegistry,
    repository: WorktreeRepository,
    snapshot: RepositorySnapshot,
    projects: Vec<Project>,
    links: Vec<ProjectWorktree>,
) -> WorktreeResult<WorktreeList> {
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
    let mut entries = Vec::with_capacity(snapshot.worktrees.len());
    for record in snapshot.worktrees {
        let path_key = canonical_display(&record.path);
        let project = project_by_path.get(&path_key).copied();
        let link = project.and_then(|project| link_by_project.get(&project.id).copied());
        let branch = display_branch(&record);
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
        let status = worktree_status(&record).await?;
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
            can_remove: managed && !is_main,
            locked: record.locked,
            prunable: record.prunable,
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
        },
        worktrees: entries,
    })
}

fn register_project(
    store: &Store,
    repository: &WorktreeRepository,
    path: &Path,
    branch: &str,
    managed: bool,
) -> WorktreeResult<Project> {
    let canonical = std::fs::canonicalize(path)?;
    let canonical_string = canonical.to_string_lossy().into_owned();
    let existing = store
        .list_projects()?
        .into_iter()
        .find(|project| same_path(Path::new(&project.path), &canonical));
    let project = if let Some(project) = existing {
        project
    } else {
        let project = Project {
            id: store.next_project_id()?,
            path: canonical_string,
            name: format!("{}: {branch}", repository.name),
            display_name: None,
            icon: None,
            selected: false,
            sort_order: store.next_project_sort_order()?,
        };
        store.put_project(&project)?;
        project
    };
    let root_project_id = store
        .list_projects()?
        .into_iter()
        .find(|candidate| same_path(Path::new(&candidate.path), Path::new(&repository.root_path)))
        .map(|candidate| candidate.id);
    let existing_link = store.get_project_worktree(project.id)?;
    store.put_project_worktree(&ProjectWorktree {
        project_id: project.id,
        repository_id: repository.id,
        parent_project_id: (!same_path(&canonical, Path::new(&repository.root_path)))
            .then_some(root_project_id)
            .flatten(),
        branch: branch.to_owned(),
        managed: existing_link.map(|link| link.managed).unwrap_or(managed),
    })?;
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

async fn branch_state(snapshot: &RepositorySnapshot, branch: &str) -> WorktreeResult<BranchState> {
    if git_success(
        &snapshot.root_path,
        [
            "show-ref",
            "--verify",
            "--quiet",
            format!("refs/heads/{branch}").as_str(),
        ],
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
    )
    .await?
    {
        return Ok(BranchState::Remote);
    }
    if !git_success(&snapshot.root_path, ["remote", "get-url", "origin"]).await? {
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
    )
    .await?;
    Ok(if output.status.success() {
        BranchState::RemoteUnfetched
    } else {
        BranchState::Missing
    })
}

async fn resolve_start_point(repository: &Path, requested: Option<&str>) -> WorktreeResult<String> {
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
    if simple_branch && git_success(repository, ["remote", "get-url", "origin"]).await? {
        let _ = git_output(
            repository,
            ["fetch", "--quiet", "origin", from_ref.as_str()],
            GIT_NETWORK_TIMEOUT,
        )
        .await;
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
    )
    .await?
    {
        return Ok(remote);
    }
    Err(WorktreeError::InvalidBranch(format!(
        "cannot find branch-from ref {from_ref:?}"
    )))
}

async fn validate_branch(branch: &str) -> WorktreeResult<()> {
    if branch.trim() != branch || branch.is_empty() {
        return Err(WorktreeError::InvalidBranch(
            "worktree branch must not be empty or padded with whitespace".into(),
        ));
    }
    let output = command_output(
        Command::new("git").args(["check-ref-format", "--branch", branch]),
        Duration::from_secs(5),
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

async fn snapshot_async(path: &Path) -> WorktreeResult<RepositorySnapshot> {
    let top = git_required(path, ["rev-parse", "--show-toplevel"], "resolve repository").await?;
    let top = std::fs::canonicalize(top.trim())?;
    let porcelain = git_required_bytes(
        &top,
        ["worktree", "list", "--porcelain", "-z"],
        "list worktrees",
    )
    .await?;
    snapshot_from_porcelain(top, &porcelain)
}

fn snapshot_sync(path: &Path) -> WorktreeResult<RepositorySnapshot> {
    let top = std_git_required(path, ["rev-parse", "--show-toplevel"], "resolve repository")?;
    let top = std::fs::canonicalize(top.trim())?;
    let porcelain = std_git_output(&top, ["worktree", "list", "--porcelain", "-z"])?;
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
    let root_path = std::fs::canonicalize(&root_path).unwrap_or(root_path);
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

async fn worktree_status(record: &GitWorktree) -> WorktreeResult<&'static str> {
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

fn is_swm_managed_path(path: &Path, managed_root: &str, branch: &str) -> bool {
    let managed_root = absolute_path(PathBuf::from(managed_root));
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| absolute_path(path.to_path_buf()));
    path == managed_root.join(site_slug(branch))
}

fn same_path(left: &Path, right: &Path) -> bool {
    canonical_display(left) == canonical_display(right)
}

fn canonical_display(path: &Path) -> String {
    std::fs::canonicalize(path)
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

async fn git_success<I, S>(directory: &Path, args: I) -> WorktreeResult<bool>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    Ok(git_output(directory, args, Duration::from_secs(10))
        .await?
        .status
        .success())
}

async fn git_required<I, S>(directory: &Path, args: I, operation: &str) -> WorktreeResult<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    let output = git_output(directory, args, GIT_NETWORK_TIMEOUT).await?;
    if !output.status.success() {
        return Err(git_failure(operation, &output));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

async fn git_required_bytes<I, S>(
    directory: &Path,
    args: I,
    operation: &str,
) -> WorktreeResult<Vec<u8>>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    let output = git_output(directory, args, GIT_NETWORK_TIMEOUT).await?;
    if !output.status.success() {
        return Err(git_failure(operation, &output));
    }
    Ok(output.stdout)
}

async fn git_output<I, S>(directory: &Path, args: I, deadline: Duration) -> WorktreeResult<Output>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(directory)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .kill_on_drop(true);
    command_output(&mut command, deadline).await
}

async fn command_output(command: &mut Command, deadline: Duration) -> WorktreeResult<Output> {
    timeout(deadline, command.output())
        .await
        .map_err(|_| WorktreeError::Git {
            operation: "run Git command".into(),
            message: format!("timed out after {}s", deadline.as_secs()),
        })?
        .map_err(WorktreeError::Io)
}

fn std_git_required<I, S>(directory: &Path, args: I, operation: &str) -> WorktreeResult<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    let output = std_git_output(directory, args)?;
    if !output.status.success() {
        return Err(git_failure(operation, &output));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn std_git_output<I, S>(directory: &Path, args: I) -> WorktreeResult<Output>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    Ok(StdCommand::new("git")
        .arg("-C")
        .arg(directory)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()?)
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
}
