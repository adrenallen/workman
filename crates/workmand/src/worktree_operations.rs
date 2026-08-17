//! Async desktop worktree operations and their status-stream progress snapshots.

use std::{
    collections::{BTreeMap, VecDeque},
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use workman_core::ProjectId;

use crate::{SharedProcessRegistry, status_invalidation::StatusInvalidationHub, worktrees};

const MAX_OPERATIONS: usize = 64;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WorktreeOperationMode {
    Create,
    Fork,
    Adopt,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WorktreeOperationStatus {
    Running,
    Completed,
    Failed,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WorktreeStepId {
    Branch,
    Worktree,
    Environment,
    Herd,
    Registered,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WorktreeStepStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Skipped,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct WorktreeOperationStep {
    id: WorktreeStepId,
    label: &'static str,
    status: WorktreeStepStatus,
    detail: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct WorktreeOperation {
    id: String,
    mode: WorktreeOperationMode,
    source_project_id: Option<ProjectId>,
    repository_id: Option<i64>,
    branch: Option<String>,
    path: Option<String>,
    label: String,
    status: WorktreeOperationStatus,
    steps: Vec<WorktreeOperationStep>,
    error: Option<String>,
    project: Option<worktrees::ProjectEnvelope>,
    created_at: u64,
    updated_at: u64,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct WorktreeOperationAck {
    pub operation_id: String,
    pub accepted: bool,
}

#[derive(Clone)]
pub(crate) struct WorktreeOperationHub {
    inner: Arc<Mutex<VecDeque<WorktreeOperation>>>,
    status_invalidations: StatusInvalidationHub,
}

impl Default for WorktreeOperationHub {
    fn default() -> Self {
        Self::new(StatusInvalidationHub::default())
    }
}

impl WorktreeOperationHub {
    pub(crate) fn new(status_invalidations: StatusInvalidationHub) -> Self {
        Self {
            inner: Arc::new(Mutex::new(VecDeque::new())),
            status_invalidations,
        }
    }

    pub(crate) fn snapshot(&self) -> Vec<WorktreeOperation> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .cloned()
            .collect()
    }

    fn begin(&self, operation: WorktreeOperation) -> Result<(), StartError> {
        let mut operations = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if operations
            .iter()
            .any(|candidate| candidate.id == operation.id)
        {
            return Err(StartError::new(
                "worktree_operation_exists",
                "that worktree operation is already running",
            ));
        }
        operations.push_front(operation);
        operations.truncate(MAX_OPERATIONS);
        drop(operations);
        self.status_invalidations.invalidate();
        Ok(())
    }

    fn update(&self, id: &str, mutate: impl FnOnce(&mut WorktreeOperation)) {
        let mut operations = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let changed =
            if let Some(operation) = operations.iter_mut().find(|candidate| candidate.id == id) {
                mutate(operation);
                operation.updated_at = now_millis();
                true
            } else {
                false
            };
        drop(operations);
        if changed {
            self.status_invalidations.invalidate();
        }
    }
}

#[derive(Clone)]
pub(crate) struct WorktreeOperationReporter {
    hub: WorktreeOperationHub,
    operation_id: String,
}

impl WorktreeOperationReporter {
    pub(crate) fn running(&self, id: WorktreeStepId, detail: impl Into<Option<String>>) {
        let detail = detail.into();
        self.hub.update(&self.operation_id, |operation| {
            operation.status = WorktreeOperationStatus::Running;
            if let Some(step) = operation.steps.iter_mut().find(|step| step.id == id) {
                step.status = WorktreeStepStatus::Running;
                step.detail = detail;
            }
        });
    }

    pub(crate) fn completed(&self, id: WorktreeStepId, detail: impl Into<Option<String>>) {
        let detail = detail.into();
        self.hub.update(&self.operation_id, |operation| {
            if let Some(step) = operation.steps.iter_mut().find(|step| step.id == id) {
                step.status = WorktreeStepStatus::Completed;
                step.detail = detail;
            }
        });
    }

    pub(crate) fn skipped(&self, id: WorktreeStepId, detail: impl Into<Option<String>>) {
        let detail = detail.into();
        self.hub.update(&self.operation_id, |operation| {
            if let Some(step) = operation.steps.iter_mut().find(|step| step.id == id) {
                step.status = WorktreeStepStatus::Skipped;
                step.detail = detail;
            }
        });
    }

    fn finish(&self, mutation: &worktrees::WorktreeMutation) {
        self.hub.update(&self.operation_id, |operation| {
            operation.status = WorktreeOperationStatus::Completed;
            operation.error = None;
            operation.project = Some(mutation.project.clone());
            operation.repository_id = Some(mutation.repository.id);
            operation.path = Some(mutation.worktree.path.clone());
            for step in &mut operation.steps {
                if matches!(
                    step.status,
                    WorktreeStepStatus::Pending | WorktreeStepStatus::Running
                ) {
                    step.status = WorktreeStepStatus::Completed;
                }
            }
        });
    }

    fn fail(&self, message: String) {
        self.hub.update(&self.operation_id, |operation| {
            operation.status = WorktreeOperationStatus::Failed;
            operation.error = Some(message.clone());
            if let Some(step) = operation
                .steps
                .iter_mut()
                .find(|step| step.status == WorktreeStepStatus::Running)
            {
                step.status = WorktreeStepStatus::Failed;
                step.detail = Some(message);
            }
        });
    }
}

#[derive(Debug)]
pub(crate) struct StartError {
    pub code: &'static str,
    pub message: String,
}

impl StartError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct CreateParams {
    operation_id: String,
    project_id: ProjectId,
    branch: String,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    from_ref: Option<String>,
    #[serde(default)]
    managed_root: Option<String>,
    #[serde(default)]
    preferences: BTreeMap<String, String>,
    #[serde(default)]
    env_policy: Option<worktrees::EnvPortPolicy>,
    #[serde(default)]
    remember_env_policy: bool,
}

#[derive(Debug, Deserialize)]
struct ForkParams {
    operation_id: String,
    project_id: ProjectId,
    branch: String,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    managed_root: Option<String>,
    #[serde(default)]
    preferences: BTreeMap<String, String>,
    #[serde(default)]
    env_policy: Option<worktrees::EnvPortPolicy>,
    #[serde(default)]
    remember_env_policy: bool,
}

#[derive(Debug, Deserialize)]
struct AdoptParams {
    operation_id: String,
    path: String,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    preferences: BTreeMap<String, String>,
}

pub(crate) async fn start(
    method: &str,
    params: Value,
    registry: SharedProcessRegistry,
    hub: WorktreeOperationHub,
) -> Result<WorktreeOperationAck, StartError> {
    match method {
        "worktree.create_async" => {
            let params: CreateParams = parse_params(params)?;
            validate_operation_id(&params.operation_id)?;
            let operation_id = params.operation_id.clone();
            let repository_id = repository_id_for(&registry, params.project_id).await;
            let operation = new_operation(
                operation_id.clone(),
                WorktreeOperationMode::Create,
                Some(params.project_id),
                repository_id,
                Some(params.branch.clone()),
                None,
            );
            hub.begin(operation)?;
            let reporter = WorktreeOperationReporter {
                hub: hub.clone(),
                operation_id: operation_id.clone(),
            };
            tokio::spawn(async move {
                let result = worktrees::create_with_progress(
                    &registry,
                    worktrees::CreateWorktree {
                        source_project_id: params.project_id,
                        branch: params.branch,
                        display_name: params.display_name,
                        from_ref: params.from_ref,
                        managed_root: params.managed_root.map(PathBuf::from),
                        preferences: params.preferences,
                        env_policy: params.env_policy,
                        remember_env_policy: params.remember_env_policy,
                    },
                    Some(&reporter),
                )
                .await;
                finish_operation(reporter, result);
            });
            Ok(WorktreeOperationAck {
                operation_id,
                accepted: true,
            })
        }
        "worktree.fork_async" => {
            let params: ForkParams = parse_params(params)?;
            validate_operation_id(&params.operation_id)?;
            let operation_id = params.operation_id.clone();
            let repository_id = repository_id_for(&registry, params.project_id).await;
            let operation = new_operation(
                operation_id.clone(),
                WorktreeOperationMode::Fork,
                Some(params.project_id),
                repository_id,
                Some(params.branch.clone()),
                None,
            );
            hub.begin(operation)?;
            let reporter = WorktreeOperationReporter {
                hub: hub.clone(),
                operation_id: operation_id.clone(),
            };
            tokio::spawn(async move {
                let result = worktrees::fork_with_progress(
                    &registry,
                    worktrees::ForkWorktree {
                        source_project_id: params.project_id,
                        branch: params.branch,
                        display_name: params.display_name,
                        managed_root: params.managed_root.map(PathBuf::from),
                        preferences: params.preferences,
                        env_policy: params.env_policy,
                        remember_env_policy: params.remember_env_policy,
                    },
                    Some(&reporter),
                )
                .await;
                finish_operation(reporter, result);
            });
            Ok(WorktreeOperationAck {
                operation_id,
                accepted: true,
            })
        }
        "worktree.adopt_async" => {
            let params: AdoptParams = parse_params(params)?;
            validate_operation_id(&params.operation_id)?;
            let operation_id = params.operation_id.clone();
            let operation = new_operation(
                operation_id.clone(),
                WorktreeOperationMode::Adopt,
                None,
                None,
                None,
                Some(params.path.clone()),
            );
            hub.begin(operation)?;
            let reporter = WorktreeOperationReporter {
                hub: hub.clone(),
                operation_id: operation_id.clone(),
            };
            tokio::spawn(async move {
                let result = worktrees::adopt_with_progress(
                    &registry,
                    worktrees::AdoptWorktree {
                        path: PathBuf::from(params.path),
                        display_name: params.display_name,
                        preferences: params.preferences,
                    },
                    Some(&reporter),
                )
                .await;
                finish_operation(reporter, result);
            });
            Ok(WorktreeOperationAck {
                operation_id,
                accepted: true,
            })
        }
        _ => Err(StartError::new(
            "method_not_found",
            "unknown async worktree operation",
        )),
    }
}

fn finish_operation(
    reporter: WorktreeOperationReporter,
    result: worktrees::WorktreeResult<worktrees::WorktreeMutation>,
) {
    match result {
        Ok(mutation) => reporter.finish(&mutation),
        Err(error) => reporter.fail(error.to_string()),
    }
}

fn parse_params<T: for<'de> Deserialize<'de>>(params: Value) -> Result<T, StartError> {
    serde_json::from_value(params)
        .map_err(|error| StartError::new("invalid_params", error.to_string()))
}

fn validate_operation_id(id: &str) -> Result<(), StartError> {
    if id.is_empty()
        || id.len() > 128
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err(StartError::new(
            "invalid_params",
            "operation_id must be 1-128 letters, numbers, hyphens, or underscores",
        ));
    }
    Ok(())
}

async fn repository_id_for(registry: &SharedProcessRegistry, project_id: ProjectId) -> Option<i64> {
    registry
        .lock()
        .await
        .store()
        .get_project_worktree(project_id)
        .ok()
        .flatten()
        .map(|link| link.repository_id)
}

fn new_operation(
    id: String,
    mode: WorktreeOperationMode,
    source_project_id: Option<ProjectId>,
    repository_id: Option<i64>,
    branch: Option<String>,
    path: Option<String>,
) -> WorktreeOperation {
    let label = match mode {
        WorktreeOperationMode::Adopt => path
            .as_deref()
            .and_then(|path| {
                PathBuf::from(path)
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
            })
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| "Adopting worktree".into()),
        WorktreeOperationMode::Create | WorktreeOperationMode::Fork => {
            branch.clone().unwrap_or_else(|| "Creating worktree".into())
        }
    };
    let now = now_millis();
    WorktreeOperation {
        id,
        mode,
        source_project_id,
        repository_id,
        branch,
        path,
        label,
        status: WorktreeOperationStatus::Running,
        steps: initial_steps(mode),
        error: None,
        project: None,
        created_at: now,
        updated_at: now,
    }
}

fn initial_steps(mode: WorktreeOperationMode) -> Vec<WorktreeOperationStep> {
    let labels = [
        (
            WorktreeStepId::Branch,
            if mode == WorktreeOperationMode::Adopt {
                "Worktree inspected"
            } else {
                "Branch created"
            },
        ),
        (WorktreeStepId::Worktree, "Worktree added"),
        (WorktreeStepId::Environment, ".env ported"),
        (WorktreeStepId::Herd, "Herd parked"),
        (WorktreeStepId::Registered, "Project registered"),
    ];
    labels
        .into_iter()
        .enumerate()
        .map(|(index, (id, label))| WorktreeOperationStep {
            id,
            label,
            status: if index == 0 {
                WorktreeStepStatus::Running
            } else {
                WorktreeStepStatus::Pending
            },
            detail: None,
        })
        .collect()
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn async_worktree_params_remain_compatible_without_display_name() {
        let create: CreateParams = parse_params(serde_json::json!({
            "operation_id": "create-1",
            "project_id": 7,
            "branch": "feature/legacy-client"
        }))
        .unwrap();
        assert!(create.display_name.is_none());

        let fork: ForkParams = parse_params(serde_json::json!({
            "operation_id": "fork-1",
            "project_id": 7,
            "branch": "feature/legacy-fork"
        }))
        .unwrap();
        assert!(fork.display_name.is_none());

        let adopt: AdoptParams = parse_params(serde_json::json!({
            "operation_id": "adopt-1",
            "path": "/tmp/legacy-worktree"
        }))
        .unwrap();
        assert!(adopt.display_name.is_none());
    }

    #[test]
    fn hub_tracks_steps_and_failure_on_the_active_step() {
        let invalidations = StatusInvalidationHub::default();
        let hub = WorktreeOperationHub::new(invalidations.clone());
        hub.begin(new_operation(
            "fixture-op".into(),
            WorktreeOperationMode::Create,
            Some(1),
            Some(2),
            Some("feature/fixture".into()),
            None,
        ))
        .unwrap();
        let reporter = WorktreeOperationReporter {
            hub: hub.clone(),
            operation_id: "fixture-op".into(),
        };
        reporter.completed(WorktreeStepId::Branch, Some("branch ready".into()));
        reporter.running(WorktreeStepId::Worktree, None);
        reporter.fail("git worktree add failed".into());

        let operation = hub.snapshot().pop().unwrap();
        assert_eq!(operation.status, WorktreeOperationStatus::Failed);
        assert_eq!(operation.steps[0].status, WorktreeStepStatus::Completed);
        assert_eq!(operation.steps[1].status, WorktreeStepStatus::Failed);
        assert_eq!(operation.error.as_deref(), Some("git worktree add failed"));
        assert_eq!(invalidations.version_at(0), 4);
    }

    #[test]
    fn rejects_unsafe_operation_ids() {
        assert!(validate_operation_id("fixture_123-ok").is_ok());
        assert!(validate_operation_id("../../escape").is_err());
        assert!(validate_operation_id("").is_err());
    }
}
