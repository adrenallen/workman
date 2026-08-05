//! Live process telemetry and project-level coordination counts.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use gbuild_core::{Process, ProcessId, ProcessKind, ProcessStatus, ProjectId, Store, StoreError};
use serde::{Deserialize, Serialize};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
use tokio::{
    sync::{RwLock, watch},
    task::{JoinHandle, spawn_blocking},
    time::sleep,
};

use crate::{RegistryError, RegistryResult, SharedProcessRegistry};

/// Process telemetry is intentionally coarser than the terminal stream.
pub const LIVE_STATS_SAMPLE_INTERVAL: Duration = Duration::from_secs(2);

/// One operating-system process below a gbuild-managed PTY root.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct DescendantProcessStats {
    pub pid: u32,
    pub parent_pid: Option<u32>,
    pub name: String,
    pub command: Option<String>,
    pub cpu_percent: f32,
    pub memory_bytes: u64,
}

/// Aggregated telemetry for one gbuild process and its subprocess tree.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ProcessRuntimeStats {
    pub process_id: ProcessId,
    pub pid: Option<u32>,
    pub cpu_percent: f32,
    pub memory_bytes: u64,
    pub uptime_seconds: u64,
    pub descendant_count: usize,
    pub descendants: Vec<DescendantProcessStats>,
}

/// Resource rollup for all process trees attributed to one project.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProjectRuntimeStats {
    pub project_id: ProjectId,
    pub memory_bytes: u64,
}

/// Sidebar counts for one project.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProjectCounts {
    pub todo_open: u64,
    pub scratchpad_total: u64,
    pub agent_running: u64,
    pub agent_total: u64,
    pub terminal_running: u64,
    pub terminal_total: u64,
    pub command_running: u64,
    pub command_total: u64,
}

/// One coherent sample shared by every status-stream consumer.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct LiveStatsSnapshot {
    /// Unix epoch milliseconds. Zero means no client-triggered sample has completed yet.
    pub sampled_at: u64,
    pub processes: BTreeMap<ProcessId, ProcessRuntimeStats>,
    pub projects: BTreeMap<ProjectId, ProjectRuntimeStats>,
    pub counts: BTreeMap<ProjectId, ProjectCounts>,
}

/// Shared latest-value cache and connected-client activation signal.
#[derive(Clone)]
pub(crate) struct LiveStatsHub {
    latest: Arc<RwLock<LiveStatsSnapshot>>,
    client_count: Arc<AtomicUsize>,
    client_count_tx: watch::Sender<usize>,
}

impl LiveStatsHub {
    pub(crate) fn new() -> Self {
        let (client_count_tx, _) = watch::channel(0);
        Self {
            latest: Arc::new(RwLock::new(LiveStatsSnapshot::default())),
            client_count: Arc::new(AtomicUsize::new(0)),
            client_count_tx,
        }
    }

    pub(crate) fn client_connected(&self) -> LiveStatsClientGuard {
        let count = self.client_count.fetch_add(1, Ordering::AcqRel) + 1;
        self.client_count_tx.send_replace(count);
        LiveStatsClientGuard { hub: self.clone() }
    }

    pub(crate) async fn snapshot(&self) -> LiveStatsSnapshot {
        self.latest.read().await.clone()
    }

    async fn publish(&self, snapshot: LiveStatsSnapshot) {
        *self.latest.write().await = snapshot;
    }

    fn client_count_receiver(&self) -> watch::Receiver<usize> {
        self.client_count_tx.subscribe()
    }
}

pub(crate) struct LiveStatsClientGuard {
    hub: LiveStatsHub,
}

impl Drop for LiveStatsClientGuard {
    fn drop(&mut self) {
        let previous = self.hub.client_count.fetch_sub(1, Ordering::AcqRel);
        debug_assert!(previous > 0, "live stats client count underflow");
        self.hub
            .client_count_tx
            .send_replace(previous.saturating_sub(1));
    }
}

/// Run one sampler for the daemon, sleeping completely while no WS client is connected.
pub(crate) fn spawn_live_stats_sampler(
    hub: LiveStatsHub,
    registry: SharedProcessRegistry,
    mut shutdown: watch::Receiver<bool>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut client_count = hub.client_count_receiver();
        let mut system = System::new();

        loop {
            if *shutdown.borrow() {
                return;
            }
            if *client_count.borrow() == 0 {
                tokio::select! {
                    changed = shutdown.changed() => {
                        if changed.is_err() || *shutdown.borrow() {
                            return;
                        }
                    }
                    changed = client_count.changed() => {
                        if changed.is_err() {
                            return;
                        }
                    }
                }
                continue;
            }

            if let Ok((processes, counts)) = sample_inputs(&registry).await {
                match spawn_blocking(move || {
                    refresh_system(&mut system);
                    let snapshot = snapshot_from_system(&system, processes, counts);
                    (system, snapshot)
                })
                .await
                {
                    Ok((next_system, snapshot)) => {
                        system = next_system;
                        hub.publish(snapshot).await;
                    }
                    Err(_) => system = System::new(),
                }
            }

            tokio::select! {
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        return;
                    }
                }
                changed = client_count.changed() => {
                    if changed.is_err() {
                        return;
                    }
                }
                _ = sleep(LIVE_STATS_SAMPLE_INTERVAL) => {}
            }
        }
    })
}

