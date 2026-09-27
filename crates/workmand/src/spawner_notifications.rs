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
    Process, ProcessId, ProcessKind, ProcessStatus, ProjectId, SpawnerIdleNotificationSetting,
    SpawnerReportedState,
    attention::{AgentState, AttentionState},
};

use crate::{
    ProcessRegistry, RegistryResult, SharedProcessRegistry,
    completion_ledger::{Completion, CompletionLedger},
    status_invalidation::StatusInvalidationHub,
    timers::now_millis,
};

const NOTIFICATION_POLL_INTERVAL: Duration = Duration::from_millis(100);
const NEEDS_INPUT_RESET_CONFIRMATION_MS: i64 = 3_000;
const ERROR_LOG_INTERVAL_MS: i64 = 60_000;
const DRAFT_HOLD_LOG_AFTER_MS: i64 = 10 * 60 * 1_000;
const MAX_CHILD_NAME_CHARS: usize = 80;
const MAX_NOTIFICATION_ENTRIES: usize = 10;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ChildNotificationReason {
    Finished,
    NeedsInput,
    Exited,
    Crashed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NotificationRelevance {
    Deliver,
    Hold,
    Drop,
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
    child_project_id: ProjectId,
    reason: ChildNotificationReason,
    completion: Option<Completion>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ObservationKey {
    state: AttentionState,
    last_input_at: Option<i64>,
    work_evidence_at: Option<i64>,
    pending_prompt: bool,
}

#[derive(Clone, Debug)]
struct EligibleChild {
    process: Process,
    setting: SpawnerIdleNotificationSetting,
    attention: AgentState,
    waiting: bool,
    explicit_idle_timer: bool,
}

#[derive(Clone, Copy, Debug)]
struct DraftHold {
    started_at_ms: i64,
    logged: bool,
}

#[derive(Default)]
pub(crate) struct SpawnerNotificationService {
    pending: BTreeMap<ProcessId, BTreeMap<ProcessId, PendingChildNotification>>,
    observations: BTreeMap<ProcessId, ObservationKey>,
    needs_input_clear_since: BTreeMap<ProcessId, i64>,
    reported_attention: BTreeMap<ProcessId, SpawnerReportedState>,
    suppressed_completion_ids: BTreeMap<(ProcessId, ProcessId), i64>,
    draft_holds: BTreeMap<ProcessId, DraftHold>,
    last_error_log_at: BTreeMap<String, i64>,
}

impl SpawnerNotificationService {
    #[cfg(test)]
    pub(crate) fn tick(&mut self, registry: &mut ProcessRegistry) -> RegistryResult<bool> {
        self.tick_at(registry, now_millis())
    }

    fn tick_at(&mut self, registry: &mut ProcessRegistry, now_ms: i64) -> RegistryResult<bool> {
        registry.refresh_exits()?;
        let settings = registry.store().list_spawner_idle_notification_settings()?;
        let timer_context = if settings.is_empty() {
            Default::default()
        } else {
            registry.notification_timer_context()?
        };
        let enabled = settings
            .iter()
            .map(|setting| setting.process_id)
            .collect::<HashSet<_>>();
        self.pending.retain(|_, children| {
            children.retain(|child_id, _| enabled.contains(child_id));
            !children.is_empty()
        });
        self.observations
            .retain(|child_id, _| enabled.contains(child_id));
        self.needs_input_clear_since
            .retain(|child_id, _| enabled.contains(child_id));
        self.reported_attention
            .retain(|child_id, _| enabled.contains(child_id));
        let mut eligible_children =
            BTreeMap::<ProcessId, BTreeMap<ProcessId, EligibleChild>>::new();
        let mut poll_again = !self.pending.is_empty();

        for setting in settings {
            if let Err(error) = self.observe_child(
                registry,
                setting,
                now_ms,
                &timer_context.owned_idle_watches,
                &timer_context.waiting_processes,
                &mut eligible_children,
                &mut poll_again,
            ) {
                self.log_error(
                    format!("child:{}", setting.process_id),
                    now_ms,
                    format!(
                        "process {}: child-to-spawner observation failed: {error}",
                        setting.process_id
                    ),
                );
            }
        }

        for (spawner_id, children) in eligible_children {
            if let Err(error) = self.observe_completions(registry, spawner_id, &children, now_ms) {
                self.log_error(
                    format!("completions:{spawner_id}"),
                    now_ms,
                    format!("process {spawner_id}: child completion observation failed: {error}"),
                );
            }
        }

        self.deliver_ready(registry, now_ms);
        Ok(poll_again || !self.pending.is_empty())
    }

    fn observe_child(
        &mut self,
        registry: &mut ProcessRegistry,
        setting: SpawnerIdleNotificationSetting,
        now_ms: i64,
        owned_idle_watches: &HashSet<(ProcessId, ProcessId)>,
        waiting_processes: &HashSet<ProcessId>,
        eligible_children: &mut BTreeMap<ProcessId, BTreeMap<ProcessId, EligibleChild>>,
        poll_again: &mut bool,
    ) -> RegistryResult<()> {
        let Some(child) = registry.store().get_process(setting.process_id)? else {
            return Ok(());
        };
        let effective_reported = self
            .reported_attention
            .get(&child.id)
            .copied()
            .unwrap_or(setting.last_reported_state);
        let terminal_state = match child.status {
            ProcessStatus::Exited => Some(SpawnerReportedState::Exited),
            ProcessStatus::Crashed => Some(SpawnerReportedState::Crashed),
            _ => None,
        };
        if terminal_state.is_some_and(|state| state == effective_reported) {
            registry
                .store()
                .set_spawner_idle_notification(child.id, false, 0, 0)?;
            self.remove_child(child.id);
            return Ok(());
        }
        if child.status == ProcessStatus::Stopped {
            return Ok(());
        }
        *poll_again |= matches!(
            child.status,
            ProcessStatus::Starting | ProcessStatus::Running
        );

        let Some(spawner_id) = child.spawned_by_process_id else {
            self.disable_invalid_setting(registry, &child, "its spawner no longer exists", now_ms);
            return Ok(());
        };
        let Some(spawner) = registry.store().get_process(spawner_id)? else {
            self.disable_invalid_setting(
                registry,
                &child,
                &format!("spawner {spawner_id} no longer exists"),
                now_ms,
            );
            return Ok(());
        };
        if spawner.kind != ProcessKind::Agent {
            self.disable_invalid_setting(
                registry,
                &child,
                &format!("spawner {spawner_id} is not an agent"),
                now_ms,
            );
            return Ok(());
        }
        if spawner.project_id != child.project_id {
            self.disable_invalid_setting(
                registry,
                &child,
                &format!("spawner {spawner_id} is in another project"),
                now_ms,
            );
            return Ok(());
        }

        let attention = registry.agent_attention_snapshot(child.id)?;
        let waiting =
            attention.state == AttentionState::Idle && waiting_processes.contains(&child.id);
        let observed_state = if waiting {
            AttentionState::Waiting
        } else {
            attention.state
        };
        let explicit_idle_timer = owned_idle_watches.contains(&(spawner_id, child.id));
        let has_pending_prompts = registry.has_pending_prompts(child.id);
        let observation = ObservationKey {
            state: observed_state,
            last_input_at: attention.last_input_at,
            work_evidence_at: attention.work_evidence_at(),
            pending_prompt: has_pending_prompts,
        };
        if self.observations.get(&child.id) != Some(&observation) {
            CompletionLedger::new(registry.store()).observe_process(
                child.id,
                observed_state,
                attention.last_input_at,
                attention.work_evidence_at(),
                has_pending_prompts,
                now_ms,
            )?;
            self.observations.insert(child.id, observation);
        }

        eligible_children.entry(spawner_id).or_default().insert(
            child.id,
            EligibleChild {
                process: child.clone(),
                setting,
                attention: attention.clone(),
                waiting,
                explicit_idle_timer,
            },
        );
        if explicit_idle_timer {
            self.remove_pending_child(spawner_id, child.id);
        } else {
            self.observe_attention_reason(
                registry,
                spawner_id,
                &child,
                child_reason(child.status, observed_state),
                effective_reported,
                now_ms,
            );
        }
        Ok(())
    }

    fn observe_attention_reason(
        &mut self,
        registry: &ProcessRegistry,
        spawner_id: ProcessId,
        child: &Process,
        reason: Option<ChildNotificationReason>,
        effective_reported: SpawnerReportedState,
        now_ms: i64,
    ) {
        match reason {
            Some(reason) => {
                self.needs_input_clear_since.remove(&child.id);
                if reason.reported_state() != Some(effective_reported) {
                    self.queue(spawner_id, child, reason);
                }
            }
            None => {
                let has_pending_needs_input = self.pending.values().any(|children| {
                    children.get(&child.id).is_some_and(|pending| {
                        pending.reason == ChildNotificationReason::NeedsInput
                    })
                });
                if effective_reported == SpawnerReportedState::NeedsInput || has_pending_needs_input
                {
                    let clear_since = self
                        .needs_input_clear_since
                        .entry(child.id)
                        .or_insert(now_ms);
                    if now_ms.saturating_sub(*clear_since) >= NEEDS_INPUT_RESET_CONFIRMATION_MS {
                        self.reported_attention
                            .insert(child.id, SpawnerReportedState::Neutral);
                        if let Err(error) = registry
                            .store()
                            .set_spawner_reported_state(child.id, SpawnerReportedState::Neutral)
                        {
                            self.log_error(
                                format!("state-reset:{}", child.id),
                                now_ms,
                                format!(
                                    "process {}: could not persist needs-input reset: {error}",
                                    child.id
                                ),
                            );
                        }
                        self.remove_pending_reason(child.id, ChildNotificationReason::NeedsInput);
                        self.needs_input_clear_since.remove(&child.id);
                    }
                } else {
                    self.needs_input_clear_since.remove(&child.id);
                }
            }
        }
    }

    fn observe_completions(
        &mut self,
        registry: &ProcessRegistry,
        spawner_id: ProcessId,
        children: &BTreeMap<ProcessId, EligibleChild>,
        now_ms: i64,
    ) -> RegistryResult<()> {
        let child_ids = children.keys().copied().collect::<Vec<_>>();
        let completions = CompletionLedger::new(registry.store())
            .unreported_completions(spawner_id, &child_ids)?;
        let mut current_completions = BTreeMap::new();
        for completion in completions {
            let Some(child) = children.get(&completion.process_id) else {
                continue;
            };
            let suppressed = self
                .suppressed_completion_ids
                .get(&(spawner_id, completion.process_id))
                .copied()
                .unwrap_or(0);
            if completion.id <= child.setting.baseline_completion_id || completion.id <= suppressed
            {
                continue;
            }
            if child.explicit_idle_timer {
                continue;
            }
            if child.waiting {
                self.suppressed_completion_ids
                    .entry((spawner_id, completion.process_id))
                    .and_modify(|current| *current = (*current).max(completion.id))
                    .or_insert(completion.id);
                continue;
            }
            current_completions.insert(completion.process_id, completion);
        }

        // Pending finished entries are a projection of the ledger, not an append-only queue.
        // Re-prompts move the owner's input baseline and explicit timers may consume a
        // completion, so remove anything the current unreported query no longer returns.
        let remove_spawner = self.pending.get_mut(&spawner_id).is_some_and(|pending| {
            pending.retain(|child_id, notification| {
                if !children.contains_key(child_id) {
                    return true;
                }
                notification.completion = current_completions.get(child_id).copied();
                notification.completion.is_some()
                    || notification.reason != ChildNotificationReason::Finished
            });
            pending.is_empty()
        });
        if remove_spawner {
            self.pending.remove(&spawner_id);
        }

        for completion in current_completions.into_values() {
            let child = &children[&completion.process_id];
            if child.attention.state != AttentionState::NeedsInput {
                self.needs_input_clear_since.remove(&completion.process_id);
                self.reported_attention
                    .insert(completion.process_id, SpawnerReportedState::Neutral);
                if let Err(error) = registry.store().set_spawner_reported_state(
                    completion.process_id,
                    SpawnerReportedState::Neutral,
                ) {
                    self.log_error(
                        format!("completion-reset:{}", completion.process_id),
                        now_ms,
                        format!(
                            "process {}: could not persist completion state reset: {error}",
                            completion.process_id
                        ),
                    );
                }
            }
            self.queue_completion(
                spawner_id,
                &child.process,
                completion,
                child.attention.state,
            );
        }
        Ok(())
    }

    fn queue_completion(
        &mut self,
        spawner_id: ProcessId,
        child: &Process,
        completion: Completion,
        attention_state: AttentionState,
    ) {
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
                if attention_state != AttentionState::NeedsInput {
                    if !matches!(
                        pending.reason,
                        ChildNotificationReason::Exited | ChildNotificationReason::Crashed
                    ) {
                        pending.reason = ChildNotificationReason::Finished;
                    }
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

    fn deliver_ready(&mut self, registry: &mut ProcessRegistry, now_ms: i64) {
        let spawner_ids = self.pending.keys().copied().collect::<Vec<_>>();
        for spawner_id in spawner_ids {
            if let Err(error) = self.deliver_spawner(registry, spawner_id, now_ms) {
                self.log_error(
                    format!("delivery:{spawner_id}"),
                    now_ms,
                    format!("process {spawner_id}: child notification delivery failed: {error}"),
                );
            }
        }
    }

    fn deliver_spawner(
        &mut self,
        registry: &mut ProcessRegistry,
        spawner_id: ProcessId,
        now_ms: i64,
    ) -> RegistryResult<()> {
        let Some(spawner) = registry.store().get_process(spawner_id)? else {
            self.drop_for_unavailable_spawner(registry, spawner_id, "was deleted", now_ms);
            return Ok(());
        };
        if spawner.kind != ProcessKind::Agent {
            self.drop_for_unavailable_spawner(registry, spawner_id, "is not an agent", now_ms);
            return Ok(());
        }
        if matches!(
            spawner.status,
            ProcessStatus::Stopped | ProcessStatus::Exited | ProcessStatus::Crashed
        ) {
            self.drop_for_unavailable_spawner(registry, spawner_id, "is not running", now_ms);
            return Ok(());
        }
        if spawner.status != ProcessStatus::Running {
            return Ok(());
        }
        let attention = registry.agent_attention_snapshot(spawner_id)?;
        if registry.has_pending_prompts(spawner_id)
            || registry
                .input_router()
                .automatic_submission_held(spawner_id)?
            || !matches!(
                attention.state,
                AttentionState::Idle | AttentionState::Waiting
            )
        {
            return Ok(());
        }
        if registry.input_router().has_unsent_human_draft(spawner_id)? {
            let hold = self.draft_holds.entry(spawner_id).or_insert(DraftHold {
                started_at_ms: now_ms,
                logged: false,
            });
            if !hold.logged && now_ms.saturating_sub(hold.started_at_ms) >= DRAFT_HOLD_LOG_AFTER_MS
            {
                eprintln!(
                    "process {spawner_id}: child notification remains held behind an unsent human draft"
                );
                hold.logged = true;
            }
            return Ok(());
        }
        self.draft_holds.remove(&spawner_id);

        let timer_context = registry.notification_timer_context()?;
        let pending = self.pending.get(&spawner_id).cloned().unwrap_or_default();
        let mut deliverable = Vec::new();
        for child in pending.values() {
            if child.child_project_id != spawner.project_id {
                self.remove_pending_child(spawner_id, child.child_process_id);
                self.log_error(
                    format!("cross-project:{}:{}", spawner_id, child.child_process_id),
                    now_ms,
                    format!(
                        "process {spawner_id}: dropped invalid cross-project child notification for {}",
                        child.child_process_id
                    ),
                );
                continue;
            }
            match self.notification_relevance(
                registry,
                spawner_id,
                child,
                &timer_context.owned_idle_watches,
                &timer_context.waiting_processes,
            )? {
                NotificationRelevance::Deliver => deliverable.push(child.clone()),
                NotificationRelevance::Hold => {}
                NotificationRelevance::Drop => {
                    self.remove_pending_child(spawner_id, child.child_process_id);
                }
            }
        }
        if deliverable.is_empty() {
            return Ok(());
        }
        let body = notification_body(deliverable.iter());
        registry.submit_input(spawner_id, body.as_bytes())?;

        // The PTY side effect succeeded. Suppress and clear this exact batch before every
        // best-effort bookkeeping step so a database error can never cause a duplicate turn.
        for child in &deliverable {
            if let Some(completion) = child.completion {
                self.suppressed_completion_ids
                    .entry((spawner_id, child.child_process_id))
                    .and_modify(|current| *current = (*current).max(completion.id))
                    .or_insert(completion.id);
            }
            if let Some(state) = child.reason.reported_state() {
                self.reported_attention
                    .insert(child.child_process_id, state);
            }
            self.remove_pending_child(spawner_id, child.child_process_id);
        }

        if let Err(error) = CompletionLedger::new(registry.store()).mark_completions_reported(
            spawner_id,
            deliverable.iter().filter_map(|child| child.completion),
        ) {
            self.log_error(
                format!("ledger-mark:{spawner_id}"),
                now_ms,
                format!(
                    "process {spawner_id}: delivered child notification but could not mark completions reported: {error}"
                ),
            );
        }
        for child in &deliverable {
            if let Some(state) = child.reason.reported_state()
                && let Err(error) = registry
                    .store()
                    .set_spawner_reported_state(child.child_process_id, state)
            {
                self.log_error(
                    format!("state-mark:{}", child.child_process_id),
                    now_ms,
                    format!(
                        "process {}: delivered child notification but could not persist state: {error}",
                        child.child_process_id
                    ),
                );
            }
            if matches!(
                child.reason,
                ChildNotificationReason::Exited | ChildNotificationReason::Crashed
            ) && let Err(error) =
                registry
                    .store()
                    .set_spawner_idle_notification(child.child_process_id, false, 0, 0)
            {
                self.log_error(
                    format!("terminal-disable:{}", child.child_process_id),
                    now_ms,
                    format!(
                        "process {}: could not clear terminal child notification setting: {error}",
                        child.child_process_id
                    ),
                );
            }
        }
        Ok(())
    }

    fn notification_relevance(
        &mut self,
        registry: &ProcessRegistry,
        spawner_id: ProcessId,
        pending: &PendingChildNotification,
        owned_idle_watches: &HashSet<(ProcessId, ProcessId)>,
        waiting_processes: &HashSet<ProcessId>,
    ) -> RegistryResult<NotificationRelevance> {
        let Some(process) = registry.store().get_process(pending.child_process_id)? else {
            return Ok(NotificationRelevance::Drop);
        };
        if owned_idle_watches.contains(&(spawner_id, process.id)) {
            return Ok(NotificationRelevance::Drop);
        }
        match pending.reason {
            ChildNotificationReason::Finished => {
                let Some(completion) = pending.completion else {
                    return Ok(NotificationRelevance::Drop);
                };
                let current = CompletionLedger::new(registry.store())
                    .unreported_completions(spawner_id, &[process.id])?
                    .into_iter()
                    .find(|current| current.id == completion.id);
                if current.is_none() || registry.has_pending_prompts(process.id) {
                    return Ok(NotificationRelevance::Drop);
                }
                let attention = registry.agent_attention_snapshot(process.id)?;
                if attention
                    .last_input_at
                    .is_some_and(|input_at| input_at >= completion.completed_at_ms)
                {
                    return Ok(NotificationRelevance::Drop);
                }
                if waiting_processes.contains(&process.id) {
                    self.suppressed_completion_ids
                        .entry((spawner_id, process.id))
                        .and_modify(|current| *current = (*current).max(completion.id))
                        .or_insert(completion.id);
                    return Ok(NotificationRelevance::Drop);
                }
                Ok(if attention.state == AttentionState::Idle {
                    NotificationRelevance::Deliver
                } else {
                    NotificationRelevance::Hold
                })
            }
            ChildNotificationReason::NeedsInput => Ok(
                if registry.agent_attention_snapshot(process.id)?.state
                    == AttentionState::NeedsInput
                {
                    NotificationRelevance::Deliver
                } else {
                    NotificationRelevance::Hold
                },
            ),
            ChildNotificationReason::Exited => Ok(if process.status == ProcessStatus::Exited {
                NotificationRelevance::Deliver
            } else {
                NotificationRelevance::Drop
            }),
            ChildNotificationReason::Crashed => Ok(if process.status == ProcessStatus::Crashed {
                NotificationRelevance::Deliver
            } else {
                NotificationRelevance::Drop
            }),
        }
    }

    fn drop_for_unavailable_spawner(
        &mut self,
        registry: &ProcessRegistry,
        spawner_id: ProcessId,
        detail: &str,
        now_ms: i64,
    ) {
        let Some(dropped) = self.pending.remove(&spawner_id) else {
            return;
        };
        for pending in dropped.values() {
            if let Some(completion) = pending.completion {
                self.suppressed_completion_ids
                    .entry((spawner_id, pending.child_process_id))
                    .and_modify(|current| *current = (*current).max(completion.id))
                    .or_insert(completion.id);
            }
            if let Some(state) = pending.reason.reported_state() {
                self.reported_attention
                    .insert(pending.child_process_id, state);
                if let Err(error) = registry
                    .store()
                    .set_spawner_reported_state(pending.child_process_id, state)
                {
                    self.log_error(
                        format!("drop-state:{}", pending.child_process_id),
                        now_ms,
                        format!(
                            "process {}: dropped child notification but could not persist state: {error}",
                            pending.child_process_id
                        ),
                    );
                }
            }
        }
        eprintln!(
            "process {spawner_id}: dropped {} coalesced child notification(s) because the spawner {detail}",
            dropped.len()
        );
    }

    fn disable_invalid_setting(
        &mut self,
        registry: &ProcessRegistry,
        child: &Process,
        detail: &str,
        now_ms: i64,
    ) {
        if let Err(error) = registry
            .store()
            .set_spawner_idle_notification(child.id, false, 0, 0)
        {
            self.log_error(
                format!("invalid-disable:{}", child.id),
                now_ms,
                format!(
                    "process {}: could not disable invalid notify_spawner_on_idle setting: {error}",
                    child.id
                ),
            );
            return;
        }
        self.remove_child(child.id);
        eprintln!(
            "process {}: disabled notify_spawner_on_idle because {detail}",
            child.id
        );
    }

    fn remove_child(&mut self, child_id: ProcessId) {
        self.pending.retain(|_, children| {
            children.remove(&child_id);
            !children.is_empty()
        });
        self.observations.remove(&child_id);
        self.needs_input_clear_since.remove(&child_id);
        self.reported_attention.remove(&child_id);
    }

    fn remove_pending_child(&mut self, spawner_id: ProcessId, child_id: ProcessId) {
        let remove_spawner = self.pending.get_mut(&spawner_id).is_some_and(|children| {
            children.remove(&child_id);
            children.is_empty()
        });
        if remove_spawner {
            self.pending.remove(&spawner_id);
        }
    }

    fn log_error(&mut self, key: String, now_ms: i64, message: String) {
        let should_log = self
            .last_error_log_at
            .get(&key)
            .is_none_or(|last| now_ms.saturating_sub(*last) >= ERROR_LOG_INTERVAL_MS);
        if should_log {
            eprintln!("{message}");
            self.last_error_log_at.insert(key, now_ms);
        }
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
    let children = children.into_iter().collect::<Vec<_>>();
    let mut entries = children
        .iter()
        .take(MAX_NOTIFICATION_ENTRIES)
        .map(|pending| {
            format!(
                "{} ({}) {}",
                pending.child_name,
                pending.child_process_id,
                pending.reason.label()
            )
        })
        .collect::<Vec<_>>();
    if children.len() > MAX_NOTIFICATION_ENTRIES {
        entries.push(format!(
            "and {} more",
            children.len() - MAX_NOTIFICATION_ENTRIES
        ));
    }
    let entries = entries.join("; ");
    format!("[workman] child idle: {entries} — check their output/todos")
}

fn single_line_name(name: &str) -> String {
    let safe = name
        .chars()
        .map(|character| {
            if character.is_control() || is_unsafe_unicode_format(character) {
                ' '
            } else {
                character
            }
        })
        .collect::<String>();
    let collapsed = safe.split_whitespace().collect::<Vec<_>>().join(" ");
    let collapsed = if collapsed.is_empty() {
        "unnamed".to_owned()
    } else {
        collapsed
    };
    if collapsed.chars().count() <= MAX_CHILD_NAME_CHARS {
        return collapsed;
    }
    let mut truncated = collapsed
        .chars()
        .take(MAX_CHILD_NAME_CHARS.saturating_sub(1))
        .collect::<String>();
    truncated.push('…');
    truncated
}

fn is_unsafe_unicode_format(character: char) -> bool {
    matches!(
        character as u32,
        0x00AD
            | 0x061C
            | 0x180E
            | 0x200B..=0x200F
            | 0x202A..=0x202E
            | 0x2060..=0x206F
            | 0xFEFF
            | 0xFFF9..=0xFFFB
            | 0xE0001
            | 0xE0020..=0xE007F
    )
}

pub(crate) fn spawn_spawner_notification_scheduler(
    registry: SharedProcessRegistry,
    status_invalidations: StatusInvalidationHub,
    mut shutdown: watch::Receiver<bool>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut service = SpawnerNotificationService::default();
        let mut ticker = interval(NOTIFICATION_POLL_INTERVAL);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
        let mut poll_active = true;
        let mut last_version = status_invalidations.version_at(now_millis());
        let mut last_scheduler_error_at = None;
        loop {
            tokio::select! {
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        break;
                    }
                }
                _ = ticker.tick() => {
                    let now_ms = now_millis();
                    let version = status_invalidations.version_at(now_ms);
                    if !poll_active && version == last_version {
                        continue;
                    }
                    last_version = version;
                    let mut registry = registry.lock().await;
                    match service.tick_at(&mut registry, now_ms) {
                        Ok(active) => {
                            poll_active = active;
                            last_scheduler_error_at = None;
                        }
                        Err(error) => {
                            poll_active = true;
                            if last_scheduler_error_at.is_none_or(|last: i64| {
                                now_ms.saturating_sub(last) >= ERROR_LOG_INTERVAL_MS
                            }) {
                                eprintln!("child-to-spawner notification tick failed: {error}");
                                last_scheduler_error_at = Some(now_ms);
                            }
                        }
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
    use workman_core::{
        Actor, AgentTool, AgentToolSource, ProcessKind, ProcessSource, Project, Store, TimerKind,
    };

    use crate::{
        RegistryError,
        timers::{IdleTimerOutcome, TimerSatisfactionReason, TimerService, now_millis},
    };

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
    fn wait_for_attention_snapshot(
        registry: &ProcessRegistry,
        process_id: ProcessId,
        expected: AttentionState,
    ) {
        let deadline = Instant::now() + Duration::from_secs(7);
        loop {
            let state = registry.agent_attention_snapshot(process_id).unwrap().state;
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

    fn put_actor(registry: &ProcessRegistry, actor_id: &str, process_id: ProcessId) {
        registry
            .store()
            .put_actor(&Actor {
                id: actor_id.into(),
                session_id: format!("{actor_id}-session"),
                process_id: Some(process_id),
                selected_project_id: Some(1),
                created_at: 1_000,
                last_seen_at: 1_000,
            })
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
            AttentionState::Idle,
        );
        service.queue_completion(
            1,
            &child(2, "first child"),
            Completion {
                id: 2,
                process_id: 2,
                completed_at_ms: 20,
            },
            AttentionState::Idle,
        );
        service.queue_completion(
            1,
            &child(2, "first child"),
            Completion {
                id: 4,
                process_id: 2,
                completed_at_ms: 40,
            },
            AttentionState::Idle,
        );
        let children = service.pending.get(&1).unwrap();
        assert_eq!(children.len(), 2);
        assert_eq!(children[&2].completion.unwrap().id, 4);
        assert_eq!(
            notification_body(children.values()),
            "[workman] child idle: first child (2) finished; second child (3) finished — check their output/todos"
        );
    }

    #[test]
    fn body_sanitizes_controls_caps_names_and_summarizes_large_batches() {
        let dangerous = format!("x\x1b[201~\x15\x03\x7f\0y\u{202e}{}", "z".repeat(120));
        let mut service = SpawnerNotificationService::default();
        for process_id in 2..=13 {
            service.queue(
                1,
                &child(
                    process_id,
                    if process_id == 2 {
                        &dangerous
                    } else {
                        "ordinary child"
                    },
                ),
                ChildNotificationReason::NeedsInput,
            );
        }
        let children = service.pending.get(&1).unwrap();
        let body = notification_body(children.values());
        assert!(!body.chars().any(char::is_control), "{body:?}");
        assert!(!body.contains('\u{202e}'));
        assert!(!body.contains("\x1b[201~"));
        assert!(body.contains('…'));
        assert!(body.contains("and 2 more"), "{body}");
        assert!(!body.contains("ordinary child (13)"), "{body}");
        assert!(body.chars().count() < 1_200, "body was too long");
        assert_eq!(single_line_name(&"a".repeat(200)).chars().count(), 80);
    }

    #[cfg(unix)]
    #[test]
    fn only_an_agent_spawner_can_enable_or_receive_notifications() {
        let mut registry = registry();
        let mut parent = process(1, "command-parent", "sleep 30");
        parent.kind = ProcessKind::Command;
        parent.agent_tool_id = None;
        registry.create(parent).unwrap();
        registry
            .create(process(
                2,
                "child",
                "printf '❯\\n'; while IFS= read -r line; do :; done",
            ))
            .unwrap();
        assert!(matches!(
            registry.set_notify_spawner_on_idle(1, 2, true),
            Err(RegistryError::SpawnerNotificationRequesterRequiresAgent(1))
        ));

        registry.start(2).unwrap();
        registry
            .store()
            .set_spawner_idle_notification(2, true, now_millis(), 0)
            .unwrap();
        let mut service = SpawnerNotificationService::default();
        service.tick(&mut registry).unwrap();
        assert!(
            !registry
                .store()
                .spawner_idle_notification_enabled(2)
                .unwrap()
        );
        registry.stop(2).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn enabling_after_an_unobserved_completion_uses_its_id_as_the_baseline() {
        let mut registry = registry();
        registry
            .create(process(
                1,
                "parent",
                "stty -echo; printf '❯\\n'; while IFS= read -r line; do printf 'received:[%s]\\n❯\\n' \"$line\"; done",
            ))
            .unwrap();
        registry
            .create(process(
                2,
                "child",
                "printf '❯\\n'; while IFS= read -r line; do printf 'thinking...\\nesc to interrupt\\n'; sleep 0.2; printf 'done\\n❯\\n'; done",
            ))
            .unwrap();
        registry.start(1).unwrap();
        registry.start(2).unwrap();
        wait_for_attention_snapshot(&registry, 1, AttentionState::Idle);
        wait_for_attention_snapshot(&registry, 2, AttentionState::Idle);

        submit_as(&mut registry, 1, 2, b"before-enable");
        wait_for_attention_snapshot(&registry, 2, AttentionState::Working);
        wait_for_attention_snapshot(&registry, 2, AttentionState::Idle);
        registry.set_notify_spawner_on_idle(1, 2, true).unwrap();
        let setting = registry
            .store()
            .spawner_idle_notification_setting(2)
            .unwrap()
            .unwrap();
        assert!(setting.baseline_completion_id > 0);

        let mut service = SpawnerNotificationService::default();
        service.tick(&mut registry).unwrap();
        let output = registry.rendered_output(1).unwrap().text;
        assert!(!output.contains("[workman] child idle:"), "{output:?}");

        submit_as(&mut registry, 1, 2, b"after-enable");
        wait_for_attention_snapshot(&registry, 2, AttentionState::Working);
        wait_for_attention_snapshot(&registry, 2, AttentionState::Idle);
        service.tick(&mut registry).unwrap();
        wait_for_output(&mut registry, 1, "child (2) finished");
        registry.stop(1).unwrap();
        registry.stop(2).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn unchanged_notification_ticks_do_not_write_the_store() {
        let mut registry = registry();
        registry
            .create(process(
                1,
                "parent",
                "printf '❯\\n'; while IFS= read -r line; do :; done",
            ))
            .unwrap();
        registry
            .create(process(
                2,
                "child",
                "printf '❯\\n'; while IFS= read -r line; do :; done",
            ))
            .unwrap();
        registry.start(1).unwrap();
        registry.start(2).unwrap();
        wait_for_attention_snapshot(&registry, 1, AttentionState::Idle);
        wait_for_attention_snapshot(&registry, 2, AttentionState::Idle);
        registry.set_notify_spawner_on_idle(1, 2, true).unwrap();

        let mut service = SpawnerNotificationService::default();
        let now = now_millis();
        service.tick_at(&mut registry, now).unwrap();
        let changes = registry.store().connection().total_changes();
        for offset in 1..=100 {
            service.tick_at(&mut registry, now + offset).unwrap();
        }
        assert_eq!(registry.store().connection().total_changes(), changes);
        registry.stop(1).unwrap();
        registry.stop(2).unwrap();
    }

    #[test]
    fn r7_unchanged_opted_in_children_have_zero_steady_state_row_writes() {
        for child_count in [10, 50, 200] {
            let temp = tempfile::tempdir().unwrap();
            let store = Store::open(temp.path().join("workman.sqlite3")).unwrap();
            store
                .put_project(&Project {
                    id: 1,
                    path: "/tmp".into(),
                    name: format!("r7-{child_count}"),
                    display_name: None,
                    icon: None,
                    selected: true,
                    sort_order: 0,
                })
                .unwrap();
            store
                .put_agent_tool(&AgentTool {
                    id: 90,
                    name: "R7 agent".into(),
                    command: "r7-agent".into(),
                    tool_type: "claude_code".into(),
                    enabled: true,
                    source: AgentToolSource::Local,
                    resume_args: None,
                    continue_args: None,
                })
                .unwrap();
            let mut registry =
                ProcessRegistry::with_stop_grace_for_test(store, Duration::from_millis(50))
                    .unwrap();
            registry.create(process(1, "parent", "sleep 30")).unwrap();
            for index in 0..child_count {
                let process_id = i64::from(index) + 2;
                registry
                    .create(process(process_id, &format!("child-{index}"), "sleep 30"))
                    .unwrap();
            }
            for process_id in 1..=i64::from(child_count) + 1 {
                let mut stored = registry.store().get_process(process_id).unwrap().unwrap();
                stored.status = ProcessStatus::Running;
                registry.store().put_process(&stored).unwrap();
                if process_id != 1 {
                    registry
                        .store()
                        .set_spawner_idle_notification(process_id, true, now_millis(), 0)
                        .unwrap();
                }
            }

            let mut service = SpawnerNotificationService::default();
            let now = now_millis();
            service.tick_at(&mut registry, now).unwrap();
            let changes = registry.store().connection().total_changes();
            let started = Instant::now();
            const TICKS: i64 = 100;
            for offset in 1..=TICKS {
                service.tick_at(&mut registry, now + offset).unwrap();
            }
            let elapsed = started.elapsed();
            let writes = registry.store().connection().total_changes() - changes;
            let average_ms = elapsed.as_secs_f64() * 1_000.0 / TICKS as f64;
            eprintln!(
                "R7 after: children={child_count} row_writes/tick={:.2} row_writes/s={:.2} registry_lock_ms/tick={average_ms:.3}",
                writes as f64 / TICKS as f64,
                writes as f64 / TICKS as f64 * 10.0,
            );
            assert_eq!(writes, 0, "{child_count} children wrote in steady state");
        }
    }

    #[cfg(unix)]
    #[test]
    fn unsent_human_draft_holds_notification_after_typing_pause_is_disabled() {
        let mut registry = registry();
        registry
            .create(process(
                1,
                "parent",
                r#"true claude; stty raw -echo; printf '❯ '; exec perl -e '$|=1; my $draft=""; while (1) { my $n=sysread(STDIN,my $chunk,4096); exit 2 unless defined($n) && $n>0; my $redraw=0; for my $c (split //,$chunk) { if ($c eq "\r") { print "\r\nreceived:[$draft]\r\n❯ "; $draft=""; next; } if (ord($c)==21) { $draft=""; next; } $draft.=$c; $redraw=1; } print "\r\e[2K❯ $draft" if $redraw; }'"#,
            ))
            .unwrap();
        registry
            .create(process(
                2,
                "child",
                "printf '❯\\n'; while IFS= read -r line; do printf 'thinking...\\nesc to interrupt\\n'; sleep 0.2; printf 'done\\n❯\\n'; done",
            ))
            .unwrap();
        registry.start(1).unwrap();
        registry.start(2).unwrap();
        wait_for_state(&mut registry, 1, AttentionState::Idle);
        wait_for_state(&mut registry, 2, AttentionState::Idle);
        registry.set_notify_spawner_on_idle(1, 2, true).unwrap();
        registry
            .input_router()
            .set_typing_pause(crate::settings::TypingPauseSettings {
                enabled: false,
                delay_ms: 1_000,
            });
        registry
            .input_router()
            .send_terminal_input(1, b"human draft", true)
            .unwrap();
        wait_for_output(&mut registry, 1, "❯ human draft");
        wait_for_state(&mut registry, 1, AttentionState::Idle);

        submit_as(&mut registry, 1, 2, b"go");
        wait_for_state(&mut registry, 2, AttentionState::Working);
        wait_for_state(&mut registry, 2, AttentionState::Idle);
        let mut service = SpawnerNotificationService::default();
        service.tick(&mut registry).unwrap();
        let held = registry.rendered_output(1).unwrap().text;
        assert!(!held.contains("[workman] child idle:"), "{held:?}");

        registry
            .input_router()
            .send_terminal_input(1, b"\x15", true)
            .unwrap();
        service.tick(&mut registry).unwrap();
        wait_for_output(&mut registry, 1, "received:[[workman] child idle:");
        let delivered = registry.rendered_output(1).unwrap().text;
        assert!(!delivered.contains("received:[human draft[workman]"));
        registry.stop(1).unwrap();
        registry.stop(2).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn reprompted_child_drops_stale_finished_until_the_new_turn_completes() {
        let mut registry = registry();
        registry
            .create(process(
                1,
                "parent",
                r#"true claude; stty raw -echo; printf '❯ '; exec perl -e '$|=1; my $draft=""; while (1) { my $n=sysread(STDIN,my $chunk,4096); exit 2 unless defined($n) && $n>0; my $redraw=0; for my $c (split //,$chunk) { if ($c eq "\r") { print "\r\nreceived:[$draft]\r\n❯ "; $draft=""; next; } if (ord($c)==21) { $draft=""; next; } $draft.=$c; $redraw=1; } print "\r\e[2K❯ $draft" if $redraw; }'"#,
            ))
            .unwrap();
        registry
            .create(process(
                2,
                "child",
                "printf '❯\\n'; while IFS= read -r line; do printf 'thinking...\\nesc to interrupt\\n'; sleep 0.5; printf 'done:%s\\n❯\\n' \"$line\"; done",
            ))
            .unwrap();
        registry.start(1).unwrap();
        registry.start(2).unwrap();
        wait_for_state(&mut registry, 1, AttentionState::Idle);
        wait_for_state(&mut registry, 2, AttentionState::Idle);
        registry.set_notify_spawner_on_idle(1, 2, true).unwrap();
        registry
            .input_router()
            .set_typing_pause(crate::settings::TypingPauseSettings {
                enabled: false,
                delay_ms: 1_000,
            });
        registry
            .input_router()
            .send_terminal_input(1, b"hold notification", true)
            .unwrap();
        wait_for_output(&mut registry, 1, "❯ hold notification");

        submit_as(&mut registry, 1, 2, b"first");
        wait_for_state(&mut registry, 2, AttentionState::Working);
        wait_for_state(&mut registry, 2, AttentionState::Idle);
        let mut service = SpawnerNotificationService::default();
        service.tick(&mut registry).unwrap();
        assert!(service.pending.get(&1).unwrap().contains_key(&2));

        submit_as(&mut registry, 1, 2, b"second");
        wait_for_attention_snapshot(&registry, 2, AttentionState::Working);
        service.tick(&mut registry).unwrap();
        assert!(
            service
                .pending
                .get(&1)
                .is_none_or(|children| !children.contains_key(&2)),
            "the first completion remained pending after the child was re-prompted"
        );
        registry
            .input_router()
            .send_terminal_input(1, b"\x15", true)
            .unwrap();
        service.tick(&mut registry).unwrap();
        let while_working = registry.rendered_output(1).unwrap().text;
        assert!(!while_working.contains("[workman] child idle:"));

        wait_for_attention_snapshot(&registry, 2, AttentionState::Idle);
        assert!(!registry.input_router().has_unsent_human_draft(1).unwrap());
        service.tick(&mut registry).unwrap();
        assert!(
            service.pending.is_empty(),
            "new completion was still pending: {:?}",
            service.pending
        );
        wait_for_output(&mut registry, 1, "child (2) finished");
        let output = registry.rendered_output(1).unwrap().text;
        assert_eq!(output.matches("[workman] child idle:").count(), 1);
        registry.stop(1).unwrap();
        registry.stop(2).unwrap();
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
    fn transient_working_grace_does_not_reannounce_the_same_needs_input_dialog() {
        let mut registry = registry();
        registry
            .create(process(
                1,
                "parent",
                "stty -echo; printf '❯\\n'; while IFS= read -r line; do printf 'received:[%s]\\n❯\\n' \"$line\"; done",
            ))
            .unwrap();
        registry
            .create(process(
                2,
                "dialog-child",
                "while true; do printf 'Do you want to proceed?\\n❯ 1. Yes, allow\\n  2. No, and tell Claude\\n'; IFS= read -r line || exit; done",
            ))
            .unwrap();
        registry.start(1).unwrap();
        registry.start(2).unwrap();
        registry.set_notify_spawner_on_idle(1, 2, true).unwrap();
        wait_for_state(&mut registry, 1, AttentionState::Idle);
        wait_for_state(&mut registry, 2, AttentionState::NeedsInput);

        let mut service = SpawnerNotificationService::default();
        let base = now_millis();
        service.tick_at(&mut registry, base).unwrap();
        wait_for_output(&mut registry, 1, "dialog-child (2) needs input");
        wait_for_state(&mut registry, 1, AttentionState::Idle);

        registry.submit_input(2, b"").unwrap();
        wait_for_state(&mut registry, 2, AttentionState::Working);
        service.tick_at(&mut registry, base + 500).unwrap();
        wait_for_state(&mut registry, 2, AttentionState::NeedsInput);
        service.tick_at(&mut registry, base + 2_500).unwrap();
        let output =
            String::from_utf8_lossy(&registry.raw_output(1, None, 64 * 1024).unwrap().data)
                .into_owned();
        assert_eq!(output.matches("[workman] child idle:").count(), 1);

        registry.stop(1).unwrap();
        registry.stop(2).unwrap();
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
    fn exited_spawner_drop_leaves_completion_unreported_for_restart() {
        let mut registry = registry();
        registry.create(process(1, "parent", "exit 0")).unwrap();
        registry
            .create(process(
                2,
                "child",
                "printf '❯\\n'; while IFS= read -r line; do printf 'thinking...\\nesc to interrupt\\n'; sleep 0.2; printf 'done\\n❯\\n'; done",
            ))
            .unwrap();
        registry.set_notify_spawner_on_idle(1, 2, true).unwrap();
        registry.start(1).unwrap();
        registry.start(2).unwrap();
        wait_for_state(&mut registry, 1, AttentionState::Exited);
        wait_for_state(&mut registry, 2, AttentionState::Idle);
        submit_as(&mut registry, 1, 2, b"go");
        wait_for_state(&mut registry, 2, AttentionState::Working);
        wait_for_state(&mut registry, 2, AttentionState::Idle);

        let mut service = SpawnerNotificationService::default();
        service.tick(&mut registry).unwrap();
        assert!(service.pending.is_empty());
        assert_eq!(
            CompletionLedger::new(registry.store())
                .unreported_completions(1, &[2])
                .unwrap()
                .len(),
            1,
            "dropping for a down parent must not consume its completion"
        );

        let mut parent = registry.get(1).unwrap();
        parent.command = Some(
            "stty -echo; printf '❯\\n'; while IFS= read -r line; do printf 'received:[%s]\\n❯\\n' \"$line\"; done"
                .into(),
        );
        registry.update(parent).unwrap();
        registry.start(1).unwrap();
        wait_for_state(&mut registry, 1, AttentionState::Idle);
        put_actor(&registry, "restarted-parent", 1);
        let outcome = TimerService::new(&mut registry)
            .set_idle(
                "restarted-parent".into(),
                1,
                "restart saw completion".into(),
                TimerKind::IdleAny,
                vec![2],
                10_000,
                now_millis(),
            )
            .unwrap();
        assert!(matches!(outcome, IdleTimerOutcome::AlreadySatisfied { .. }));
        wait_for_output(&mut registry, 1, "restart saw completion");
        registry.stop(1).unwrap();
        registry.stop(2).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn explicit_idle_timer_owned_by_spawner_wins_without_double_wake() {
        let mut registry = registry();
        registry
            .create(process(
                1,
                "parent",
                "stty -echo; printf '❯\\n'; while IFS= read -r line; do printf 'received:[%s]\\n❯\\n' \"$line\"; done",
            ))
            .unwrap();
        registry
            .create(process(
                2,
                "child",
                "printf '❯\\n'; while IFS= read -r line; do printf 'thinking...\\nesc to interrupt\\n'; sleep 0.5; printf 'done\\n❯\\n'; done",
            ))
            .unwrap();
        registry.start(1).unwrap();
        registry.start(2).unwrap();
        wait_for_state(&mut registry, 1, AttentionState::Idle);
        wait_for_state(&mut registry, 2, AttentionState::Idle);
        registry.set_notify_spawner_on_idle(1, 2, true).unwrap();
        put_actor(&registry, "timer-parent", 1);

        submit_as(&mut registry, 1, 2, b"go");
        wait_for_attention_snapshot(&registry, 2, AttentionState::Working);
        let outcome = TimerService::new(&mut registry)
            .set_idle(
                "timer-parent".into(),
                1,
                "TIMER-WAKE".into(),
                TimerKind::IdleAny,
                vec![2],
                60_000,
                now_millis(),
            )
            .unwrap();
        assert!(matches!(outcome, IdleTimerOutcome::Created(_)));
        wait_for_attention_snapshot(&registry, 2, AttentionState::Idle);

        let mut service = SpawnerNotificationService::default();
        service.tick(&mut registry).unwrap();
        assert!(service.pending.is_empty());
        assert!(
            !registry
                .rendered_output(1)
                .unwrap()
                .text
                .contains("[workman]")
        );

        let fired = TimerService::new(&mut registry).tick(now_millis()).unwrap();
        assert_eq!(fired.len(), 1);
        wait_for_output(&mut registry, 1, "TIMER-WAKE");
        service.tick(&mut registry).unwrap();
        let output = registry.rendered_output(1).unwrap().text;
        assert_eq!(output.matches("TIMER-WAKE").count(), 1);
        assert!(!output.contains("[workman] child idle:"), "{output:?}");
        registry.stop(1).unwrap();
        registry.stop(2).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn parked_child_is_not_finished_until_work_after_its_timer_wake_completes() {
        let mut registry = registry();
        registry
            .create(process(
                1,
                "parent",
                "stty -echo; printf '❯\\n'; while IFS= read -r line; do printf 'received:[%s]\\n❯\\n' \"$line\"; done",
            ))
            .unwrap();
        registry
            .create(process(
                2,
                "child",
                "printf '❯\\n'; while IFS= read -r line; do printf 'thinking...\\nesc to interrupt\\n'; sleep 0.3; printf 'done:%s\\n❯\\n' \"$line\"; done",
            ))
            .unwrap();
        registry.start(1).unwrap();
        registry.start(2).unwrap();
        wait_for_state(&mut registry, 1, AttentionState::Idle);
        wait_for_state(&mut registry, 2, AttentionState::Idle);
        registry.set_notify_spawner_on_idle(1, 2, true).unwrap();
        submit_as(&mut registry, 1, 2, b"first");
        wait_for_attention_snapshot(&registry, 2, AttentionState::Working);
        wait_for_attention_snapshot(&registry, 2, AttentionState::Idle);

        put_actor(&registry, "parked-child", 2);
        let armed_at = now_millis();
        TimerService::new(&mut registry)
            .set_delay(
                "parked-child".into(),
                2,
                "timer-work".into(),
                1_000,
                false,
                None,
                armed_at,
            )
            .unwrap();
        let mut service = SpawnerNotificationService::default();
        service.tick(&mut registry).unwrap();
        assert!(service.pending.is_empty());
        assert!(
            !registry
                .rendered_output(1)
                .unwrap()
                .text
                .contains("[workman]")
        );

        let fired = TimerService::new(&mut registry)
            .tick(armed_at + 1_001)
            .unwrap();
        assert_eq!(fired.len(), 1);
        wait_for_attention_snapshot(&registry, 2, AttentionState::Working);
        wait_for_attention_snapshot(&registry, 2, AttentionState::Idle);
        service.tick(&mut registry).unwrap();
        wait_for_output(&mut registry, 1, "child (2) finished");
        let output = registry.rendered_output(1).unwrap().text;
        assert_eq!(output.matches("[workman] child idle:").count(), 1);
        registry.stop(1).unwrap();
        registry.stop(2).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn opted_in_completion_is_reported_but_unnotified_child_still_satisfies_idle_any() {
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

        registry
            .store()
            .put_actor(&Actor {
                id: "parent-owner".into(),
                session_id: "parent-owner-session".into(),
                process_id: Some(1),
                selected_project_id: Some(1),
                created_at: 1_000,
                last_seen_at: 1_000,
            })
            .unwrap();
        let outcome = TimerService::new(&mut registry)
            .set_idle(
                "parent-owner".into(),
                1,
                "unnotified sibling completion".into(),
                TimerKind::IdleAny,
                vec![2, 3],
                10_000,
                now_millis(),
            )
            .unwrap();
        let IdleTimerOutcome::AlreadySatisfied { satisfied_by, .. } = outcome else {
            panic!("the unnotified child's completion did not satisfy idle_any");
        };
        assert_eq!(satisfied_by.len(), 1);
        assert_eq!(satisfied_by[0].process_id, Some(3));
        assert_eq!(
            satisfied_by[0].reason,
            TimerSatisfactionReason::UnseenCompletion
        );
        assert!(
            CompletionLedger::new(registry.store())
                .unreported_completions(1, &[2, 3])
                .unwrap()
                .is_empty()
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
