//! Daemon-owned process lifecycle automation.

use std::{
    collections::{HashMap, HashSet},
    io,
    path::{Component, Path, PathBuf},
    time::Duration,
};

use awm_core::{ProcessId, ProcessStatus, Project, ProjectId};
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::{
    sync::{mpsc, watch},
    task::JoinHandle,
    time::{Instant, MissedTickBehavior, interval},
};

use crate::{
    AWM_CONFIG_FILE, LEGACY_GBUILD_CONFIG_FILE, ProcessRegistry, RegistryError,
    SharedProcessRegistry, is_process_trusted, project_config_path, sync_awm_yml,
    sync_awm_yml_file,
};

/// Timing policy for lifecycle reconciliation.
#[derive(Clone, Debug)]
pub struct LifecycleOptions {
    pub reconcile_interval: Duration,
    pub change_debounce: Duration,
    pub restart_backoff_initial: Duration,
    pub restart_backoff_max: Duration,
    pub stable_run_reset: Duration,
}

impl Default for LifecycleOptions {
    fn default() -> Self {
        Self {
            reconcile_interval: Duration::from_millis(100),
            change_debounce: Duration::from_millis(200),
            restart_backoff_initial: Duration::from_millis(250),
            restart_backoff_max: Duration::from_secs(30),
            stable_run_reset: Duration::from_secs(10),
        }
    }
}

/// Start eligible commands when a project is opened.
///
/// Individual launch failures do not prevent the other commands from starting. The registry's
/// central trust and working-directory checks remain authoritative for every attempted launch.
pub fn auto_start_project(
    registry: &mut ProcessRegistry,
    project_id: ProjectId,
) -> Result<Vec<ProcessId>, RegistryError> {
    let processes = registry.list(Some(project_id))?;
    let mut started = Vec::new();
    for process in processes {
        if !process.auto_start || is_active(process.status) || !is_process_trusted(&process) {
            continue;
        }
        if registry.start(process.id).is_ok() {
            started.push(process.id);
        }
    }
    Ok(started)
}

/// Spawn the daemon lifecycle supervisor and its filesystem watcher.
pub fn spawn_lifecycle_supervisor(
    registry: SharedProcessRegistry,
    shutdown: watch::Receiver<bool>,
) -> io::Result<JoinHandle<()>> {
    spawn_lifecycle_supervisor_with_options(registry, shutdown, LifecycleOptions::default())
}

/// Spawn a lifecycle supervisor with explicit timings, primarily for deterministic tests.
pub fn spawn_lifecycle_supervisor_with_options(
    registry: SharedProcessRegistry,
    shutdown: watch::Receiver<bool>,
    options: LifecycleOptions,
) -> io::Result<JoinHandle<()>> {
    let (event_tx, event_rx) = mpsc::unbounded_channel();
    let watcher = notify::recommended_watcher(move |event| {
        let _ = event_tx.send(event);
    })
    .map_err(io::Error::other)?;
    let supervisor = LifecycleSupervisor {
        registry,
        shutdown,
        options,
        watcher,
        event_rx,
        watched_projects: HashMap::new(),
        opened_projects: HashSet::new(),
        pending_config_syncs: HashMap::new(),
        pending_changes: HashMap::new(),
        restart_attempts: HashMap::new(),
        restart_due: HashMap::new(),
        running_since: HashMap::new(),
    };
    Ok(tokio::spawn(supervisor.run()))
}

struct LifecycleSupervisor {
    registry: SharedProcessRegistry,
    shutdown: watch::Receiver<bool>,
    options: LifecycleOptions,
    watcher: RecommendedWatcher,
    event_rx: mpsc::UnboundedReceiver<notify::Result<Event>>,
    watched_projects: HashMap<ProjectId, PathBuf>,
    opened_projects: HashSet<ProjectId>,
    pending_config_syncs: HashMap<ProjectId, Instant>,
    pending_changes: HashMap<ProcessId, Instant>,
    restart_attempts: HashMap<ProcessId, u32>,
    restart_due: HashMap<ProcessId, Instant>,
    running_since: HashMap<ProcessId, (Option<u32>, Instant)>,
}