/// Inspect descendants using a caller-owned process snapshot.
///
/// The root is deliberately excluded. This lets callers validate a destructive child action
/// against the same `System` instance they use to signal the child.
pub fn inspect_process_tree_in(system: &System, root_pid: u32) -> Vec<DescendantProcessStats> {
    if system.process(Pid::from_u32(root_pid)).is_none() {
        return Vec::new();
    }

    let parents = system
        .processes()
        .values()
        .filter_map(|process| {
            process
                .parent()
                .map(|parent| (process.pid().as_u32(), parent.as_u32()))
        })
        .collect::<HashMap<_, _>>();
    let tree = descendant_pids(root_pid, &parents);
    let mut descendants = system
        .processes()
        .values()
        .filter(|process| {
            let pid = process.pid().as_u32();
            pid != root_pid && tree.contains(&pid)
        })
        .map(descendant_stats)
        .collect::<Vec<_>>();
    descendants.sort_by_key(|process| process.pid);
    descendants
}

/// Refresh a process snapshot and inspect all descendants below `root_pid`.
pub fn inspect_process_tree(root_pid: u32) -> Vec<DescendantProcessStats> {
    let mut system = System::new();
    refresh_system(&mut system);
    inspect_process_tree_in(&system, root_pid)
}

fn refresh_system(system: &mut System) {
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing()
            .with_memory()
            .with_cpu()
            .with_cmd(UpdateKind::OnlyIfNotSet)
            .without_tasks(),
    );
}

async fn sample_inputs(
    registry: &SharedProcessRegistry,
) -> RegistryResult<(Vec<Process>, BTreeMap<ProjectId, ProjectCounts>)> {
    let mut registry = registry.lock().await;
    let processes = registry.list(None)?;
    let counts = collect_project_counts(registry.store(), &processes)?;
    Ok((processes, counts))
}

fn collect_project_counts(
    store: &Store,
    processes: &[Process],
) -> RegistryResult<BTreeMap<ProjectId, ProjectCounts>> {
    let mut counts = store
        .list_projects()?
        .into_iter()
        .map(|project| (project.id, ProjectCounts::default()))
        .collect::<BTreeMap<_, _>>();

    for process in processes {
        let project = counts.entry(process.project_id).or_default();
        let running = matches!(
            process.status,
            ProcessStatus::Starting | ProcessStatus::Running
        );
        match process.kind {
            ProcessKind::Agent => {
                project.agent_total += 1;
                project.agent_running += u64::from(running);
            }
            ProcessKind::Terminal => {
                project.terminal_total += 1;
                project.terminal_running += u64::from(running);
            }
            ProcessKind::Command => {
                project.command_total += 1;
                project.command_running += u64::from(running);
            }
        }
    }

    for (project_id, project) in &mut counts {
        project.todo_open = sql_count(
            store,
            "SELECT COUNT(*) FROM todos WHERE project_id = ?1 AND completed = 0",
            *project_id,
        )?;
        project.scratchpad_total = sql_count(
            store,
            "SELECT COUNT(*) FROM scratchpads WHERE project_id = ?1 AND archived = 0",
            *project_id,
        )?;
    }
    Ok(counts)
}

fn sql_count(store: &Store, sql: &str, project_id: ProjectId) -> RegistryResult<u64> {
    let count: i64 = store
        .connection()
        .query_row(sql, [project_id], |row| row.get(0))
        .map_err(StoreError::from)
        .map_err(RegistryError::from)?;
    Ok(count.max(0) as u64)
}

