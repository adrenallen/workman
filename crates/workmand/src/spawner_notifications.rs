//! Coalesced child-to-spawner prompt delivery.
//!
//! Delivery intentionally uses `ProcessRegistry::submit_input`, the same route as timers, so
//! bracketed paste, Enter separation, dialog guards, typing pauses, and process-local FIFO order
//! stay centralized. Completed-turn discovery and reporting are supplied by the completion ledger.

use std::{
    collections::{BTreeMap, HashSet},
    time::Duration,
};

use tokio::{
    sync::watch,
    task::JoinHandle,
    time::{MissedTickBehavior, interval},
};
use workman_core::{
    Process, ProcessId, ProcessStatus, SpawnerReportedState, attention::AttentionState,
};

use crate::{ProcessRegistry, RegistryError, RegistryResult, SharedProcessRegistry};

const NOTIFICATION_POLL_INTERVAL: Duration = Duration::from_millis(25);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ChildNotificationReason {
    Finished,
    NeedsInput,
    Exited,
    Crashed,
}

impl ChildNotificationReason {
    const fn label(self) -> &'static str {
        match self {
            Self::Finished => "finished",
            Self::NeedsInput => "needs input",
            Self::Exited => "exited",
            Self::Crashed => "crashed",
        }
    }

    const fn reported_state(self) -> Option<SpawnerReportedState> {
        match self {
            Self::Finished => None,
            Self::NeedsInput => Some(SpawnerReportedState::NeedsInput),
            Self::Exited => Some(SpawnerReportedState::Exited),
            Self::Crashed => Some(SpawnerReportedState::Crashed),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PendingChildNotification {
    child_process_id: ProcessId,
    child_name: String,
    child_project_id: i64,
    reason: ChildNotificationReason,
}

#[derive(Default)]
pub(crate) struct SpawnerNotificationService {
    pending: BTreeMap<ProcessId, BTreeMap<ProcessId, PendingChildNotification>>,
}

impl SpawnerNotificationService {
    pub(crate) fn tick(&mut self, registry: &mut ProcessRegistry) -> RegistryResult<()> {
        let settings = registry.store().list_spawner_idle_notification_settings()?;
        let enabled = settings
            .iter()
            .map(|setting| setting.process_id)
            .collect::<HashSet<_>>();
        self.pending.retain(|_, children| {
            children.retain(|child_id, _| enabled.contains(child_id));
            !children.is_empty()
        });

        for setting in settings {
            let child_status = match registry.get_status(setting.process_id) {
                Ok(status) => status,
                Err(RegistryError::NotFound(_)) => continue,
                Err(error) => return Err(error),
            };
            let child = child_status.process;
            let Some(spawner_id) = child.spawned_by_process_id else {
                eprintln!(
                    "process {}: dropping notify_spawner_on_idle because its spawner no longer exists",
                    child.id
                );
                registry
                    .store()
                    .set_spawner_idle_notification(child.id, false, 0)?;
                continue;
            };
            let Some(spawner) = registry.store().get_process(spawner_id)? else {
                eprintln!(
                    "process {}: dropping notify_spawner_on_idle because spawner {spawner_id} no longer exists",
                    child.id
                );
                registry
                    .store()
                    .set_spawner_idle_notification(child.id, false, 0)?;
                continue;
            };
            if spawner.project_id != child.project_id {
                eprintln!(
                    "process {}: dropping cross-project notify_spawner_on_idle targeting spawner {spawner_id}",
                    child.id
                );
                registry
                    .store()
                    .set_spawner_idle_notification(child.id, false, 0)?;
                continue;
            }

            let reason = child_reason(child.status, child_status.agent_state.state);
            match reason {
                Some(reason) if reason.reported_state() != Some(setting.last_reported_state) => {
                    self.queue(spawner_id, &child, reason);
                }
                Some(_) => {}
                None => {
                    if setting.last_reported_state != SpawnerReportedState::Neutral {
                        registry
                            .store()
                            .set_spawner_reported_state(child.id, SpawnerReportedState::Neutral)?;
                    }
                    self.remove_pending_reason(child.id, ChildNotificationReason::NeedsInput);
                }
            }
        }

        self.deliver_ready(registry)
    }

    /// Queue a completed child once the completion ledger identifies a real, unreported turn.
    /// The ledger integration also carries completion IDs so successful delivery can mark them.
    pub(crate) fn queue_finished(&mut self, spawner_id: ProcessId, child: &Process) {
        self.queue(spawner_id, child, ChildNotificationReason::Finished);
    }

    fn queue(&mut self, spawner_id: ProcessId, child: &Process, reason: ChildNotificationReason) {
        self.pending.entry(spawner_id).or_default().insert(
            child.id,
            PendingChildNotification {
                child_process_id: child.id,
                child_name: single_line_name(&child.name),
                child_project_id: child.project_id,
                reason,
            },
        );
    }

    fn remove_pending_reason(
        &mut self,
        child_process_id: ProcessId,
        reason: ChildNotificationReason,
    ) {
        self.pending.retain(|_, children| {
            if children
                .get(&child_process_id)
                .is_some_and(|pending| pending.reason == reason)
            {
                children.remove(&child_process_id);
            }
            !children.is_empty()
        });
    }

    fn deliver_ready(&mut self, registry: &mut ProcessRegistry) -> RegistryResult<()> {
        let spawner_ids = self.pending.keys().copied().collect::<Vec<_>>();
        for spawner_id in spawner_ids {
            let spawner = match registry.get(spawner_id) {
                Ok(process) => process,
                Err(RegistryError::NotFound(_)) => {
                    self.drop_for_exited_spawner(registry, spawner_id, "was deleted")?;
                    continue;
                }
                Err(error) => return Err(error),
            };
            if matches!(
                spawner.status,
                ProcessStatus::Stopped | ProcessStatus::Exited | ProcessStatus::Crashed
            ) {
                self.drop_for_exited_spawner(registry, spawner_id, "is not running")?;
                continue;
            }
            if spawner.status != ProcessStatus::Running {
                continue;
            }
            let status = registry.get_status(spawner_id)?;
            if registry.has_pending_prompts(spawner_id)
                || !matches!(
                    status.agent_state.state,
                    AttentionState::Idle | AttentionState::Waiting
                )
            {
                continue;
            }
            let Some(children) = self.pending.get(&spawner_id) else {
                continue;
            };
            if children
                .values()
                .any(|pending| pending.child_project_id != spawner.project_id)
            {
                eprintln!(
                    "process {spawner_id}: dropped an invalid cross-project child notification"
                );
                self.pending.remove(&spawner_id);
                continue;
            }
            let body = notification_body(children.values());
            if registry.submit_input(spawner_id, body.as_bytes()).is_err() {
                // A lifecycle edge may race the readiness check. The next tick either retries a
                // live parent or drops the batch once its exit is visible.
                continue;
            }
            let delivered = self.pending.remove(&spawner_id).unwrap_or_default();
            for pending in delivered.values() {
                if let Some(state) = pending.reason.reported_state() {
                    registry
                        .store()
                        .set_spawner_reported_state(pending.child_process_id, state)?;
                }
            }
        }
        Ok(())
    }

    fn drop_for_exited_spawner(
        &mut self,
        registry: &ProcessRegistry,
        spawner_id: ProcessId,
        detail: &str,
    ) -> RegistryResult<()> {
        let Some(dropped) = self.pending.remove(&spawner_id) else {
            return Ok(());
        };
        for pending in dropped.values() {
            if let Some(state) = pending.reason.reported_state() {
                registry
                    .store()
                    .set_spawner_reported_state(pending.child_process_id, state)?;
            }
        }
        eprintln!(
            "process {spawner_id}: dropped {} coalesced child notification(s) because the spawner {detail}",
            dropped.len()
        );
        Ok(())
    }
}

fn child_reason(
    process_status: ProcessStatus,
    attention_state: AttentionState,
) -> Option<ChildNotificationReason> {
    match process_status {
        ProcessStatus::Exited => Some(ChildNotificationReason::Exited),
        ProcessStatus::Crashed => Some(ChildNotificationReason::Crashed),
        ProcessStatus::Starting | ProcessStatus::Running
            if attention_state == AttentionState::NeedsInput =>
        {
            Some(ChildNotificationReason::NeedsInput)
        }
        _ => None,
    }
}

fn notification_body<'a>(
    children: impl IntoIterator<Item = &'a PendingChildNotification>,
) -> String {
    let entries = children
        .into_iter()
        .map(|pending| {
            format!(
                "{} ({}) {}",
                pending.child_name,
                pending.child_process_id,
                pending.reason.label()
            )
        })
        .collect::<Vec<_>>()
        .join("; ");
    format!("[workman] child idle: {entries} — check their output/todos")
}

fn single_line_name(name: &str) -> String {
    name.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(crate) fn spawn_spawner_notification_scheduler(
    registry: SharedProcessRegistry,
    mut shutdown: watch::Receiver<bool>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut service = SpawnerNotificationService::default();
        let mut ticker = interval(NOTIFICATION_POLL_INTERVAL);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        break;
                    }
                }
                _ = ticker.tick() => {
                    let mut registry = registry.lock().await;
                    if let Err(error) = service.tick(&mut registry) {
                        eprintln!("child-to-spawner notification tick failed: {error}");
                    }
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, thread, time::Instant};

    use super::*;
    use workman_core::{AgentTool, AgentToolSource, ProcessKind, ProcessSource, Project, Store};

    fn child(id: ProcessId, name: &str) -> Process {
        Process {
            id,
            project_id: 1,
            kind: workman_core::ProcessKind::Agent,
            name: name.into(),
            command: None,
            working_dir: "/tmp".into(),
            env: Default::default(),
            auto_start: false,
            auto_restart: false,
            restart_when_changed: Vec::new(),
            source: workman_core::ProcessSource::Local,
            trust_hash: None,
            status: ProcessStatus::Running,
            pid: None,
            exit_code: None,
            exit_signal: None,
            exited_at: None,
            agent_tool_id: None,
            spawned_by_process_id: Some(1),
            sort_order: 0,
        }
    }

    #[cfg(unix)]
    fn process(id: ProcessId, name: &str, command: &str) -> Process {
        Process {
            id,
            project_id: 1,
            kind: ProcessKind::Agent,
            name: name.into(),
            command: Some(command.into()),
            working_dir: "/tmp".into(),
            env: BTreeMap::new(),
            auto_start: false,
            auto_restart: false,
            restart_when_changed: Vec::new(),
            source: ProcessSource::Local,
            trust_hash: None,
            status: ProcessStatus::Stopped,
            pid: None,
            exit_code: None,
            exit_signal: None,
            exited_at: None,
            agent_tool_id: Some(90),
            spawned_by_process_id: (id != 1).then_some(1),
            sort_order: 0,
        }
    }

    #[cfg(unix)]
    fn registry() -> ProcessRegistry {
        let store = Store::open_in_memory().unwrap();
        store
            .put_project(&Project {
                id: 1,
                path: "/tmp".into(),
                name: "spawner notifications".into(),
                display_name: None,
                icon: None,
                selected: true,
                sort_order: 0,
            })
            .unwrap();
        store
            .put_agent_tool(&AgentTool {
                id: 90,
                name: "Scripted Claude".into(),
                command: "scripted-claude".into(),
                tool_type: "claude_code".into(),
                enabled: true,
                source: AgentToolSource::Local,
                resume_args: None,
                continue_args: None,
            })
            .unwrap();
        ProcessRegistry::with_stop_grace_for_test(store, Duration::from_millis(50)).unwrap()
    }

    #[cfg(unix)]
    fn wait_for_state(
        registry: &mut ProcessRegistry,
        process_id: ProcessId,
        expected: AttentionState,
    ) {
        let deadline = Instant::now() + Duration::from_secs(7);
        loop {
            let state = registry.get_status(process_id).unwrap().agent_state.state;
            if state == expected {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "process {process_id} did not reach {expected:?}; current state is {state:?}"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[cfg(unix)]
    fn wait_for_output(registry: &mut ProcessRegistry, process_id: ProcessId, needle: &str) {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let output = registry.rendered_output(process_id).unwrap().text;
            if output.contains(needle) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "process {process_id} output did not contain {needle:?}: {output:?}"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn body_is_one_line_stable_and_repeated_child_completions_coalesce() {
        let mut service = SpawnerNotificationService::default();
        service.queue_finished(1, &child(3, "second\nchild"));
        service.queue_finished(1, &child(2, "first child"));
        service.queue_finished(1, &child(2, "first child"));
        let children = service.pending.get(&1).unwrap();
        assert_eq!(children.len(), 2);
        assert_eq!(
            notification_body(children.values()),
            "[workman] child idle: first child (2) finished; second child (3) finished — check their output/todos"
        );
    }

    #[cfg(unix)]
    #[test]
    fn needs_input_exit_and_crash_coalesce_through_the_timer_submission_path() {
        let mut registry = registry();
        registry
            .create(process(
                1,
                "parent",
                "printf '❯\\n'; while IFS= read -r line; do printf 'received:[%s]\\n' \"$line\"; done",
            ))
            .unwrap();
        registry
            .create(process(
                2,
                "needs-input",
                "printf 'Do you want to proceed?\\n❯ 1. Yes, allow\\n  2. No, and tell Claude\\n'; sleep 30",
            ))
            .unwrap();
        registry.create(process(3, "clean-exit", "exit 0")).unwrap();
        registry.create(process(4, "crash", "exit 7")).unwrap();
        registry
            .create(process(
                5,
                "not-opted-in",
                "printf 'Do you want to proceed?\\n❯ 1. Yes, allow\\n  2. No, and tell Claude\\n'; sleep 30",
            ))
            .unwrap();
        for process_id in 1..=5 {
            registry.start(process_id).unwrap();
        }
        for process_id in 2..=4 {
            registry
                .set_notify_spawner_on_idle(1, process_id, true)
                .unwrap();
        }
        wait_for_state(&mut registry, 1, AttentionState::Idle);
        wait_for_state(&mut registry, 2, AttentionState::NeedsInput);

        let mut service = SpawnerNotificationService::default();
        service.tick(&mut registry).unwrap();
        wait_for_output(&mut registry, 1, "[workman] child idle:");
        let output = String::from_utf8_lossy(
            &registry.raw_output(1, None, 64 * 1024).unwrap().data,
        )
        .into_owned();
        assert_eq!(output.matches("[workman] child idle:").count(), 1);
        assert!(output.contains("needs-input (2) needs input"));
        assert!(output.contains("clean-exit (3) exited"));
        assert!(output.contains("crash (4) crashed"), "{output:?}");
        assert!(!output.contains("not-opted-in"));

        registry.stop(1).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn exited_spawner_drops_attention_delivery_once_without_retrying() {
        let mut registry = registry();
        registry.create(process(1, "parent", "exit 0")).unwrap();
        registry
            .create(process(
                2,
                "needs-input",
                "printf 'Do you want to proceed?\\n❯ 1. Yes, allow\\n  2. No, and tell Claude\\n'; sleep 30",
            ))
            .unwrap();
        registry.start(1).unwrap();
        registry.start(2).unwrap();
        registry.set_notify_spawner_on_idle(1, 2, true).unwrap();
        wait_for_state(&mut registry, 2, AttentionState::NeedsInput);

        let mut service = SpawnerNotificationService::default();
        service.tick(&mut registry).unwrap();
        assert!(service.pending.is_empty());
        assert_eq!(
            registry
                .store()
                .spawner_idle_notification_setting(2)
                .unwrap()
                .unwrap()
                .last_reported_state,
            SpawnerReportedState::NeedsInput
        );
        service.tick(&mut registry).unwrap();
        assert!(service.pending.is_empty());

        registry.stop(2).unwrap();
    }
}