impl LifecycleSupervisor {
    async fn run(mut self) {
        let mut reconcile = interval(self.options.reconcile_interval);
        reconcile.set_missed_tick_behavior(MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                changed = self.shutdown.changed() => {
                    if changed.is_err() || *self.shutdown.borrow() {
                        break;
                    }
                }
                event = self.event_rx.recv() => {
                    if let Some(Ok(event)) = event {
                        self.record_file_event(event).await;
                    }
                }
                _ = reconcile.tick() => {
                    self.reconcile().await;
                }
            }
        }
    }

    async fn reconcile(&mut self) {
        self.reconcile_projects().await;
        self.apply_config_syncs().await;
        self.apply_changed_restarts().await;
        self.reconcile_crashes().await;
    }

    async fn reconcile_projects(&mut self) {
        let projects = {
            let registry = self.registry.lock().await;
            registry.store().list_projects().unwrap_or_default()
        };
        let current_ids = projects
            .iter()
            .map(|project| project.id)
            .collect::<HashSet<_>>();

        let removed = self
            .watched_projects
            .keys()
            .copied()
            .filter(|project_id| !current_ids.contains(project_id))
            .collect::<Vec<_>>();
        for project_id in removed {
            if let Some(root) = self.watched_projects.remove(&project_id) {
                let _ = self.watcher.unwatch(&root);
            }
            self.opened_projects.remove(&project_id);
            self.pending_config_syncs.remove(&project_id);
        }

        for project in projects {
            self.watch_project(&project);
            if self.opened_projects.insert(project.id) {
                let mut registry = self.registry.lock().await;
                let _ = sync_project_config(&mut registry, &project);
                let _ = auto_start_project(&mut registry, project.id);
            }
        }
    }

    fn watch_project(&mut self, project: &Project) {
        let Ok(root) = std::fs::canonicalize(&project.path) else {
            return;
        };
        if self.watched_projects.get(&project.id) == Some(&root) {
            return;
        }
        if let Some(previous) = self.watched_projects.remove(&project.id) {
            let _ = self.watcher.unwatch(&previous);
        }
        if self.watcher.watch(&root, RecursiveMode::Recursive).is_ok() {
            self.watched_projects.insert(project.id, root);
        }
    }

    async fn record_file_event(&mut self, event: Event) {
        if matches!(event.kind, EventKind::Access(_)) {
            return;
        }
        let deadline = Instant::now() + self.options.change_debounce;
        for (project_id, root) in &self.watched_projects {
            if event
                .paths
                .iter()
                .filter_map(|path| relative_event_path(root, path))
                .any(|path| {
                    path == Path::new(AWM_CONFIG_FILE)
                        || path == Path::new(LEGACY_GBUILD_CONFIG_FILE)
                })
            {
                self.pending_config_syncs.insert(*project_id, deadline);
            }
        }

        let processes = {
            let mut registry = self.registry.lock().await;
            registry.list(None).unwrap_or_default()
        };
        for process in processes {
            if !is_active(process.status)
                || !is_process_trusted(&process)
                || process.restart_when_changed.is_empty()
            {
                continue;
            }
            let Some(root) = self.watched_projects.get(&process.project_id) else {
                continue;
            };
            let Some(globs) = compile_globs(&process.restart_when_changed) else {
                continue;
            };
            if event
                .paths
                .iter()
                .filter_map(|path| relative_event_path(root, path))
                .any(|path| globs.is_match(path))
            {
                self.pending_changes.insert(process.id, deadline);
            }
        }
    }

    async fn apply_config_syncs(&mut self) {
        let now = Instant::now();
        let due = self
            .pending_config_syncs
            .iter()
            .filter_map(|(project_id, deadline)| (*deadline <= now).then_some(*project_id))
            .collect::<Vec<_>>();
        for project_id in due {
            self.pending_config_syncs.remove(&project_id);
            let mut registry = self.registry.lock().await;
            let Ok(Some(project)) = registry.store().get_project(project_id) else {
                continue;
            };
            let _ = sync_project_config_event(&mut registry, &project);
        }
    }

    async fn apply_changed_restarts(&mut self) {
        let now = Instant::now();
        let due = self
            .pending_changes
            .iter()
            .filter_map(|(process_id, deadline)| (*deadline <= now).then_some(*process_id))
            .collect::<Vec<_>>();
        for process_id in due {
            self.pending_changes.remove(&process_id);
            let mut registry = self.registry.lock().await;
            let Ok(process) = registry.get(process_id) else {
                continue;
            };
            if is_active(process.status)
                && is_process_trusted(&process)
                && !process.restart_when_changed.is_empty()
            {
                let _ = registry.restart(process_id);
            }
        }
    }

    async fn reconcile_crashes(&mut self) {
        let now = Instant::now();
        let processes = {
            let mut registry = self.registry.lock().await;
            registry.list(None).unwrap_or_default()
        };
        let current_ids = processes
            .iter()
            .map(|process| process.id)
            .collect::<HashSet<_>>();
        self.restart_attempts
            .retain(|process_id, _| current_ids.contains(process_id));
        self.restart_due
            .retain(|process_id, _| current_ids.contains(process_id));
        self.running_since
            .retain(|process_id, _| current_ids.contains(process_id));

        for process in processes {
            if !process.auto_restart || !is_process_trusted(&process) {
                self.clear_restart_state(process.id);
                continue;
            }
            match process.status {
                ProcessStatus::Running | ProcessStatus::Starting => {
                    self.restart_due.remove(&process.id);
                    let running = self
                        .running_since
                        .entry(process.id)
                        .or_insert((process.pid, now));
                    if running.0 != process.pid {
                        *running = (process.pid, now);
                    }
                    if now.duration_since(running.1) >= self.options.stable_run_reset {
                        self.restart_attempts.remove(&process.id);
                    }
                }
                ProcessStatus::Crashed => {
                    self.running_since.remove(&process.id);
                    if !self.restart_due.contains_key(&process.id) {
                        let attempt = self.restart_attempts.get(&process.id).copied().unwrap_or(0);
                        self.restart_due.insert(
                            process.id,
                            now + backoff_delay(
                                self.options.restart_backoff_initial,
                                self.options.restart_backoff_max,
                                attempt,
                            ),
                        );
                    }
                }
                ProcessStatus::Stopped | ProcessStatus::Exited => {
                    self.clear_restart_state(process.id);
                }
            }
        }

        let due = self
            .restart_due
            .iter()
            .filter_map(|(process_id, deadline)| (*deadline <= now).then_some(*process_id))
            .collect::<Vec<_>>();
        for process_id in due {
            self.restart_due.remove(&process_id);
            let mut registry = self.registry.lock().await;
            let Ok(process) = registry.get(process_id) else {
                continue;
            };
            if process.status != ProcessStatus::Crashed
                || !process.auto_restart
                || !is_process_trusted(&process)
            {
                continue;
            }
            let attempts = self.restart_attempts.entry(process_id).or_insert(0);
            *attempts = attempts.saturating_add(1);
            if let Ok(process) = registry.start(process_id) {
                if is_active(process.status) {
                    self.running_since
                        .insert(process_id, (process.pid, Instant::now()));
                }
            }
        }
    }

    fn clear_restart_state(&mut self, process_id: ProcessId) {
        self.restart_attempts.remove(&process_id);
        self.restart_due.remove(&process_id);
        self.running_since.remove(&process_id);
    }
}