fn snapshot_from_system(
    system: &System,
    processes: Vec<Process>,
    counts: BTreeMap<ProjectId, ProjectCounts>,
) -> LiveStatsSnapshot {
    let mut runtime_processes = BTreeMap::new();
    let mut project_pids = BTreeMap::<ProjectId, HashSet<u32>>::new();

    for process in processes {
        let descendants = process
            .pid
            .map(|pid| inspect_process_tree_in(system, pid))
            .unwrap_or_default();
        let root = process
            .pid
            .and_then(|pid| system.process(Pid::from_u32(pid)));
        let memory_bytes = root.map_or(0, |root| root.memory())
            + descendants
                .iter()
                .map(|descendant| descendant.memory_bytes)
                .sum::<u64>();
        let cpu_percent = root.map_or(0.0, |root| root.cpu_usage())
            + descendants
                .iter()
                .map(|descendant| descendant.cpu_percent)
                .sum::<f32>();
        let uptime_seconds = root.map_or(0, |root| root.run_time());

        if root.is_some() {
            let pids = project_pids.entry(process.project_id).or_default();
            if let Some(pid) = process.pid {
                pids.insert(pid);
            }
            pids.extend(descendants.iter().map(|descendant| descendant.pid));
        }

        runtime_processes.insert(
            process.id,
            ProcessRuntimeStats {
                process_id: process.id,
                pid: process.pid.filter(|_| root.is_some()),
                cpu_percent,
                memory_bytes,
                uptime_seconds,
                descendant_count: descendants.len(),
                descendants,
            },
        );
    }

    let mut projects = counts
        .keys()
        .map(|project_id| {
            (
                *project_id,
                ProjectRuntimeStats {
                    project_id: *project_id,
                    memory_bytes: 0,
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    for (project_id, pids) in project_pids {
        let memory_bytes = pids
            .into_iter()
            .filter_map(|pid| system.process(Pid::from_u32(pid)))
            .map(|process| process.memory())
            .sum();
        projects.insert(
            project_id,
            ProjectRuntimeStats {
                project_id,
                memory_bytes,
            },
        );
    }

    LiveStatsSnapshot {
        sampled_at: now_millis(),
        processes: runtime_processes,
        projects,
        counts,
    }
}

fn descendant_stats(process: &sysinfo::Process) -> DescendantProcessStats {
    let command = process
        .cmd()
        .iter()
        .map(|part| part.to_string_lossy())
        .collect::<Vec<_>>()
        .join(" ");
    DescendantProcessStats {
        pid: process.pid().as_u32(),
        parent_pid: process.parent().map(Pid::as_u32),
        name: process.name().to_string_lossy().into_owned(),
        command: (!command.is_empty()).then_some(command),
        cpu_percent: process.cpu_usage(),
        memory_bytes: process.memory(),
    }
}

fn descendant_pids(root: u32, parents: &HashMap<u32, u32>) -> HashSet<u32> {
    let mut descendants = HashSet::from([root]);
    loop {
        let before = descendants.len();
        for (pid, parent) in parents {
            if descendants.contains(parent) {
                descendants.insert(*pid);
            }
        }
        if descendants.len() == before {
            return descendants;
        }
    }
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
    use std::collections::BTreeMap;

    use gbuild_core::{NewTodo, Project, ScratchpadService, TodoPriority, TodoService};

    use super::*;

    #[test]
    fn counts_include_open_todos_visible_scratchpads_and_process_kinds() {
        let store = Store::open_in_memory().unwrap();
        store
            .put_project(&Project {
                id: 7,
                path: "/tmp/live-stats".into(),
                name: "live-stats".into(),
                display_name: None,
                icon: None,
                selected: true,
                sort_order: 0,
            })
            .unwrap();
        let todo = TodoService::new(&store)
            .create(
                7,
                NewTodo {
                    title: "Live count".into(),
                    body: String::new(),
                    priority: TodoPriority::Medium,
                    tags: Vec::new(),
                },
                1,
            )
            .unwrap();
        TodoService::new(&store)
            .complete(7, todo.id, "test", true, false, 2)
            .unwrap();
        TodoService::new(&store)
            .create(
                7,
                NewTodo {
                    title: "Still open".into(),
                    body: String::new(),
                    priority: TodoPriority::Medium,
                    tags: Vec::new(),
                },
                3,
            )
            .unwrap();
        ScratchpadService::new(&store)
            .write(7, None, "Notes".into(), "body".into(), None, None)
            .unwrap();

        let processes = vec![
            process(1, ProcessKind::Agent, ProcessStatus::Running),
            process(2, ProcessKind::Command, ProcessStatus::Stopped),
        ];
        let counts = collect_project_counts(&store, &processes).unwrap();
        assert_eq!(
            counts[&7],
            ProjectCounts {
                todo_open: 1,
                scratchpad_total: 1,
                agent_running: 1,
                agent_total: 1,
                command_total: 1,
                ..ProjectCounts::default()
            }
        );
    }

    fn process(id: ProcessId, kind: ProcessKind, status: ProcessStatus) -> Process {
        Process {
            id,
            project_id: 7,
            kind,
            name: format!("process-{id}"),
            command: Some("sleep 60".into()),
            working_dir: "/tmp".into(),
            env: BTreeMap::new(),
            auto_start: false,
            auto_restart: false,
            restart_when_changed: Vec::new(),
            source: gbuild_core::ProcessSource::Local,
            trust_hash: None,
            status,
            pid: None,
            exit_code: None,
            exit_signal: None,
            exited_at: None,
            agent_tool_id: None,
            spawned_by_process_id: None,
            sort_order: 0,
        }
    }
}
