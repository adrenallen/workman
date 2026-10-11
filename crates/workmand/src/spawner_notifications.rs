//! Coalesced child-to-spawner prompt delivery.
//!
//! Delivery intentionally uses `ProcessRegistry::submit_input`, the same route as timers, so
//! bracketed paste, Enter separation, dialog guards, typing pauses, and process-local FIFO order
//! stay centralized. Completed-turn discovery and reporting are supplied by the completion ledger.

use std::{
    collections::{BTreeMap, HashSet},
    hash::{DefaultHasher, Hash, Hasher},
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
    attention::{AgentState, AttentionState, tool_detects_busy, tool_queues_input_while_working},
};

use crate::{
    ProcessRegistry, RegistryResult, SharedProcessRegistry,
    completion_ledger::{Completion, CompletionLedger},
    process_registry::NotificationTimerContext,
    status_invalidation::StatusInvalidationHub,
    timers::now_millis,
};

const NOTIFICATION_POLL_INTERVAL: Duration = Duration::from_millis(100);
const NEEDS_INPUT_RESET_CONFIRMATION_MS: i64 = 3_000;
const ERROR_LOG_INTERVAL_MS: i64 = 60_000;
const DELIVERY_HOLD_LOG_AFTER_MS: i64 = 60_000;
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
    completion_input_at: Option<i64>,
    dialog_identity: Option<u64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ObservationKey {
    state: AttentionState,
    last_input_at: Option<i64>,
    work_evidence_at: Option<i64>,
    pending_prompt: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DialogIdentityCache {
    last_content_change_at: Option<i64>,
    identity: Option<u64>,
}

#[derive(Clone, Debug)]
struct EligibleChild {
    process: Process,
    setting: SpawnerIdleNotificationSetting,
    attention: AgentState,
    effective_reported: SpawnerReportedState,
    waiting: bool,
    explicit_idle_timer: bool,
}

#[derive(Clone, Copy, Debug)]
struct DeliveryHold {
    started_at_ms: i64,
    last_logged_at_ms: Option<i64>,
}

impl DeliveryHold {
    fn log_age(&mut self, now_ms: i64) -> Option<i64> {
        let age_ms = now_ms.saturating_sub(self.started_at_ms);
        if age_ms >= DELIVERY_HOLD_LOG_AFTER_MS
            && self
                .last_logged_at_ms
                .is_none_or(|last| now_ms.saturating_sub(last) >= ERROR_LOG_INTERVAL_MS)
        {
            self.last_logged_at_ms = Some(now_ms);
            Some(age_ms)
        } else {
            None
        }
    }
}

#[derive(Default)]
pub(crate) struct SpawnerNotificationService {
    pending: BTreeMap<ProcessId, BTreeMap<ProcessId, PendingChildNotification>>,
    observations: BTreeMap<ProcessId, ObservationKey>,
    needs_input_clear_since: BTreeMap<ProcessId, i64>,
    reported_attention: BTreeMap<ProcessId, SpawnerReportedState>,
    reported_dialog_identity: BTreeMap<ProcessId, u64>,
    dialog_identity_cache: BTreeMap<ProcessId, DialogIdentityCache>,
    suppressed_completion_ids: BTreeMap<(ProcessId, ProcessId), i64>,
    reported_finished_inputs: BTreeMap<(ProcessId, ProcessId), i64>,
    draft_holds: HashSet<ProcessId>,
    delivery_holds: BTreeMap<ProcessId, DeliveryHold>,
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
        self.reported_dialog_identity
            .retain(|child_id, _| enabled.contains(child_id));
        self.dialog_identity_cache
            .retain(|child_id, _| enabled.contains(child_id));
        let mut eligible_children =
            BTreeMap::<ProcessId, BTreeMap<ProcessId, EligibleChild>>::new();
        let mut poll_again = !self.pending.is_empty();

        for setting in settings.iter().copied() {
            if let Err(error) = self.observe_child(
                registry,
                setting,
                now_ms,
                &timer_context,
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

        let active_spawners = self
            .pending
            .keys()
            .copied()
            .chain(settings.iter().filter_map(|setting| {
                registry
                    .store()
                    .get_process(setting.process_id)
                    .ok()
                    .flatten()
                    .and_then(|process| process.spawned_by_process_id)
            }))
            .collect::<HashSet<_>>();
        self.suppressed_completion_ids
            .retain(|(spawner_id, child_id), _| {
                active_spawners.contains(spawner_id) && enabled.contains(child_id)
            });
        self.reported_finished_inputs
            .retain(|(spawner_id, child_id), _| {
                active_spawners.contains(spawner_id) && enabled.contains(child_id)
            });
        self.last_error_log_at
            .retain(|_, logged_at| now_ms.saturating_sub(*logged_at) < ERROR_LOG_INTERVAL_MS);
        let stale_draft_holds = self
            .draft_holds
            .iter()
            .filter(|spawner_id| !self.pending.contains_key(spawner_id))
            .copied()
            .collect::<Vec<_>>();
        for spawner_id in stale_draft_holds {
            self.set_draft_hold(registry, spawner_id, false);
        }
        self.delivery_holds
            .retain(|spawner_id, _| self.pending.contains_key(spawner_id));

        self.deliver_ready(registry, now_ms);
        Ok(poll_again || !self.pending.is_empty())
    }

    fn observe_child(
        &mut self,
        registry: &mut ProcessRegistry,
        setting: SpawnerIdleNotificationSetting,
        now_ms: i64,
        timer_context: &NotificationTimerContext,
        eligible_children: &mut BTreeMap<ProcessId, BTreeMap<ProcessId, EligibleChild>>,
        poll_again: &mut bool,
    ) -> RegistryResult<()> {
        let Some(child) = registry.store().get_process(setting.process_id)? else {
            return Ok(());
        };
        let mut effective_reported = self
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
            self.remove_child(child.id);
            return Ok(());
        }
        if child.status == ProcessStatus::Stopped {
            return Ok(());
        }
        if terminal_state.is_none()
            && matches!(
                effective_reported,
                SpawnerReportedState::Exited | SpawnerReportedState::Crashed
            )
        {
            registry
                .store()
                .set_spawner_reported_state(child.id, SpawnerReportedState::Neutral)?;
            effective_reported = SpawnerReportedState::Neutral;
            self.reported_attention
                .insert(child.id, SpawnerReportedState::Neutral);
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
        let waiting = attention.state == AttentionState::Idle
            && timer_context.waiting_processes.contains(&child.id);
        let observed_state = if waiting {
            AttentionState::Waiting
        } else {
            attention.state
        };
        let explicit_idle_timer = timer_context
            .owned_idle_watches
            .contains(&(spawner_id, child.id));
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
                effective_reported,
                waiting,
                explicit_idle_timer,
            },
        );
        let dialog_identity = if observed_state == AttentionState::NeedsInput {
            self.dialog_identity(registry, child.id, &attention)?
        } else {
            None
        };
        self.observe_attention_reason(
            registry,
            spawner_id,
            &child,
            child_reason(child.status, observed_state).map(|reason| (reason, dialog_identity)),
            effective_reported,
            now_ms,
        );
        Ok(())
    }

    fn dialog_identity(
        &mut self,
        registry: &mut ProcessRegistry,
        child_id: ProcessId,
        attention: &AgentState,
    ) -> RegistryResult<Option<u64>> {
        let last_content_change_at = attention.last_content_change_at;
        if let Some(cached) = self.dialog_identity_cache.get(&child_id)
            && cached.last_content_change_at == last_content_change_at
        {
            return Ok(cached.identity);
        }

        let mut hasher = DefaultHasher::new();
        let identity = if let Some(dialog) = registry.pending_dialog_viewport(child_id)? {
            dialog.classification.hash(&mut hasher);
            dialog.rendered.hash(&mut hasher);
            Some(hasher.finish())
        } else {
            attention.classification.as_ref().map(|classification| {
                classification.hash(&mut hasher);
                hasher.finish()
            })
        };
        self.dialog_identity_cache.insert(
            child_id,
            DialogIdentityCache {
                last_content_change_at,
                identity,
            },
        );
        Ok(identity)
    }

    fn observe_attention_reason(
        &mut self,
        registry: &ProcessRegistry,
        spawner_id: ProcessId,
        child: &Process,
        observation: Option<(ChildNotificationReason, Option<u64>)>,
        effective_reported: SpawnerReportedState,
        now_ms: i64,
    ) {
        match observation {
            Some((reason, dialog_identity)) => {
                self.needs_input_clear_since.remove(&child.id);
                let changed_dialog = reason == ChildNotificationReason::NeedsInput
                    && effective_reported == SpawnerReportedState::NeedsInput
                    && dialog_identity.as_ref() != self.reported_dialog_identity.get(&child.id);
                if reason.reported_state() != Some(effective_reported) || changed_dialog {
                    self.queue(spawner_id, child, reason, dialog_identity);
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
                        self.reported_dialog_identity.remove(&child.id);
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
                .unwrap_or(0)
                .max(child.setting.suppressed_completion_id);
            if completion.id <= child.setting.baseline_completion_id || completion.id <= suppressed
            {
                continue;
            }
            let Some(input_at) =
                CompletionLedger::new(registry.store()).completion_input_at(completion.id)?
            else {
                continue;
            };
            let last_finished_input = self
                .reported_finished_inputs
                .get(&(spawner_id, completion.process_id))
                .copied()
                .or(child.setting.last_finished_input_at);
            if let Some(reported) = last_finished_input {
                let same_input_without_new_busy_episode = input_at == reported
                    && !tool_detects_busy(child.attention.tool_type.as_deref());
                if input_at < reported || same_input_without_new_busy_episode {
                    continue;
                }
            }
            if child.explicit_idle_timer {
                continue;
            }
            if child.waiting {
                self.suppress_completion(registry, spawner_id, completion, now_ms);
                continue;
            }
            current_completions.insert(completion.process_id, (completion, input_at));
        }

        // Pending finished entries are a projection of the ledger, not an append-only queue.
        // Re-prompts move the owner's input baseline and explicit timers may consume a
        // completion, so remove anything the current unreported query no longer returns.
        let remove_spawner = self.pending.get_mut(&spawner_id).is_some_and(|pending| {
            pending.retain(|child_id, notification| {
                if !children.contains_key(child_id) {
                    return true;
                }
                let current = current_completions.get(child_id).copied();
                notification.completion = current.map(|(completion, _)| completion);
                notification.completion_input_at = current.map(|(_, input_at)| input_at);
                notification.completion.is_some()
                    || notification.reason != ChildNotificationReason::Finished
            });
            pending.is_empty()
        });
        if remove_spawner {
            self.pending.remove(&spawner_id);
        }

        for (completion, input_at) in current_completions.into_values() {
            let child = &children[&completion.process_id];
            if child.attention.state != AttentionState::NeedsInput {
                self.needs_input_clear_since.remove(&completion.process_id);
                self.reported_dialog_identity.remove(&completion.process_id);
                if child.effective_reported != SpawnerReportedState::Neutral {
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
            }
            self.queue_completion(
                spawner_id,
                &child.process,
                completion,
                input_at,
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
        completion_input_at: i64,
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
                    pending.completion_input_at = Some(completion_input_at);
                }
                if attention_state != AttentionState::NeedsInput
                    && !matches!(
                        pending.reason,
                        ChildNotificationReason::Exited | ChildNotificationReason::Crashed
                    )
                {
                    pending.reason = ChildNotificationReason::Finished;
                }
            })
            .or_insert_with(|| PendingChildNotification {
                child_process_id: child.id,
                child_name: single_line_name(&child.name),
                child_project_id: child.project_id,
                reason: ChildNotificationReason::Finished,
                completion: Some(completion),
                completion_input_at: Some(completion_input_at),
                dialog_identity: None,
            });
    }

    fn queue(
        &mut self,
        spawner_id: ProcessId,
        child: &Process,
        reason: ChildNotificationReason,
        dialog_identity: Option<u64>,
    ) {
        self.pending
            .entry(spawner_id)
            .or_default()
            .entry(child.id)
            .and_modify(|pending| {
                pending.child_name = single_line_name(&child.name);
                pending.child_project_id = child.project_id;
                pending.reason = reason;
                pending.dialog_identity = dialog_identity;
            })
            .or_insert_with(|| PendingChildNotification {
                child_process_id: child.id,
                child_name: single_line_name(&child.name),
                child_project_id: child.project_id,
                reason,
                completion: None,
                completion_input_at: None,
                dialog_identity,
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
            self.set_draft_hold(registry, spawner_id, false);
            self.hold_delivery(spawner_id, "spawner is starting", now_ms);
            return Ok(());
        }
        let has_human_draft = registry.input_router().has_unsent_human_draft(spawner_id)?;
        let attention = registry.agent_attention_snapshot(spawner_id)?;
        let hold_reason = if attention.state == AttentionState::NeedsInput
            || registry.pending_dialog_viewport(spawner_id)?.is_some()
        {
            Some("spawner needs input (permission or trust dialog)")
        } else if registry.has_pending_prompts(spawner_id) {
            // Includes the reservation made before asynchronous initial-prompt delivery.
            Some("spawner has a pending prompt")
        } else if registry
            .input_router()
            .automatic_submission_held(spawner_id)?
        {
            Some("automatic submission is paused by typing")
        } else {
            match attention.state {
                AttentionState::Idle | AttentionState::Waiting => None,
                AttentionState::Working if attention.last_input_at.is_none() => {
                    Some("spawner is starting before its first prompt")
                }
                AttentionState::Working
                    if tool_queues_input_while_working(attention.tool_type.as_deref()) =>
                {
                    None
                }
                AttentionState::Working => Some("tool has no verified mid-turn input queue"),
                _ => Some("spawner is not ready"),
            }
        };
        self.set_draft_hold(
            registry,
            spawner_id,
            hold_reason.is_none() && has_human_draft,
        );
        let hold_reason =
            hold_reason.or(has_human_draft.then_some("spawner has an unsent human draft"));
        if let Some(reason) = hold_reason {
            self.hold_delivery(spawner_id, reason, now_ms);
            return Ok(());
        }

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
                now_ms,
            )? {
                NotificationRelevance::Deliver => deliverable.push(child.clone()),
                NotificationRelevance::Hold => {}
                NotificationRelevance::Drop => {
                    self.remove_pending_child(spawner_id, child.child_process_id);
                }
            }
        }
        if deliverable.is_empty() {
            if self.pending.contains_key(&spawner_id) {
                self.hold_delivery(spawner_id, "child notification is not yet relevant", now_ms);
            }
            return Ok(());
        }
        let body = notification_body(deliverable.iter());
        registry.submit_input(spawner_id, body.as_bytes())?;
        self.delivery_holds.remove(&spawner_id);
        registry
            .input_router()
            .clear_human_draft_activity(spawner_id);

        // The PTY side effect succeeded. Suppress and clear this exact batch before every
        // best-effort bookkeeping step so a database error can never cause a duplicate turn.
        for child in &deliverable {
            if let Some(completion) = child.completion {
                if attention.state == AttentionState::Working {
                    registry
                        .mid_turn_deliveries
                        .insert((spawner_id, child.child_process_id), completion.id);
                }
                self.suppress_completion(registry, spawner_id, completion, now_ms);
            }
            if child.reason == ChildNotificationReason::Finished
                && let Some(input_at) = child.completion_input_at
            {
                self.reported_finished_inputs
                    .entry((spawner_id, child.child_process_id))
                    .and_modify(|current| *current = (*current).max(input_at))
                    .or_insert(input_at);
                if let Err(error) = registry
                    .store()
                    .advance_spawner_finished_input(child.child_process_id, input_at)
                {
                    self.log_error(
                        format!("finished-input:{}", child.child_process_id),
                        now_ms,
                        format!(
                            "process {}: delivered finished notification but could not persist its input boundary: {error}",
                            child.child_process_id
                        ),
                    );
                }
            }
            if let Some(state) = child.reason.reported_state() {
                self.reported_attention
                    .insert(child.child_process_id, state);
                if state == SpawnerReportedState::NeedsInput {
                    if let Some(identity) = child.dialog_identity {
                        self.reported_dialog_identity
                            .insert(child.child_process_id, identity);
                    }
                } else {
                    self.reported_dialog_identity
                        .remove(&child.child_process_id);
                }
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
        now_ms: i64,
    ) -> RegistryResult<NotificationRelevance> {
        let Some(process) = registry.store().get_process(pending.child_process_id)? else {
            return Ok(NotificationRelevance::Drop);
        };
        match pending.reason {
            ChildNotificationReason::Finished => {
                if owned_idle_watches.contains(&(spawner_id, process.id)) {
                    return Ok(NotificationRelevance::Drop);
                }
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
                    self.suppress_completion(registry, spawner_id, completion, now_ms);
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
        self.set_draft_hold(registry, spawner_id, false);
        self.delivery_holds.remove(&spawner_id);
        for pending in dropped.values() {
            if let Some(completion) = pending.completion {
                self.suppress_completion(registry, spawner_id, completion, now_ms);
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
        self.reported_dialog_identity.remove(&child_id);
        self.dialog_identity_cache.remove(&child_id);
        self.suppressed_completion_ids
            .retain(|(_, candidate), _| *candidate != child_id);
        self.reported_finished_inputs
            .retain(|(_, candidate), _| *candidate != child_id);
    }

    fn suppress_completion(
        &mut self,
        registry: &ProcessRegistry,
        spawner_id: ProcessId,
        completion: Completion,
        now_ms: i64,
    ) {
        self.suppressed_completion_ids
            .entry((spawner_id, completion.process_id))
            .and_modify(|current| *current = (*current).max(completion.id))
            .or_insert(completion.id);
        if let Err(error) = registry
            .store()
            .advance_spawner_suppressed_completion(completion.process_id, completion.id)
        {
            self.log_error(
                format!("completion-suppress:{}", completion.process_id),
                now_ms,
                format!(
                    "process {}: could not persist the child-notification completion watermark: {error}",
                    completion.process_id
                ),
            );
        }
    }

    fn set_draft_hold(&mut self, registry: &ProcessRegistry, spawner_id: ProcessId, held: bool) {
        if registry
            .input_router()
            .set_notification_held_by_draft(spawner_id, held)
            .unwrap_or(false)
        {
            registry.status_invalidations().invalidate();
        }
        if held {
            self.draft_holds.insert(spawner_id);
        } else {
            self.draft_holds.remove(&spawner_id);
        }
    }

    fn remove_pending_child(&mut self, spawner_id: ProcessId, child_id: ProcessId) {
        let remove_spawner = self.pending.get_mut(&spawner_id).is_some_and(|children| {
            children.remove(&child_id);
            children.is_empty()
        });
        if remove_spawner {
            self.pending.remove(&spawner_id);
            self.delivery_holds.remove(&spawner_id);
        }
    }

    fn hold_delivery(&mut self, spawner_id: ProcessId, reason: &str, now_ms: i64) {
        let hold = self
            .delivery_holds
            .entry(spawner_id)
            .or_insert(DeliveryHold {
                started_at_ms: now_ms,
                last_logged_at_ms: None,
            });
        if let Some(age_ms) = hold.log_age(now_ms) {
            eprintln!(
                "process {spawner_id}: child notification held for {:.1}s: {reason}",
                age_ms as f64 / 1_000.0,
            );
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
        store
            .put_agent_tool(&AgentTool {
                id: 91,
                name: "Generic test agent".into(),
                command: "generic-test-agent".into(),
                tool_type: "generic_test_agent".into(),
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
            10,
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
            10,
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
            10,
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
                None,
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

    #[test]
    fn delivery_hold_logs_after_one_minute_and_rate_limits_repeats() {
        let mut hold = DeliveryHold {
            started_at_ms: 1_000,
            last_logged_at_ms: None,
        };
        assert_eq!(hold.log_age(60_999), None);
        assert_eq!(hold.log_age(61_000), Some(60_000));
        assert_eq!(hold.log_age(61_100), None);
        assert_eq!(hold.log_age(120_999), None);
        assert_eq!(hold.log_age(121_000), Some(120_000));
    }

    #[cfg(unix)]
    #[test]
    fn spawner_dialog_holds_until_cleared_even_for_queue_capable_tool() {
        let mut registry = registry();
        registry.create(process(1, "parent",
            "stty -echo; printf '❯\\n'; IFS= read -r show; printf 'Do you want to proceed?\\n❯ 1. Yes, allow\\n  2. No, and tell Claude\\n'; IFS= read -r answer; printf '\\033[2J\\033[H❯\\n'; while IFS= read -r line; do printf 'received:[%s]\\n❯\\n' \"$line\"; done")).unwrap();
        registry.create(process(2, "child", "exit 0")).unwrap();
        registry.start(1).unwrap();
        registry.start(2).unwrap();
        registry.set_notify_spawner_on_idle(1, 2, true).unwrap();
        wait_for_state(&mut registry, 1, AttentionState::Idle);
        registry.submit_input(1, b"show dialog").unwrap();
        wait_for_output(&mut registry, 1, "Do you want to proceed?");
        let mut service = SpawnerNotificationService::default();
        wait_for_state(&mut registry, 2, AttentionState::Exited);
        service.tick_at(&mut registry, 1_000).unwrap();
        assert!(
            service.pending.contains_key(&1),
            "rendered dialog must hold even during input grace"
        );
        wait_for_state(&mut registry, 1, AttentionState::NeedsInput);
        service.tick_at(&mut registry, 61_000).unwrap();
        assert!(service.pending.contains_key(&1));
        assert_eq!(service.delivery_holds[&1].last_logged_at_ms, Some(61_000));
        assert!(
            !registry
                .rendered_output(1)
                .unwrap()
                .text
                .contains("[workman]")
        );
        registry.input_router().send_input(1, b"clear\n").unwrap();
        wait_for_state(&mut registry, 1, AttentionState::Idle);
        service.tick(&mut registry).unwrap();
        wait_for_output(&mut registry, 1, "child (2) exited");
        assert!(service.pending.is_empty());
        assert!(service.delivery_holds.is_empty());
        registry.stop(1).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn working_unverified_tool_holds_until_idle() {
        let mut registry = registry();
        let mut parent = process(
            1,
            "parent",
            "stty -echo; printf '>\\n'; while IFS= read -r line; do printf 'received:[%s]\\n>\\n' \"$line\"; done",
        );
        parent.agent_tool_id = Some(91);
        registry.create(parent).unwrap();
        registry.create(process(2, "child", "exit 0")).unwrap();
        registry.start(1).unwrap();
        registry.start(2).unwrap();
        registry.set_notify_spawner_on_idle(1, 2, true).unwrap();
        wait_for_state(&mut registry, 2, AttentionState::Exited);
        registry.submit_input(1, b"work").unwrap();
        wait_for_output(&mut registry, 1, "received:[work]");
        assert_eq!(
            registry.agent_attention_snapshot(1).unwrap().state,
            AttentionState::Working
        );
        let mut service = SpawnerNotificationService::default();
        service.tick(&mut registry).unwrap();
        assert!(service.pending.contains_key(&1));
        assert!(
            !registry
                .rendered_output(1)
                .unwrap()
                .text
                .contains("[workman]")
        );
        wait_for_state(&mut registry, 1, AttentionState::Idle);
        service.tick(&mut registry).unwrap();
        wait_for_output(&mut registry, 1, "child (2) exited");
        registry.stop(1).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn startup_and_reserved_initial_prompt_hold_notification() {
        let mut registry = registry();
        registry.create(process(1, "parent",
            "stty -echo; printf '❯\\n'; while IFS= read -r line; do printf 'received:[%s]\\n❯\\n' \"$line\"; done")).unwrap();
        registry.create(process(2, "child", "exit 0")).unwrap();
        registry.start(1).unwrap();
        registry.start(2).unwrap();
        registry.set_notify_spawner_on_idle(1, 2, true).unwrap();
        wait_for_state(&mut registry, 2, AttentionState::Exited);
        let mut service = SpawnerNotificationService::default();
        wait_for_output(&mut registry, 1, "❯");
        assert_eq!(
            registry.agent_attention_snapshot(1).unwrap().state,
            AttentionState::Working
        );
        service.tick(&mut registry).unwrap();
        assert!(service.pending.contains_key(&1));
        assert!(
            !registry
                .rendered_output(1)
                .unwrap()
                .text
                .contains("[workman]")
        );
        let initial = registry.reserve_prompt(1).unwrap();
        wait_for_state(&mut registry, 1, AttentionState::Idle);
        service.tick(&mut registry).unwrap();
        assert!(service.pending.contains_key(&1));
        registry.submit_initial_prompt(1, b"initial").unwrap();
        wait_for_output(&mut registry, 1, "received:[initial]");
        drop(initial);
        let deadline = Instant::now() + Duration::from_secs(3);
        while registry.has_pending_prompts(1) {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(10));
        }
        service.tick(&mut registry).unwrap();
        wait_for_output(&mut registry, 1, "child (2) exited");
        let output = registry.rendered_output(1).unwrap().text;
        assert!(output.find("received:[initial]").unwrap() < output.find("[workman]").unwrap());
        registry.stop(1).unwrap();
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
    fn finished_is_delivered_once_per_spawner_child_input() {
        let mut registry = registry();
        registry
            .create(process(
                1,
                "parent",
                "stty -echo; printf '❯\\n'; while IFS= read -r line; do printf 'received:[%s]\\n❯\\n' \"$line\"; done",
            ))
            .unwrap();
        let mut generic_child = process(
            2,
            "generic-child",
            "printf '❯\\n'; while IFS= read -r line; do :; done",
        );
        generic_child.agent_tool_id = Some(91);
        registry.create(generic_child).unwrap();
        registry.start(1).unwrap();
        registry.start(2).unwrap();
        wait_for_state(&mut registry, 1, AttentionState::Idle);
        wait_for_state(&mut registry, 2, AttentionState::Idle);
        registry.set_notify_spawner_on_idle(1, 2, true).unwrap();

        CompletionLedger::new(registry.store())
            .record_input(1, 2, 100)
            .unwrap();
        registry
            .store()
            .connection()
            .execute(
                "INSERT INTO process_completions
                    (process_id, input_at, work_evidence_at, completed_at)
                 VALUES (2, 100, 200, 300)",
                [],
            )
            .unwrap();
        let mut service = SpawnerNotificationService::default();
        service.tick_at(&mut registry, 400).unwrap();
        wait_for_output(&mut registry, 1, "generic-child (2) finished");
        wait_for_state(&mut registry, 1, AttentionState::Idle);

        registry
            .store()
            .connection()
            .execute(
                "INSERT INTO process_completions
                    (process_id, input_at, work_evidence_at, completed_at)
                 VALUES (2, 100, 500, 600)",
                [],
            )
            .unwrap();
        service.tick_at(&mut registry, 700).unwrap();
        assert!(service.pending.is_empty());
        let output = registry.rendered_output(1).unwrap().text;
        assert_eq!(output.matches("generic-child (2) finished").count(), 1);
        assert_eq!(
            registry
                .store()
                .spawner_idle_notification_setting(2)
                .unwrap()
                .unwrap()
                .last_finished_input_at,
            Some(100)
        );

        registry.stop(1).unwrap();
        registry.stop(2).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn busy_adapter_can_report_a_later_completion_for_the_same_input() {
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
                "busy-child",
                "printf '❯\\n'; while IFS= read -r line; do :; done",
            ))
            .unwrap();
        registry.start(1).unwrap();
        registry.start(2).unwrap();
        wait_for_state(&mut registry, 1, AttentionState::Idle);
        wait_for_state(&mut registry, 2, AttentionState::Idle);
        registry.set_notify_spawner_on_idle(1, 2, true).unwrap();

        CompletionLedger::new(registry.store())
            .record_input(1, 2, 100)
            .unwrap();
        let insert_completion =
            |registry: &ProcessRegistry, evidence_at: i64, completed_at: i64| {
                registry
                    .store()
                    .connection()
                    .execute(
                        "INSERT INTO process_completions
                        (process_id, input_at, work_evidence_at, completed_at)
                     VALUES (2, 100, ?1, ?2)",
                        (evidence_at, completed_at),
                    )
                    .unwrap();
            };
        insert_completion(&registry, 200, 300);
        let mut service = SpawnerNotificationService::default();
        service.tick_at(&mut registry, 400).unwrap();
        wait_for_output(&mut registry, 1, "busy-child (2) finished");
        wait_for_state(&mut registry, 1, AttentionState::Idle);

        insert_completion(&registry, 500, 600);
        service.tick_at(&mut registry, 700).unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let output = registry.rendered_output(1).unwrap().text;
            if output.matches("busy-child (2) finished").count() == 2 {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "second completion was hidden: {output:?}"
            );
            thread::sleep(Duration::from_millis(10));
        }

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
                r#"true claude; stty raw -echo; printf '❯ '; exec perl -e '$|=1; my $draft=""; while (1) { my $n=sysread(STDIN,my $chunk,4096); exit 2 unless defined($n) && $n>0; my $redraw=0; for my $c (split //,$chunk) { if ($c eq "\r") { print "\r\nreceived:[$draft]\r\n❯ "; $draft=""; next; } if (ord($c)==21) { $draft=""; print "\r\e[2K❯ "; next; } $draft.=$c; $redraw=1; } print "\r\e[2K❯ $draft" if $redraw; }'"#,
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
        assert!(registry.get_status(1).unwrap().notification_held_by_draft);
        let changes = registry.store().connection().total_changes();
        for _ in 0..20 {
            service.tick(&mut registry).unwrap();
        }
        assert_eq!(
            registry.store().connection().total_changes(),
            changes,
            "a held completion must not write on every notification tick"
        );

        registry
            .input_router()
            .send_terminal_input(1, b"\x15", true)
            .unwrap();
        let clear_deadline = Instant::now() + Duration::from_secs(2);
        loop {
            service.tick(&mut registry).unwrap();
            if !registry.get_status(1).unwrap().notification_held_by_draft {
                break;
            }
            assert!(Instant::now() < clear_deadline, "draft hold did not clear");
            thread::sleep(Duration::from_millis(10));
        }
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
                r#"true claude; stty raw -echo; printf '❯ '; exec perl -e '$|=1; my $draft=""; while (1) { my $n=sysread(STDIN,my $chunk,4096); exit 2 unless defined($n) && $n>0; my $redraw=0; for my $c (split //,$chunk) { if ($c eq "\r") { print "\r\nreceived:[$draft]\r\n❯ "; $draft=""; next; } if (ord($c)==21) { $draft=""; print "\r\e[2K❯ "; next; } $draft.=$c; $redraw=1; } print "\r\e[2K❯ $draft" if $redraw; }'"#,
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
    fn changed_dialog_is_a_new_needs_input_episode_inside_hysteresis() {
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
                "printf 'Do you want to proceed?\\n❯ 1. Yes, allow\\n  2. No, and tell Claude\\n'; IFS= read -r line || exit; printf '\\033[2J\\033[Hmake this edit to b.rs?\\n❯ 1. Yes, allow\\n  2. No, and tell Claude\\n'; sleep 30",
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

        registry.submit_dialog_response(2, b"yes").unwrap();
        wait_for_state(&mut registry, 2, AttentionState::Working);
        wait_for_state(&mut registry, 2, AttentionState::NeedsInput);
        service.tick_at(&mut registry, base + 1_000).unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let output =
                String::from_utf8_lossy(&registry.raw_output(1, None, 64 * 1024).unwrap().data)
                    .into_owned();
            if output.matches("dialog-child (2) needs input").count() == 2 {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "changed dialog was not announced: {output:?}"
            );
            thread::sleep(Duration::from_millis(10));
        }

        registry.stop(1).unwrap();
        registry.stop(2).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn dialog_status_repaints_for_virtual_thirty_seconds_announce_once() {
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
                "printf '\\033[2J\\033[Hstatus:0\\nDo you want to proceed?\\n❯ 1. Yes, allow\\n  2. No, and tell Claude\\n'; sleep 0.3; i=0; while [ $i -lt 10 ]; do i=$((i + 1)); printf '\\033[1;1H\\033[2Kstatus:%s' \"$i\"; sleep 0.12; done; sleep 30",
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

        let deadline = Instant::now() + Duration::from_secs(4);
        let mut tick = 1;
        loop {
            service
                .tick_at(&mut registry, base + i64::from(tick) * 3_000)
                .unwrap();
            let child_output = registry.rendered_output(2).unwrap().text;
            if child_output.contains("status:10") {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "repaint fixture stalled: {child_output:?}"
            );
            tick += 1;
            thread::sleep(Duration::from_millis(20));
        }
        wait_for_state(&mut registry, 1, AttentionState::Idle);
        service.tick_at(&mut registry, base + 33_000).unwrap();
        let output =
            String::from_utf8_lossy(&registry.raw_output(1, None, 64 * 1024).unwrap().data)
                .into_owned();
        assert_eq!(output.matches("dialog-child (2) needs input").count(), 1);

        registry.stop(1).unwrap();
        registry.stop(2).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn terminal_report_keeps_opt_in_across_child_restart() {
        let mut registry = registry();
        registry
            .create(process(
                1,
                "parent",
                "stty -echo; printf '❯\\n'; while IFS= read -r line; do printf 'received:[%s]\\n❯\\n' \"$line\"; done",
            ))
            .unwrap();
        registry
            .create(process(2, "child", "printf 'first run exits\\n'; exit 0"))
            .unwrap();
        registry.start(1).unwrap();
        registry.start(2).unwrap();
        registry.set_notify_spawner_on_idle(1, 2, true).unwrap();
        wait_for_state(&mut registry, 1, AttentionState::Idle);
        wait_for_state(&mut registry, 2, AttentionState::Exited);

        let mut service = SpawnerNotificationService::default();
        service.tick(&mut registry).unwrap();
        wait_for_output(&mut registry, 1, "child (2) exited");
        wait_for_state(&mut registry, 1, AttentionState::Idle);
        assert!(
            registry
                .store()
                .spawner_idle_notification_enabled(2)
                .unwrap()
        );

        let mut restarted = registry.get(2).unwrap();
        restarted.command = Some(
            "printf '❯\\n'; while IFS= read -r line; do printf 'thinking...\\nesc to interrupt\\n'; sleep 0.2; printf 'done\\n❯\\n'; done"
                .into(),
        );
        registry.update(restarted).unwrap();
        registry.start(2).unwrap();
        wait_for_state(&mut registry, 2, AttentionState::Idle);
        service.tick(&mut registry).unwrap();
        submit_as(&mut registry, 1, 2, b"after-restart");
        wait_for_attention_snapshot(&registry, 2, AttentionState::Working);
        wait_for_attention_snapshot(&registry, 2, AttentionState::Idle);
        service.tick(&mut registry).unwrap();
        wait_for_output(&mut registry, 1, "child (2) finished");
        let output = registry.rendered_output(1).unwrap().text;
        assert!(output.contains("child (2) exited"));
        assert!(output.contains("child (2) finished"));

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
        let mut restarted_service = SpawnerNotificationService::default();
        restarted_service.tick(&mut registry).unwrap();
        assert!(restarted_service.pending.is_empty());
        assert!(
            !registry
                .rendered_output(1)
                .unwrap()
                .text
                .contains("[workman] child idle:"),
            "the persisted dropped-completion watermark must survive daemon service restart"
        );
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
                "stty -echo; printf '❯\\n'; while IFS= read -r line; do printf 'received:[%s]\\nthinking...\\nesc to interrupt\\n' \"$line\"; done",
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
        submit_as(&mut registry, 1, 1, b"keep working");
        wait_for_state(&mut registry, 1, AttentionState::Working);

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

        assert_eq!(
            registry.agent_attention_snapshot(1).unwrap().state,
            AttentionState::Working
        );
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
    fn explicit_idle_timer_suppresses_only_finished_not_needs_input() {
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
                "printf 'Do you want to proceed?\\n❯ 1. Yes, allow\\n  2. No, and tell Claude\\n'; sleep 30",
            ))
            .unwrap();
        registry.start(1).unwrap();
        registry.start(2).unwrap();
        registry.set_notify_spawner_on_idle(1, 2, true).unwrap();
        wait_for_state(&mut registry, 1, AttentionState::Idle);
        wait_for_state(&mut registry, 2, AttentionState::NeedsInput);
        put_actor(&registry, "timer-parent-needs-input", 1);
        let outcome = TimerService::new(&mut registry)
            .set_idle(
                "timer-parent-needs-input".into(),
                1,
                "timer deadline".into(),
                TimerKind::IdleAny,
                vec![2],
                60_000,
                now_millis(),
            )
            .unwrap();
        let timer_id = match outcome {
            IdleTimerOutcome::Created(timer) => timer.timer.id,
            other => panic!("unexpected timer outcome: {other:?}"),
        };

        let mut service = SpawnerNotificationService::default();
        service.tick(&mut registry).unwrap();
        wait_for_output(&mut registry, 1, "dialog-child (2) needs input");
        TimerService::new(&mut registry)
            .cancel("timer-parent-needs-input", 1, timer_id)
            .unwrap();
        registry.stop(1).unwrap();
        registry.stop(2).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn paused_idle_timer_does_not_suppress_finished_notification() {
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
                "printf '❯\\n'; while IFS= read -r line; do printf 'thinking...\\nesc to interrupt\\n'; sleep 0.3; printf 'done\\n❯\\n'; done",
            ))
            .unwrap();
        registry.start(1).unwrap();
        registry.start(2).unwrap();
        registry.set_notify_spawner_on_idle(1, 2, true).unwrap();
        wait_for_state(&mut registry, 1, AttentionState::Idle);
        wait_for_state(&mut registry, 2, AttentionState::Idle);
        put_actor(&registry, "timer-parent-paused", 1);
        submit_as(&mut registry, 1, 2, b"go");
        wait_for_attention_snapshot(&registry, 2, AttentionState::Working);
        let outcome = TimerService::new(&mut registry)
            .set_idle(
                "timer-parent-paused".into(),
                1,
                "paused timer".into(),
                TimerKind::IdleAny,
                vec![2],
                60_000,
                now_millis(),
            )
            .unwrap();
        let timer_id = match outcome {
            IdleTimerOutcome::Created(timer) => timer.timer.id,
            other => panic!("unexpected timer outcome: {other:?}"),
        };
        TimerService::new(&mut registry)
            .pause("timer-parent-paused", 1, timer_id, now_millis())
            .unwrap();
        wait_for_attention_snapshot(&registry, 2, AttentionState::Idle);

        let mut service = SpawnerNotificationService::default();
        service.tick(&mut registry).unwrap();
        wait_for_output(&mut registry, 1, "child (2) finished");
        TimerService::new(&mut registry)
            .cancel("timer-parent-paused", 1, timer_id)
            .unwrap();
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
                "stty -echo; printf '❯\\n'; while IFS= read -r line; do printf 'received:[%s]\\nthinking...\\nesc to interrupt\\n' \"$line\"; sleep 0.1; printf '❯\\n'; done",
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
    fn check_mid_turn_notices(child_count: i64, late_owner_end: bool) {
        let mut registry = registry();
        registry.create(process(1, "parent", "stty -echo; printf '❯\\n'; while IFS= read -r line; do printf 'received:[%s]\\nthinking...\\nesc to interrupt\\n' \"$line\"; if [ \"$line\" = end ]; then sleep 0.1; printf '❯\\n'; fi; done")).unwrap();
        registry.create(process(2, "child", "printf '❯\\n'; while IFS= read -r line; do printf 'thinking...\\nesc to interrupt\\n'; sleep 0.1; printf 'done\\n❯\\n'; done")).unwrap();
        if child_count == 2 {
            registry.create(process(3, "second-child", "printf '❯\\n'; while IFS= read -r line; do printf 'thinking...\\nesc to interrupt\\n'; sleep 0.1; printf 'done\\n❯\\n'; done")).unwrap();
        }
        for id in 1..=child_count + 1 {
            registry.start(id).unwrap();
            wait_for_state(&mut registry, id, AttentionState::Idle);
        }
        registry.set_notify_spawner_on_idle(1, 2, true).unwrap();
        put_actor(&registry, "mid-turn-parent", 1);
        submit_as(&mut registry, 1, 1, b"hold");
        wait_for_state(&mut registry, 1, AttentionState::Working);
        submit_as(&mut registry, 1, 2, b"go");
        wait_for_state(&mut registry, 2, AttentionState::Working);
        wait_for_state(&mut registry, 2, AttentionState::Idle);
        if late_owner_end {
            let child_done = CompletionLedger::new(registry.store())
                .latest_completion(2)
                .unwrap()
                .unwrap()
                .completed_at_ms;
            registry.submit_input(1, b"end").unwrap();
            wait_for_state(&mut registry, 1, AttentionState::Idle);
            let owner_end = CompletionLedger::new(registry.store())
                .latest_completion(1)
                .unwrap()
                .unwrap()
                .completed_at_ms;
            assert!(owner_end > child_done);
            submit_as(&mut registry, 1, 1, b"hold");
            wait_for_state(&mut registry, 1, AttentionState::Working);
        }
        let mut service = SpawnerNotificationService::default();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            service.tick(&mut registry).unwrap();
            if registry
                .rendered_output(1)
                .unwrap()
                .text
                .contains("child (2) finished")
            {
                break;
            }
            assert!(Instant::now() < deadline, "mid-turn notice did not arrive");
            thread::sleep(Duration::from_millis(20));
        }
        assert!(
            CompletionLedger::new(registry.store())
                .unreported_completions(1, &[2])
                .unwrap()
                .is_empty()
        );
        if child_count == 2 {
            // A later tick queues a second notice, advancing the owner's last_input_at.
            let first_input = registry.agent_attention_snapshot(1).unwrap().last_input_at;
            registry.set_notify_spawner_on_idle(1, 3, true).unwrap();
            submit_as(&mut registry, 1, 3, b"go");
            wait_for_state(&mut registry, 3, AttentionState::Working);
            wait_for_state(&mut registry, 3, AttentionState::Idle);
            service.tick(&mut registry).unwrap();
            wait_for_output(&mut registry, 1, "second-child (3) finished");
            assert!(registry.agent_attention_snapshot(1).unwrap().last_input_at > first_input);
        }
        let now = now_millis();
        let outcome = TimerService::new(&mut registry)
            .set_idle(
                "mid-turn-parent".into(),
                1,
                "TIMER-WAKE".into(),
                TimerKind::IdleAny,
                vec![2],
                60_000,
                now,
            )
            .unwrap();
        let IdleTimerOutcome::AlreadySatisfied { satisfied_by, .. } = outcome else {
            panic!("queued completion must satisfy the watch immediately");
        };
        assert_eq!(satisfied_by.len(), 1);
        assert_eq!(satisfied_by[0].process_id, Some(2));
        assert_eq!(
            satisfied_by[0].reason,
            TimerSatisfactionReason::QueuedNotice
        );
        assert!(!registry.mid_turn_deliveries.contains_key(&(1, 2)));
        wait_for_output(&mut registry, 1, "received:[TIMER-WAKE]");
        assert!(
            TimerService::new(&mut registry)
                .tick(now + 60_001)
                .unwrap()
                .is_empty()
        );
        let deadline = Instant::now() + Duration::from_millis(600);
        while Instant::now() < deadline {
            service.tick(&mut registry).unwrap();
            thread::sleep(Duration::from_millis(20));
        }
        let output =
            String::from_utf8_lossy(&registry.raw_output(1, None, 64 * 1024).unwrap().data)
                .into_owned();
        assert_eq!(output.matches("received:[TIMER-WAKE]").count(), 1);

        let reprompt_child = if child_count == 2 { 3 } else { 2 };
        if child_count == 2 {
            assert!(registry.mid_turn_deliveries.contains_key(&(1, 3)));
        }
        // A child re-prompt invalidates its reported completion even if typing
        // pause leaves its previous idle frame visible while the body is queued.
        registry
            .input_router()
            .set_typing_pause(crate::settings::TypingPauseSettings {
                enabled: true,
                delay_ms: 60_000,
            });
        registry
            .input_router()
            .send_terminal_input(reprompt_child, b"", true)
            .unwrap();
        submit_as(&mut registry, 1, reprompt_child, b"next turn");
        assert!(registry.has_pending_prompts(reprompt_child));
        let outcome = TimerService::new(&mut registry)
            .set_idle(
                "mid-turn-parent".into(),
                1,
                "WAIT-FOR-NEW-CHILD-TURN".into(),
                TimerKind::IdleAny,
                vec![reprompt_child],
                60_000,
                now_millis(),
            )
            .unwrap();
        let IdleTimerOutcome::Created(timer) = outcome else {
            panic!("a re-prompted child's previous completion must not satisfy a watch");
        };
        assert!(
            !registry
                .mid_turn_deliveries
                .contains_key(&(1, reprompt_child))
        );
        TimerService::new(&mut registry)
            .cancel("mid-turn-parent", 1, timer.timer.id)
            .unwrap();
        registry
            .input_router()
            .set_typing_pause(crate::settings::TypingPauseSettings {
                enabled: false,
                delay_ms: 1000,
            });
        wait_for_state(&mut registry, reprompt_child, AttentionState::Working);
        wait_for_state(&mut registry, reprompt_child, AttentionState::Idle);
        // Consume the new episode so another watch has no eligible completion.
        let ledger = CompletionLedger::new(registry.store());
        ledger
            .mark_completions_reported(1, ledger.latest_completion(reprompt_child).unwrap())
            .unwrap();
        registry.submit_input(1, b"end").unwrap();
        wait_for_state(&mut registry, 1, AttentionState::Idle);
        assert!(
            CompletionLedger::new(registry.store())
                .latest_completion(1)
                .unwrap()
                .is_some()
        );
        assert!(
            matches!(
                TimerService::new(&mut registry)
                    .set_idle(
                        "mid-turn-parent".into(),
                        1,
                        "NEXT-TURN-WAIT".into(),
                        TimerKind::IdleAny,
                        vec![2],
                        60_000,
                        now_millis(),
                    )
                    .unwrap(),
                IdleTimerOutcome::Created(_)
            ),
            "an already consumed completion must not satisfy a new watch"
        );
        registry.stop(1).unwrap();
        for id in 2..=child_count + 1 {
            registry.stop(id).unwrap();
        }
    }

    #[cfg(unix)]
    #[test]
    fn idle_any_after_one_mid_turn_notice_is_already_satisfied() {
        check_mid_turn_notices(1, false);
    }

    #[cfg(unix)]
    #[test]
    fn idle_any_after_two_mid_turn_notices_is_already_satisfied() {
        check_mid_turn_notices(2, false);
    }

    #[cfg(unix)]
    #[test]
    fn mid_turn_notice_survives_a_late_owner_turn_end() {
        check_mid_turn_notices(1, true);
    }

    #[cfg(unix)]
    fn dialog_parent(mode: &str) -> ProcessRegistry {
        let mut registry = registry();
        let command = format!(
            "python3 -u '{}/tests/fixtures/dialog_composer.py' {mode}",
            env!("CARGO_MANIFEST_DIR")
        );
        registry.create(process(1, "parent", &command)).unwrap();
        registry
            .input_router()
            .set_typing_pause(crate::settings::TypingPauseSettings {
                enabled: false,
                delay_ms: 1000,
            });
        registry.start(1).unwrap();
        wait_for_output(
            &mut registry,
            1,
            if mode == "before" {
                "Do you want to proceed?"
            } else {
                "❯"
            },
        );
        registry
    }

    #[cfg(unix)]
    fn wait_for_submission_event(registry: &mut ProcessRegistry, kind: &str, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        loop {
            if registry
                .get_status(1)
                .unwrap()
                .events
                .iter()
                .any(|event| event.kind == kind)
                && !registry.has_pending_prompts(1)
            {
                return;
            }
            assert!(Instant::now() < deadline, "missing {kind}");
            thread::sleep(Duration::from_millis(20));
        }
    }

    #[cfg(unix)]
    #[test]
    fn dialog_after_notice_body_hands_off_once_and_next_timer_flows() {
        let mut registry = dialog_parent("after");
        registry.submit_input(1, b"hold").unwrap();
        wait_for_output(&mut registry, 1, "RECEIVED:hold");
        registry
            .create(process(2, "exiting-child", "exit 0"))
            .unwrap();
        registry.start(2).unwrap();
        registry.set_notify_spawner_on_idle(1, 2, true).unwrap();
        wait_for_state(&mut registry, 2, AttentionState::Exited);
        let mut service = SpawnerNotificationService::default();
        service.tick(&mut registry).unwrap();
        wait_for_submission_event(
            &mut registry,
            "submit_enter_blocked_by_dialog",
            Duration::from_secs(3),
        );
        assert!(!registry.has_pending_prompts(1));
        assert!(service.pending.is_empty()); // Enqueue-time bookkeeping, with no redelivery.
        let screen = registry.rendered_output(1).unwrap().text;
        assert!(!screen.contains("APPROVED_BY_ENTER"), "{screen}");

        // The timer stays due upstream until the human dismisses the dialog. No
        // human Enter or artificial typing pause flushes the handed-off draft.
        let now = now_millis();
        TimerService::new(&mut registry)
            .set_delay("human".into(), 1, "TIMER-WAKE".into(), 0, false, None, now)
            .unwrap();
        assert!(
            TimerService::new(&mut registry)
                .tick(now + 1)
                .unwrap()
                .is_empty()
        );
        registry
            .input_router()
            .send_terminal_input(1, b"\x05", true)
            .unwrap();
        wait_for_state(&mut registry, 1, AttentionState::Working);
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let fires = TimerService::new(&mut registry).tick(now_millis()).unwrap();
            if !fires.is_empty() {
                assert_eq!(fires.len(), 1);
                break;
            }
            assert!(
                Instant::now() < deadline,
                "timer stayed held after the dialog cleared"
            );
            thread::sleep(Duration::from_millis(20));
        }
        wait_for_output(&mut registry, 1, "RECEIVED:[workman]");
        let raw = registry.raw_output(1, None, 64 * 1024).unwrap();
        assert!(String::from_utf8_lossy(&raw.data).contains("output/todos ⏐ TIMER-WAKE"));
        for _ in 0..10 {
            service.tick(&mut registry).unwrap();
            thread::sleep(Duration::from_millis(100));
        }
        let screen = registry.rendered_output(1).unwrap().text;
        assert!(screen.contains("ENTER_COUNT:2"), "{screen}");
        assert!(!screen.contains("ENTER_COUNT:3"), "{screen}");
        assert!(
            !screen.contains("APPROVED_BY_ENTER") && !screen.contains("BODY_IN_DIALOG"),
            "{screen}"
        );
        assert!(!registry.has_pending_prompts(1));
        registry.stop(1).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn wrapped_timer_body_with_permission_phrases_submits_once() {
        let mut registry = dialog_parent("normal");
        let body = "Timer: check the worker output and reply (y/n) once you have read it; esc to cancel and enter to confirm are literal text.";
        let now = now_millis();
        TimerService::new(&mut registry)
            .set_delay("human".into(), 1, body.into(), 0, false, None, now)
            .unwrap();
        TimerService::new(&mut registry).tick(now + 1).unwrap();
        wait_for_output(&mut registry, 1, "RECEIVED:Timer:");
        thread::sleep(Duration::from_millis(1200));
        let status = registry.get_status(1).unwrap();
        assert!(
            status
                .events
                .iter()
                .all(|event| !event.kind.starts_with("submit_")),
            "{:?}",
            status.events
        );
        let screen = registry.rendered_output(1).unwrap().text;
        assert_eq!(screen.matches("RECEIVED:").count(), 1, "{screen}");
        assert!(!registry.has_pending_prompts(1));
        registry.stop(1).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn dialog_before_body_waits_then_sends_once() {
        let mut registry = dialog_parent("before");
        registry.submit_input(1, b"BODY-AFTER-CLEAR").unwrap();
        thread::sleep(Duration::from_millis(250));
        assert!(registry.has_pending_prompts(1));
        let screen = registry.rendered_output(1).unwrap().text;
        assert!(
            !screen.contains("BODY_IN_DIALOG") && !screen.contains("APPROVED_BY_ENTER"),
            "{screen}"
        );
        registry
            .input_router()
            .send_terminal_input(1, b"\x05", true)
            .unwrap();
        wait_for_output(&mut registry, 1, "RECEIVED:BODY-AFTER-CLEAR");
        let deadline = Instant::now() + Duration::from_secs(3);
        while registry.has_pending_prompts(1) {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(20));
        }
        registry.stop(1).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn permanent_dialog_drops_body_after_one_minute_and_releases_queue() {
        let mut registry = dialog_parent("before");
        let started = Instant::now();
        registry.submit_input(1, b"MUST-BE-DROPPED").unwrap();
        wait_for_submission_event(
            &mut registry,
            "submit_body_dialog_timeout",
            Duration::from_secs(65),
        );
        assert!(started.elapsed() >= Duration::from_secs(60));
        assert!(!registry.has_pending_prompts(1));
        registry.submit_input(1, b"LATER-MESSAGE").unwrap();
        registry
            .input_router()
            .send_terminal_input(1, b"\x05", true)
            .unwrap();
        wait_for_output(&mut registry, 1, "RECEIVED:LATER-MESSAGE");
        let screen = registry.rendered_output(1).unwrap().text;
        assert!(
            !screen.contains("MUST-BE-DROPPED")
                && !screen.contains("BODY_IN_DIALOG")
                && !screen.contains("APPROVED_BY_ENTER"),
            "{screen}"
        );
        assert_eq!(screen.matches("RECEIVED:").count(), 1, "{screen}");
        registry.stop(1).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn draft_indicator_requires_the_draft_to_be_the_only_blocker() {
        let mut registry = registry();
        let mut parent = process(
            1,
            "parent",
            r#"stty raw -echo; printf '❯ '; exec perl -e '$|=1; my $draft=""; while (sysread(STDIN,my $chunk,4096)) { for my $c (split //,$chunk) { if (ord($c)==4) { print "\e[2J\e[HDo you want to proceed?\r\n1. Yes\r\n2. No\r\nPress enter to continue"; next; } if (ord($c)==5) { print "\e[2J\e[H❯ $draft"; next; } if ($c eq "\r") { print "\e[2J\e[Hworking\r\n❯ $draft"; next; } $draft.=$c; print "\r\e[2K❯ $draft"; } }'"#,
        );
        parent.agent_tool_id = Some(91); // No verified queue-while-working capability.
        registry.create(parent).unwrap();
        registry.create(process(2, "child", "exit 0")).unwrap();
        registry
            .input_router()
            .set_typing_pause(crate::settings::TypingPauseSettings {
                enabled: false,
                delay_ms: 1000,
            });
        registry.start(1).unwrap();
        registry.start(2).unwrap();
        registry.set_notify_spawner_on_idle(1, 2, true).unwrap();
        wait_for_state(&mut registry, 2, AttentionState::Exited);
        registry
            .input_router()
            .send_terminal_input(1, b"human draft", true)
            .unwrap();
        wait_for_output(&mut registry, 1, "❯ human draft");
        wait_for_state(&mut registry, 1, AttentionState::Idle);
        let mut service = SpawnerNotificationService::default();
        service.tick(&mut registry).unwrap();
        assert!(registry.input_router().notification_held_by_draft(1));
        let reservation = registry.reserve_prompt(1).unwrap();
        service.tick(&mut registry).unwrap();
        assert!(!registry.input_router().notification_held_by_draft(1));
        drop(reservation);
        registry
            .input_router()
            .set_typing_pause(crate::settings::TypingPauseSettings {
                enabled: true,
                delay_ms: 60_000,
            });
        registry
            .input_router()
            .send_terminal_input(1, b"", true)
            .unwrap();
        service.tick(&mut registry).unwrap();
        assert!(!registry.input_router().notification_held_by_draft(1));
        registry
            .input_router()
            .set_typing_pause(crate::settings::TypingPauseSettings {
                enabled: false,
                delay_ms: 1000,
            });
        registry.send_input(1, b"\x04").unwrap();
        wait_for_state(&mut registry, 1, AttentionState::NeedsInput);
        assert!(registry.pending_dialog_viewport(1).unwrap().is_some());
        assert!(registry.input_router().has_unsent_human_draft(1).unwrap());
        service.tick(&mut registry).unwrap();
        assert!(!registry.input_router().notification_held_by_draft(1));
        registry.send_input(1, b"\x05").unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while registry.pending_dialog_viewport(1).unwrap().is_some() {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(10));
        }
        wait_for_state(&mut registry, 1, AttentionState::Idle);
        service.tick(&mut registry).unwrap();
        assert!(registry.input_router().notification_held_by_draft(1));
        registry.send_input(1, b"\r").unwrap();
        wait_for_output(&mut registry, 1, "working");
        assert_eq!(
            registry.agent_attention_snapshot(1).unwrap().state,
            AttentionState::Working
        );
        assert!(registry.input_router().has_unsent_human_draft(1).unwrap());
        service.tick(&mut registry).unwrap();
        assert!(!registry.input_router().notification_held_by_draft(1));
        wait_for_state(&mut registry, 1, AttentionState::Idle);
        service.tick(&mut registry).unwrap();
        assert!(registry.input_router().notification_held_by_draft(1));
        assert!(service.pending.contains_key(&1));
        registry.stop(1).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn working_claude_receives_coalesced_completions_and_later_child_turn() {
        let mut registry = registry();
        registry
            .create(process(
                1,
                "parent",
                "stty -echo; printf '❯\\n'; while IFS= read -r line; do if [ \"$line\" = hold ]; then printf 'thinking...\\nesc to interrupt\\n'; :; else printf 'received:[%s]\\nthinking...\\nesc to interrupt\\n' \"$line\"; fi; done",
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

        assert_eq!(
            registry.agent_attention_snapshot(1).unwrap().state,
            AttentionState::Working
        );
        let mut service = SpawnerNotificationService::default();
        let pending_prompt = registry.reserve_prompt(1).unwrap();
        service.tick(&mut registry).unwrap();
        assert_eq!(service.pending[&1].len(), 2);
        drop(pending_prompt);
        let router = registry.input_router();
        router.set_typing_pause(crate::settings::TypingPauseSettings {
            enabled: true,
            delay_ms: 60_000,
        });
        // Clear an empty input line: no draft, but human activity still pauses automation.
        router.send_terminal_input(1, b"\x15", true).unwrap();
        // This scripted busy UI has no composer to confirm the empty draft.
        router.clear_human_draft_activity(1);
        assert!(router.automatic_submission_held(1).unwrap());
        assert!(!router.has_unsent_human_draft(1).unwrap());
        service.tick(&mut registry).unwrap();
        assert_eq!(service.pending[&1].len(), 2);
        assert!(
            !registry
                .rendered_output(1)
                .unwrap()
                .text
                .contains("[workman]")
        );
        router.set_typing_pause(crate::settings::TypingPauseSettings {
            enabled: false,
            delay_ms: 60_000,
        });
        service.tick(&mut registry).unwrap();
        assert!(service.pending.is_empty());
        wait_for_output(&mut registry, 1, "[workman] child idle:");
        let output =
            String::from_utf8_lossy(&registry.raw_output(1, None, 64 * 1024).unwrap().data)
                .into_owned();
        assert_eq!(output.matches("[workman] child idle:").count(), 1);
        assert!(output.contains("first (2) finished; second (3) finished"));

        let first_boundary = registry
            .store()
            .spawner_idle_notification_setting(2)
            .unwrap()
            .unwrap()
            .last_finished_input_at
            .unwrap();
        assert!(
            CompletionLedger::new(registry.store())
                .unreported_completions(1, &[2, 3])
                .unwrap()
                .is_empty()
        );
        submit_as(&mut registry, 1, 2, b"again");
        wait_for_state(&mut registry, 2, AttentionState::Working);
        wait_for_state(&mut registry, 2, AttentionState::Idle);
        service.tick(&mut registry).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let output = registry.rendered_output(1).unwrap().text;
            if output.matches("first (2) finished").count() == 2 {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "second completion missing: {output}"
            );
            thread::sleep(Duration::from_millis(10));
        }
        assert!(
            registry
                .store()
                .spawner_idle_notification_setting(2)
                .unwrap()
                .unwrap()
                .last_finished_input_at
                .unwrap()
                > first_boundary
        );
        assert!(
            CompletionLedger::new(registry.store())
                .unreported_completions(1, &[2])
                .unwrap()
                .is_empty()
        );
        service.tick(&mut registry).unwrap();
        assert!(service.pending.is_empty());
        registry.stop(1).unwrap();
        registry.stop(2).unwrap();
        registry.stop(3).unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn non_claude_permission_phrases_keep_held_only_submission_checks() {
        for (tool, marker, body) in [
            (
                "codex",
                "›",
                "Timer: check the worker output and then would you like to run the suite again?",
            ),
            ("generic", ">", "Timer: reply (y/n) when the worker is done"),
        ] {
            let mut registry = registry();
            let mut adapter = registry.store().get_agent_tool(90).unwrap().unwrap();
            adapter.tool_type = tool.into();
            registry.store().put_agent_tool(&adapter).unwrap();
            let command = format!(
                "python3 -u '{}/tests/fixtures/dialog_composer.py' normal {tool}",
                env!("CARGO_MANIFEST_DIR")
            );
            registry.create(process(1, "parent", &command)).unwrap();
            registry
                .input_router()
                .set_typing_pause(crate::settings::TypingPauseSettings {
                    enabled: false,
                    delay_ms: 1000,
                });
            registry.start(1).unwrap();
            wait_for_output(&mut registry, 1, marker);
            registry
                .submit_input(1, b"Timer: control body without phrases")
                .unwrap();
            wait_for_output(&mut registry, 1, "ENTER_COUNT:1");
            let now = now_millis();
            TimerService::new(&mut registry)
                .set_delay("human".into(), 1, body.into(), 0, false, None, now)
                .unwrap();
            assert_eq!(
                TimerService::new(&mut registry)
                    .tick(now + 1)
                    .unwrap()
                    .len(),
                1
            );
            wait_for_output(&mut registry, 1, "ENTER_COUNT:2");
            let deadline = Instant::now() + Duration::from_secs(3);
            while registry.has_pending_prompts(1) {
                assert!(Instant::now() < deadline, "{tool}: prompt did not finish");
                thread::sleep(Duration::from_millis(20));
            }
            let status = registry.get_status(1).unwrap();
            assert!(
                status
                    .events
                    .iter()
                    .all(|event| event.kind != "submit_enter_blocked_by_dialog"),
                "{tool}: {:?}",
                status.events
            );
            assert!(!registry.has_pending_prompts(1));
            registry.stop(1).unwrap();
        }
    }

    #[cfg(unix)]
    #[test]
    fn handoff_and_nonhuman_dialog_clear_separate_the_next_child_notice() {
        let mut registry = dialog_parent("after");
        registry.submit_input(1, b"hold").unwrap();
        wait_for_output(&mut registry, 1, "RECEIVED:hold");
        registry
            .create(process(2, "first-child", "exit 0"))
            .unwrap();
        registry.start(2).unwrap();
        registry.set_notify_spawner_on_idle(1, 2, true).unwrap();
        wait_for_state(&mut registry, 2, AttentionState::Exited);
        let mut service = SpawnerNotificationService::default();
        service.tick(&mut registry).unwrap();
        wait_for_submission_event(
            &mut registry,
            "submit_enter_blocked_by_dialog",
            Duration::from_secs(3),
        );
        registry
            .input_router()
            .send_terminal_input(1, b"\x05", false)
            .unwrap();
        wait_for_state(&mut registry, 1, AttentionState::Working);
        registry
            .create(process(3, "second-child", "exit 0"))
            .unwrap();
        registry.start(3).unwrap();
        registry.set_notify_spawner_on_idle(1, 3, true).unwrap();
        wait_for_state(&mut registry, 3, AttentionState::Exited);
        service.tick(&mut registry).unwrap();
        wait_for_output(&mut registry, 1, "ENTER_COUNT:2");
        for _ in 0..10 {
            service.tick(&mut registry).unwrap();
            thread::sleep(Duration::from_millis(100));
        }
        let raw = registry.raw_output(1, None, 64 * 1024).unwrap();
        let text = String::from_utf8_lossy(&raw.data);
        assert!(
            text.contains("output/todos ⏐ [workman] child idle: second-child"),
            "{text}"
        );
        assert!(!text.contains("output/todos[workman]") && !text.contains("APPROVED_BY_ENTER"));
        assert!(!text.contains("ENTER_COUNT:3"));
        registry.stop(1).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn timers_stay_due_at_dialog_past_body_timeout_then_deliver_once() {
        let mut registry = dialog_parent("before");
        registry
            .create(process(
                2,
                "busy",
                "printf 'thinking...\\nesc to interrupt\\n'; sleep 120",
            ))
            .unwrap();
        registry
            .create(process(3, "idle", "printf '❯\\n'; sleep 120"))
            .unwrap();
        registry.start(2).unwrap();
        registry.start(3).unwrap();
        wait_for_state(&mut registry, 2, AttentionState::Working);
        wait_for_state(&mut registry, 3, AttentionState::Idle);
        let now = now_millis();
        TimerService::new(&mut registry)
            .set_delay(
                "human".into(),
                1,
                "DIALOG-DELAY".into(),
                0,
                false,
                None,
                now,
            )
            .unwrap();
        assert!(matches!(
            TimerService::new(&mut registry)
                .set_idle(
                    "human".into(),
                    1,
                    "DIALOG-DEADLINE".into(),
                    TimerKind::IdleAny,
                    vec![2],
                    100,
                    now
                )
                .unwrap(),
            IdleTimerOutcome::Created(_)
        ));
        // Even the immediate-satisfaction path must keep a wake upstream of the PTY.
        assert!(matches!(
            TimerService::new(&mut registry)
                .set_idle(
                    "human".into(),
                    1,
                    "DIALOG-IDLE".into(),
                    TimerKind::IdleAll,
                    vec![3],
                    60_000,
                    now
                )
                .unwrap(),
            IdleTimerOutcome::Created(_)
        ));
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(61) {
            assert!(
                TimerService::new(&mut registry)
                    .tick(now_millis())
                    .unwrap()
                    .is_empty()
            );
            assert!(!registry.has_pending_prompts(1));
            assert!(
                registry
                    .get_status(1)
                    .unwrap()
                    .events
                    .iter()
                    .all(|event| event.kind != "submit_body_dialog_timeout")
            );
            thread::sleep(Duration::from_millis(50));
        }
        registry
            .input_router()
            .send_terminal_input(1, b"\x05", false)
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(8);
        let mut fires = Vec::new();
        while fires.len() < 3 {
            fires.extend(TimerService::new(&mut registry).tick(now_millis()).unwrap());
            assert!(
                Instant::now() < deadline,
                "only {} timers delivered",
                fires.len()
            );
            thread::sleep(Duration::from_millis(20));
        }
        wait_for_output(&mut registry, 1, "ENTER_COUNT:3");
        let screen = registry
            .terminal_output_source(1)
            .unwrap()
            .unwrap()
            .read_viewport()
            .text();
        for body in ["DIALOG-DELAY", "DIALOG-DEADLINE", "DIALOG-IDLE"] {
            assert_eq!(
                screen.matches(&format!("RECEIVED:{body}")).count(),
                1,
                "{screen}"
            );
        }
        assert!(!screen.contains("BODY_IN_DIALOG") && !screen.contains("APPROVED_BY_ENTER"));
        assert!(
            TimerService::new(&mut registry)
                .tick(now_millis() + 120_000)
                .unwrap()
                .is_empty()
        );
        for id in 1..=3 {
            registry.stop(id).unwrap();
        }
    }

    #[cfg(unix)]
    #[test]
    fn timer_consumed_completion_cannot_loop_when_owner_rearms_for_busy_sibling() {
        let mut registry = registry();
        registry.create(process(1, "owner", "stty -echo; printf '❯\\n'; while IFS= read -r line; do printf 'received:[%s]\\nthinking...\\nesc to interrupt\\n' \"$line\"; if [ \"$line\" = end ]; then sleep 0.1; printf '❯\\n'; fi; done")).unwrap();
        registry.create(process(2, "A", "printf '❯\\n'; IFS= read -r line; printf 'thinking...\\nesc to interrupt\\n'; sleep 7; printf 'done\\n❯\\n'; sleep 120")).unwrap();
        registry.create(process(3, "B", "printf '❯\\n'; IFS= read -r line; printf 'thinking...\\nesc to interrupt\\n'; sleep 120")).unwrap();
        for id in 1..=3 {
            registry.start(id).unwrap();
            wait_for_state(&mut registry, id, AttentionState::Idle);
        }
        put_actor(&registry, "loop-owner", 1);
        submit_as(&mut registry, 1, 2, b"go");
        submit_as(&mut registry, 1, 3, b"go");
        registry.submit_input(1, b"end").unwrap();
        let first = TimerService::new(&mut registry)
            .set_idle(
                "loop-owner".into(),
                1,
                "LOOP-WAKE".into(),
                TimerKind::IdleAny,
                vec![2, 3],
                60_000,
                now_millis(),
            )
            .unwrap();
        assert!(matches!(first, IdleTimerOutcome::Created(_)));
        wait_for_state(&mut registry, 1, AttentionState::Waiting);
        let owner_end = CompletionLedger::new(registry.store())
            .latest_completion(1)
            .unwrap()
            .unwrap()
            .completed_at_ms;
        let deadline = Instant::now() + Duration::from_secs(15);
        while registry.get_status(2).unwrap().agent_state.state != AttentionState::Idle {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(20));
        }
        assert!(
            CompletionLedger::new(registry.store())
                .latest_completion(2)
                .unwrap()
                .unwrap()
                .completed_at_ms
                > owner_end
        );
        assert_eq!(
            TimerService::new(&mut registry)
                .tick(now_millis())
                .unwrap()
                .len(),
            1
        );
        wait_for_output(&mut registry, 1, "received:[LOOP-WAKE]");
        for arm in 2..=4 {
            assert_eq!(
                registry.agent_attention_snapshot(3).unwrap().state,
                AttentionState::Working
            );
            let outcome = TimerService::new(&mut registry)
                .set_idle(
                    "loop-owner".into(),
                    1,
                    "LOOP-WAKE".into(),
                    TimerKind::IdleAny,
                    vec![2, 3],
                    60_000,
                    now_millis(),
                )
                .unwrap();
            let IdleTimerOutcome::Created(timer) = outcome else {
                panic!("arm {arm} reused the consumed completion");
            };
            TimerService::new(&mut registry)
                .cancel("loop-owner", 1, timer.timer.id)
                .unwrap();
        }
        let raw = registry.raw_output(1, None, 64 * 1024).unwrap();
        assert_eq!(
            String::from_utf8_lossy(&raw.data)
                .matches("received:[LOOP-WAKE]")
                .count(),
            1
        );
        assert!(registry.mid_turn_deliveries.is_empty());
        for id in 1..=3 {
            registry.stop(id).unwrap();
        }
    }
}