pub(crate) fn sync_project_config(
    registry: &mut ProcessRegistry,
    project: &Project,
) -> Result<(), crate::ConfigError> {
    if project_config_path(Path::new(&project.path)).is_some() {
        sync_awm_yml_file(registry, project.id)?;
    }
    Ok(())
}

fn sync_project_config_event(
    registry: &mut ProcessRegistry,
    project: &Project,
) -> Result<(), crate::ConfigError> {
    if project_config_path(Path::new(&project.path)).is_some() {
        sync_awm_yml_file(registry, project.id)?;
    } else {
        sync_awm_yml(registry, project.id, "")?;
    }
    Ok(())
}

fn is_active(status: ProcessStatus) -> bool {
    matches!(status, ProcessStatus::Starting | ProcessStatus::Running)
}

fn compile_globs(patterns: &[String]) -> Option<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    let mut valid = 0;
    for pattern in patterns {
        let Ok(glob) = GlobBuilder::new(pattern).literal_separator(true).build() else {
            continue;
        };
        builder.add(glob);
        valid += 1;
    }
    (valid > 0).then(|| builder.build().ok()).flatten()
}

fn relative_event_path(root: &Path, event_path: &Path) -> Option<PathBuf> {
    let relative = if event_path.is_absolute() {
        event_path.strip_prefix(root).ok()?
    } else {
        event_path
    };
    if relative
        .components()
        .any(|component| matches!(component, Component::ParentDir | Component::RootDir))
    {
        return None;
    }
    Some(relative.to_owned())
}

fn backoff_delay(initial: Duration, maximum: Duration, attempt: u32) -> Duration {
    let multiplier = 1_u32.checked_shl(attempt.min(31)).unwrap_or(u32::MAX);
    initial.saturating_mul(multiplier).min(maximum)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_globs_are_ignored_individually() {
        let globs = compile_globs(&["[invalid".into(), "src/**/*.rs".into()]).unwrap();
        assert!(globs.is_match("src/nested/main.rs"));
        assert!(!globs.is_match("README.md"));
        assert!(compile_globs(&["[invalid".into()]).is_none());
    }

    #[test]
    fn crash_backoff_is_exponential_and_capped() {
        let initial = Duration::from_millis(250);
        let maximum = Duration::from_secs(2);
        assert_eq!(backoff_delay(initial, maximum, 0), initial);
        assert_eq!(
            backoff_delay(initial, maximum, 1),
            Duration::from_millis(500)
        );
        assert_eq!(backoff_delay(initial, maximum, 2), Duration::from_secs(1));
        assert_eq!(backoff_delay(initial, maximum, 20), maximum);
    }
}
