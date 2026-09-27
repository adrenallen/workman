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

use crate::{
    ProcessRegistry, RegistryError, RegistryResult, SharedProcessRegistry,
    completion_ledger::{Completion, CompletionLedger},
};

const NOTIFICATION_POLL_INTERVAL: Duration = Duration::from_millis(100);

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
    completion: Option<Completion>,
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
        let mut eligible_children =
            BTreeMap::<ProcessId, BTreeMap<ProcessId, (Process, i64)>>::new();

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
            eligible_children
                .entry(spawner_id)
                .or_default()
                .insert(child.id, (child.clone(), setting.enabled_at));

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

        for (spawner_id, children) in eligible_children {
            let child_ids = children.keys().copied().collect::<Vec<_>>();
            let completions = CompletionLedger::new(registry.store())
                .unreported_completions(spawner_id, &child_ids)?;
            for completion in completions {
                let Some((child, enabled_at)) = children.get(&completion.process_id) else {
                    continue;
                };
                // Enabling the flag is prospective. Older completions stay unreported for timer
                // consumers and become eligible here only after this child completes again.
                if completion.completed_at_ms >= *enabled_at {
                    self.queue_completion(spawner_id, child, completion);
                }
            }
        }

        self.deliver_ready(registry)
    }

    fn queue_completion(&mut self, spawner_id: ProcessId, child: &Process, completion: Completion) {
        self.pending
            .entry(spawner_id)
            .or_default()
            .entry(child.id)
            .and_modify(|pending| {
                if pending
                    .completion
                    .is_none_or(|current| completion.id > current.id)
                {
                    pending.completion = Some(completion);
                }
            })
            .or_insert_with(|| PendingChildNotification {
                child_process_id: child.id,
                child_name: single_line_name(&child.name),
                child_project_id: child.project_id,
                reason: ChildNotificationReason::Finished,
                completion: Some(completion),
            });
    }

    fn queue(&mut self, spawner_id: ProcessId, child: &Process, reason: ChildNotificationReason) {
        self.pending
            .entry(spawner_id)
            .or_default()
            .entry(child.id)
            .and_modify(|pending| {
                pending.child_name = single_line_name(&child.name);
                pending.child_project_id = child.project_id;
                pending.reason = reason;
            })
            .or_insert_with(|| PendingChildNotification {
                child_process_id: child.id,
                child_name: single_line_name(&child.name),
                child_project_id: child.project_id,
                reason,
                completion: None,
            });
    }

    fn remove_pending_reason(
        &mut self,
        child_process_id: ProcessId,
        reason: ChildNotificationReason,
    ) {
        self.pending.retain(|_, children| {
            if let Some(pending) = children.get_mut(&child_process_id)
                && pending.reason == reason
            {
                if pending.completion.is_some() {
                    pending.reason = ChildNotificationReason::Finished;
                } else {
                    children.remove(&child_process_id);
                }
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
            CompletionLedger::new(registry.store()).mark_completions_reported(
                spawner_id,
                delivered.values().filter_map(|pending| pending.completion),
            )?;
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
        CompletionLedger::new(registry.store()).mark_completions_reported(
            spawner_id,
            dropped.values().filter_map(|pending| pending.completion),
        )?;
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

    #[cfg(unix)]
    fn submit_as(
        registry: &mut ProcessRegistry,
        owner_process_id: ProcessId,
        process_id: ProcessId,
        body: &[u8],
    ) {
        registry.submit_input(process_id, body).unwrap();
        let input_at = registry
            .agent_attention_snapshot(process_id)
            .unwrap()
            .last_input_at
            .unwrap();
        CompletionLedger::new(registry.store())
            .record_input(owner_process_id, process_id, input_at)
            .unwrap();
    }

    #[test]
    fn body_is_one_line_stable_and_repeated_child_completions_coalesce() {
        let mut service = SpawnerNotificationService::default();
        service.queue_completion(
            1,
            &child(3, "second\nchild"),
            Completion {
                id: 3,
                process_id: 3,
                completed_at_ms: 30,
            },
        );
        service.queue_completion(
            1,
            &child(2, "first child"),
            Completion {
                id: 2,
                process_id: 2,
                completed_at_ms: 20,
            },
        );
        service.queue_completion(
            1,
            &child(2, "first child"),
            Completion {
                id: 4,
                process_id: 2,
                completed_at_ms: 40,
            },
        );
        let children = service.pending.get(&1).unwrap();
        assert_eq!(children.len(), 2);
        assert_eq!(children[&2].completion.unwrap().id, 4);
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
                "stty -echo; printf '❯\\n'; while IFS= read -r line; do printf 'received:[%s]\\n' \"$line\"; done",
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
        let output =
            String::from_utf8_lossy(&registry.raw_output(1, None, 64 * 1024).unwrap().data)
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

    #[cfg(unix)]
    #[test]
    fn opted_in_completion_delivers_once_and_marks_only_that_child_reported() {
        let mut registry = registry();
        registry
            .create(process(
                1,
                "parent",
                "stty -echo; printf '❯\\n'; while IFS= read -r line; do printf 'received:[%s]\\n❯\\n' \"$line\"; done",
            ))
            .unwrap();
        for (process_id, name) in [(2, "opted"), (3, "not-opted-in")] {
            registry
                .create(process(
                    process_id,
                    name,
                    "printf '❯\\n'; while IFS= read -r line; do printf 'thinking...\\nesc to interrupt\\n'; sleep 0.2; printf 'done\\n❯\\n'; done",
                ))
                .unwrap();
        }
        for process_id in 1..=3 {
            registry.start(process_id).unwrap();
        }
        for process_id in 1..=3 {
            wait_for_state(&mut registry, process_id, AttentionState::Idle);
        }
        registry.set_notify_spawner_on_idle(1, 2, true).unwrap();
        submit_as(&mut registry, 1, 2, b"go");
        submit_as(&mut registry, 1, 3, b"go");
        wait_for_state(&mut registry, 2, AttentionState::Working);
        wait_for_state(&mut registry, 3, AttentionState::Working);
        wait_for_state(&mut registry, 2, AttentionState::Idle);
        wait_for_state(&mut registry, 3, AttentionState::Idle);

        let mut service = SpawnerNotificationService::default();
        service.tick(&mut registry).unwrap();
        wait_for_output(&mut registry, 1, "opted (2) finished");
        wait_for_state(&mut registry, 1, AttentionState::Idle);
        service.tick(&mut registry).unwrap();
        let output =
            String::from_utf8_lossy(&registry.raw_output(1, None, 64 * 1024).unwrap().data)
                .into_owned();
        assert_eq!(output.matches("[workman] child idle:").count(), 1);
        assert!(!output.contains("not-opted-in (3)"));
        let unreported = CompletionLedger::new(registry.store())
            .unreported_completions(1, &[2, 3])
            .unwrap();
        assert_eq!(
            unreported
                .iter()
                .map(|completion| completion.process_id)
                .collect::<Vec<_>>(),
            vec![3]
        );

        registry.stop(1).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn two_completions_wait_for_busy_spawner_and_arrive_in_one_turn() {
        let mut registry = registry();
        registry
            .create(process(
                1,
                "parent",
                "stty -echo; printf '❯\\n'; while IFS= read -r line; do if [ \"$line\" = hold ]; then printf 'thinking...\\nesc to interrupt\\n'; sleep 1; printf 'parent done\\n❯\\n'; else printf 'received:[%s]\\n❯\\n' \"$line\"; fi; done",
            ))
            .unwrap();
        for (process_id, name) in [(2, "first"), (3, "second")] {
            registry
                .create(process(
                    process_id,
                    name,
                    "printf '❯\\n'; while IFS= read -r line; do printf 'thinking...\\nesc to interrupt\\n'; sleep 0.1; printf 'done\\n❯\\n'; done",
                ))
                .unwrap();
        }
        for process_id in 1..=3 {
            registry.start(process_id).unwrap();
        }
        for process_id in 1..=3 {
            wait_for_state(&mut registry, process_id, AttentionState::Idle);
        }
        for process_id in 2..=3 {
            registry
                .set_notify_spawner_on_idle(1, process_id, true)
                .unwrap();
        }
        submit_as(&mut registry, 1, 1, b"hold");
        wait_for_state(&mut registry, 1, AttentionState::Working);
        submit_as(&mut registry, 1, 2, b"go");
        submit_as(&mut registry, 1, 3, b"go");
        wait_for_state(&mut registry, 2, AttentionState::Idle);
        wait_for_state(&mut registry, 3, AttentionState::Idle);

        let mut service = SpawnerNotificationService::default();
        service.tick(&mut registry).unwrap();
        assert_eq!(service.pending.get(&1).unwrap().len(), 2);
        let before =
            String::from_utf8_lossy(&registry.raw_output(1, None, 64 * 1024).unwrap().data)
                .into_owned();
        assert!(!before.contains("[workman] child idle:"));

        wait_for_state(&mut registry, 1, AttentionState::Idle);
        service.tick(&mut registry).unwrap();
        wait_for_output(&mut registry, 1, "[workman] child idle:");
        let output =
            String::from_utf8_lossy(&registry.raw_output(1, None, 64 * 1024).unwrap().data)
                .into_owned();
        assert_eq!(output.matches("[workman] child idle:").count(), 1);
        assert!(output.contains("first (2) finished; second (3) finished"));

        registry.stop(1).unwrap();
    }
}
