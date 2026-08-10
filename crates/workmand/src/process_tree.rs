//! Race-aware operating-system process-tree tracking for managed PTY shutdown.

use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use sysinfo::{
    Pid, ProcessRefreshKind, ProcessStatus as SystemProcessStatus, ProcessesToUpdate, Signal,
    System,
};

const SAMPLE_INTERVAL: Duration = Duration::from_millis(5);
const KILL_RETRY_INTERVAL: Duration = Duration::from_millis(10);
const KILL_DEADLINE: Duration = Duration::from_millis(500);

/// Tracks the identity of a PTY root and every descendant observed while it shuts down.
///
/// Process groups cover the normal case cheaply. This tracker closes the gap for shell jobs and
/// subprocesses that create their own group/session. Retaining `(pid, start_time)` identities also
/// lets cleanup follow a child after its parent exits without signaling a recycled PID.
pub(crate) struct TrackedProcessTree {
    tracked: Arc<Mutex<HashMap<u32, u64>>>,
    stop: Arc<AtomicBool>,
    sampler: Option<JoinHandle<()>>,
}

impl TrackedProcessTree {
    pub(crate) fn capture(root_pid: u32) -> Self {
        let tracked = Arc::new(Mutex::new(HashMap::new()));
        let mut system = System::new();
        refresh_processes(&mut system);
        if let Some(root) = system.process(Pid::from_u32(root_pid)) {
            tracked
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .insert(root_pid, root.start_time());
            extend_tracked(&system, &tracked);
        }

        let stop = Arc::new(AtomicBool::new(false));
        let sampler_tracked = Arc::clone(&tracked);
        let sampler_stop = Arc::clone(&stop);
        let sampler = thread::Builder::new()
            .name(format!("workman-process-tree-{root_pid}"))
            .spawn(move || {
                let mut system = System::new();
                while !sampler_stop.load(Ordering::Acquire) {
                    refresh_processes(&mut system);
                    extend_tracked(&system, &sampler_tracked);
                    thread::sleep(SAMPLE_INTERVAL);
                }
            })
            .ok();

        Self {
            tracked,
            stop,
            sampler,
        }
    }

    /// Stop sampling and force-kill every still-live tracked process tree.
    pub(crate) fn kill_remaining(mut self) -> Result<(), String> {
        self.stop_sampler()?;

        let deadline = Instant::now() + KILL_DEADLINE;
        let mut system = System::new();
        loop {
            refresh_processes(&mut system);
            extend_tracked(&system, &self.tracked);
            let victims = live_tracked_processes(&system, &self.tracked);
            if victims.is_empty() {
                return Ok(());
            }

            for pid in victims {
                if pid == std::process::id() {
                    continue;
                }
                if let Some(process) = system.process(Pid::from_u32(pid)) {
                    let _ = process.kill_with(Signal::Kill);
                }
            }

            if Instant::now() >= deadline {
                refresh_processes(&mut system);
                extend_tracked(&system, &self.tracked);
                let survivors = live_tracked_processes(&system, &self.tracked);
                if survivors.is_empty() {
                    return Ok(());
                }
                return Err(format!(
                    "OS descendants still alive after SIGKILL: {}",
                    survivors
                        .iter()
                        .map(u32::to_string)
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            thread::sleep(KILL_RETRY_INTERVAL);
        }
    }

    fn stop_sampler(&mut self) -> Result<(), String> {
        self.stop.store(true, Ordering::Release);
        if let Some(sampler) = self.sampler.take() {
            sampler
                .join()
                .map_err(|_| "OS process-tree sampler panicked".to_owned())?;
        }
        Ok(())
    }
}

impl Drop for TrackedProcessTree {
    fn drop(&mut self) {
        let _ = self.stop_sampler();
    }
}

fn refresh_processes(system: &mut System) {
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().without_tasks(),
    );
}

fn extend_tracked(system: &System, tracked: &Arc<Mutex<HashMap<u32, u64>>>) {
    let mut tracked = tracked
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let live_roots = tracked
        .iter()
        .filter_map(|(pid, start_time)| {
            system
                .process(Pid::from_u32(*pid))
                .filter(|process| {
                    process.start_time() == *start_time
                        && !matches!(
                            process.status(),
                            SystemProcessStatus::Dead | SystemProcessStatus::Zombie
                        )
                })
                .map(|_| *pid)
        })
        .collect::<HashSet<_>>();
    if live_roots.is_empty() {
        return;
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
    let mut tree = live_roots;
    loop {
        let before = tree.len();
        for (pid, parent) in &parents {
            if tree.contains(parent) {
                tree.insert(*pid);
            }
        }
        if tree.len() == before {
            break;
        }
    }

    for pid in tree {
        if let Some(process) = system.process(Pid::from_u32(pid)) {
            tracked.entry(pid).or_insert_with(|| process.start_time());
        }
    }
}

fn live_tracked_processes(system: &System, tracked: &Arc<Mutex<HashMap<u32, u64>>>) -> Vec<u32> {
    let tracked = tracked
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut processes = tracked
        .iter()
        .filter_map(|(pid, start_time)| {
            system
                .process(Pid::from_u32(*pid))
                .filter(|process| {
                    process.start_time() == *start_time
                        && !matches!(
                            process.status(),
                            SystemProcessStatus::Dead | SystemProcessStatus::Zombie
                        )
                })
                .map(|_| *pid)
        })
        .collect::<Vec<_>>();
    processes.sort_unstable_by(|left, right| right.cmp(left));
    processes
}
