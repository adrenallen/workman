//! Durable timer scheduling and idle-transition wake-up delivery.

use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use tokio::{
    sync::watch,
    task::JoinHandle,
    time::{MissedTickBehavior, interval},
};
use workman_core::{
    ProcessId, ProjectId, StoreError, Timer, TimerId, TimerKind, attention::AttentionState,
};

use crate::{
    ProcessRegistry, RegistryError, SharedProcessRegistry,
    completion_ledger::{Completion, CompletionLedger, CompletionLedgerError},
    timer_events::{TimerLifecycleEvent, TimerLifecycleHub, TimerLifecycleKind},
};

const TIMER_POLL_INTERVAL: Duration = Duration::from_millis(25);
const TIMER_ERROR_LOG_INTERVAL: Duration = Duration::from_secs(60);

fn log_timer_error_rate_limited(
    timer_id: TimerId,
    error: &TimerError,
    quarantine_error: Option<&TimerError>,
) {
    static LAST_LOGGED: OnceLock<Mutex<BTreeMap<TimerId, Instant>>> = OnceLock::new();

    let now = Instant::now();
    let mut last_logged = LAST_LOGGED
        .get_or_init(|| Mutex::new(BTreeMap::new()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    last_logged.retain(|_, logged_at| now.duration_since(*logged_at) < TIMER_ERROR_LOG_INTERVAL);
    if last_logged.contains_key(&timer_id) {
        return;
    }
    last_logged.insert(timer_id, now);
    if let Some(quarantine_error) = quarantine_error {
        eprintln!(
            "timer {timer_id} tick failed: {error}; quarantine persistence also failed: {quarantine_error}"
        );
    } else {
        eprintln!("timer {timer_id} tick failed and was quarantined: {error}");
    }
}

#[derive(Debug)]
pub(crate) enum TimerError {
    Store(StoreError),
    Registry(RegistryError),
    CompletionLedger(CompletionLedgerError),
    Persistence(String),
    NotFound(TimerId),
    Inactive(TimerId),
    EmptyWatchList,
    InvalidDelay,
    InvalidRepeatInterval,
    InvalidMaxWait,
    ConflictingSchedule,
    IntervalNotEditable(TimerId),
    CrossProjectTarget {
        owner_project_id: ProjectId,
        target_process_id: ProcessId,
        target_project_id: ProjectId,
    },
}

impl TimerError {
    pub(crate) const fn code(&self) -> &'static str {
        match self {
            Self::Store(_) | Self::CompletionLedger(_) | Self::Persistence(_) => {
                "timer_store_error"
            }
            Self::Registry(error) => error.code(),
            Self::NotFound(_) => "timer_not_found",
            Self::Inactive(_) => "timer_inactive",
            Self::EmptyWatchList => "empty_watch_list",
            Self::InvalidDelay => "invalid_delay",
            Self::InvalidRepeatInterval => "invalid_repeat_interval",
            Self::InvalidMaxWait => "invalid_max_wait",
            Self::ConflictingSchedule => "conflicting_timer_schedule",
            Self::IntervalNotEditable(_) => "timer_interval_not_editable",
            Self::CrossProjectTarget { .. } => "timer_cross_project_target",
        }
    }
}

impl fmt::Display for TimerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(error) => error.fmt(formatter),
            Self::Registry(error) => error.fmt(formatter),
            Self::CompletionLedger(error) => error.fmt(formatter),
            Self::Persistence(message) => formatter.write_str(message),
            Self::NotFound(timer_id) => write!(formatter, "timer {timer_id} was not found"),
            Self::Inactive(timer_id) => write!(formatter, "timer {timer_id} is no longer active"),
            Self::EmptyWatchList => formatter.write_str("watch list must contain a process"),
            Self::InvalidDelay => formatter.write_str("delay_ms must fit in a signed 64-bit value"),
            Self::InvalidRepeatInterval => {
                formatter.write_str("repeat interval must be greater than zero")
            }
            Self::InvalidMaxWait => {
                formatter.write_str("max_wait_ms must fit in a signed 64-bit value")
            }
            Self::ConflictingSchedule => {
                formatter.write_str("provide due_at or delay_ms, not both")
            }
            Self::IntervalNotEditable(timer_id) => write!(
                formatter,
                "timer {timer_id} is not recurring, so its interval cannot be edited"
            ),
            Self::CrossProjectTarget {
                owner_project_id,
                target_process_id,
                target_project_id,
            } => write!(
                formatter,
                "agent identities are scoped to project {owner_project_id}; timer target process {target_process_id} belongs to project {target_project_id}"
            ),
        }
    }
}

impl Error for TimerError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Store(error) => Some(error),
            Self::Registry(error) => Some(error),
            Self::CompletionLedger(error) => Some(error),
            _ => None,
        }
    }
}

impl From<StoreError> for TimerError {
    fn from(error: StoreError) -> Self {
        Self::Store(error)
    }
}

impl From<RegistryError> for TimerError {
    fn from(error: RegistryError) -> Self {
        Self::Registry(error)
    }
}

impl From<CompletionLedgerError> for TimerError {
    fn from(error: CompletionLedgerError) -> Self {
        Self::CompletionLedger(error)
    }
}

pub(crate) type TimerResult<T> = Result<T, TimerError>;

#[derive(Clone, Debug, Default)]
pub(crate) struct TimerEdit {
    pub body: Option<String>,
    pub due_at: Option<i64>,
    pub delay_ms: Option<i64>,
    pub interval_ms: Option<i64>,
    pub paused: Option<bool>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct WatchProgress {
    armed: bool,
    satisfied: bool,
    last_idle: bool,
    #[serde(default)]
    completion_id: Option<i64>,
}

impl WatchProgress {
    pub(crate) const fn new(initial_idle: bool, already_satisfied: bool) -> Self {
        Self {
            armed: !initial_idle,
            satisfied: already_satisfied,
            last_idle: initial_idle,
            completion_id: None,
        }
    }

    #[cfg(test)]
    pub(crate) const fn satisfied(&self) -> bool {
        self.satisfied
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
struct TimerRuntime {
    due_at: i64,
    paused_at: Option<i64>,
    watch_state: BTreeMap<ProcessId, WatchProgress>,
    diagnostics: TimerDiagnostics,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
struct TimerDiagnostics {
    already_idle: Vec<ProcessId>,
    satisfied_by: Vec<TimerSatisfaction>,
    fire_reason: Option<TimerSatisfactionReason>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct TimerSatisfaction {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub process_id: Option<ProcessId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at_ms: Option<i64>,
    pub reason: TimerSatisfactionReason,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TimerSatisfactionReason {
    UnseenCompletion,
    FreshTransition,
    Deadline,
    Error,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct TimerView {
    #[serde(flatten)]
    pub timer: Timer,
    pub owner_process_name: Option<String>,
    pub owner_label: String,
    pub due_at: i64,
    pub paused_at: Option<i64>,
    pub already_idle: Vec<ProcessId>,
    pub satisfied_by: Vec<TimerSatisfaction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fire_reason: Option<TimerSatisfactionReason>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TimerFireReason {
    Delay,
    IdleTransition,
    MaxWait,
    AlreadySatisfied,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct TimerFire {
    pub timer_id: TimerId,
    pub project_id: ProjectId,
    pub delivery_process_id: ProcessId,
    pub reason: TimerFireReason,
    pub fired_at: i64,
    pub timer: TimerView,
}

#[derive(Clone, Debug)]
pub(crate) enum IdleTimerOutcome {
    Created(Box<TimerView>),
    AlreadySatisfied {
        watch_process_ids: Vec<ProcessId>,
        delivery_process_id: ProcessId,
        delivered_at: i64,
        already_idle: Vec<ProcessId>,
        satisfied_by: Vec<TimerSatisfaction>,
    },
}

pub(crate) struct TimerService<'a> {
    registry: &'a mut ProcessRegistry,
}

impl<'a> TimerService<'a> {
    pub(crate) fn new(registry: &'a mut ProcessRegistry) -> Self {
        Self { registry }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn set_delay(
        &mut self,
        owner_actor: String,
        delivery_process_id: ProcessId,
        body: String,
        delay_ms: i64,
        loop_timer: bool,
        repeat_every_ms: Option<i64>,
        now_ms: i64,
    ) -> TimerResult<TimerView> {
        if delay_ms < 0 {
            return Err(TimerError::InvalidDelay);
        }
        if repeat_every_ms.is_some_and(|interval| interval <= 0) {
            return Err(TimerError::InvalidRepeatInterval);
        }
        let owner_process_id = self.owner_process_id(&owner_actor)?;
        self.validate_agent_targets(&owner_actor, owner_process_id, delivery_process_id, &[])?;
        let repeating = loop_timer || repeat_every_ms.is_some();
        let repeat_interval = if repeating {
            Some(repeat_every_ms.unwrap_or(delay_ms).max(1))
        } else {
            None
        };
        let due_at = now_ms.saturating_add(delay_ms);
        let timer = Timer {
            id: self.next_timer_id()?,
            owner_actor,
            owner_process_id,
            delivery_process_id,
            body,
            kind: TimerKind::Delay,
            watch_process_ids: Vec::new(),
            interval_ms: repeat_interval,
            repeating,
            max_wait_deadline: Some(due_at),
            paused: false,
            fired: false,
            fired_at: None,
            created_at: now_ms,
        };
        let runtime = TimerRuntime {
            due_at,
            ..TimerRuntime::default()
        };
        self.insert(&timer, &runtime)?;
        self.registry.status_invalidations().invalidate();
        self.view(timer, runtime)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn set_idle(
        &mut self,
        owner_actor: String,
        delivery_process_id: ProcessId,
        body: String,
        kind: TimerKind,
        watch_process_ids: Vec<ProcessId>,
        max_wait_ms: i64,
        now_ms: i64,
    ) -> TimerResult<IdleTimerOutcome> {
        if max_wait_ms < 0 {
            return Err(TimerError::InvalidMaxWait);
        }
        if watch_process_ids.is_empty() {
            return Err(TimerError::EmptyWatchList);
        }
        let owner_process_id = self.owner_process_id(&owner_actor)?;
        self.validate_agent_targets(
            &owner_actor,
            owner_process_id,
            delivery_process_id,
            &watch_process_ids,
        )?;
        debug_assert!(matches!(kind, TimerKind::IdleAny | TimerKind::IdleAll));

        let watch_process_ids = watch_process_ids
            .into_iter()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let mut idle_by_process = BTreeMap::new();
        let mut already_idle = Vec::new();
        for process_id in &watch_process_ids {
            let idle = self.process_is_idle(*process_id)?;
            idle_by_process.insert(*process_id, idle);
            if idle {
                already_idle.push(*process_id);
            }
        }
        let unseen_by_process = owner_process_id
            .map(|owner_process_id| {
                CompletionLedger::new(self.registry.store())
                    .unreported_completions(owner_process_id, &watch_process_ids)
            })
            .transpose()?
            .unwrap_or_default()
            .into_iter()
            .map(|completion| (completion.process_id, completion))
            .collect::<BTreeMap<_, _>>();
        let mut watch_state = BTreeMap::new();
        let mut satisfied_by = Vec::new();
        for process_id in &watch_process_ids {
            let idle = idle_by_process[process_id];
            let unseen = idle.then(|| unseen_by_process.get(process_id)).flatten();
            let already_satisfied = kind == TimerKind::IdleAll && idle || unseen.is_some();
            let mut progress = WatchProgress::new(idle, already_satisfied);
            if let Some(completion) = unseen {
                progress.completion_id = Some(completion.id);
                satisfied_by.push(TimerSatisfaction {
                    process_id: Some(*process_id),
                    completed_at_ms: Some(completion.completed_at_ms),
                    reason: TimerSatisfactionReason::UnseenCompletion,
                });
            }
            watch_state.insert(*process_id, progress);
        }
        let already_satisfied = match kind {
            TimerKind::IdleAny => watch_state.values().any(|progress| progress.satisfied),
            TimerKind::IdleAll => watch_state.values().all(|progress| progress.satisfied),
            TimerKind::Delay => false,
        };
        if already_satisfied {
            self.registry
                .submit_input(delivery_process_id, body.as_bytes())?;
            if let Some(owner_process_id) = owner_process_id {
                let ledger = CompletionLedger::new(self.registry.store());
                if owner_process_id != delivery_process_id
                    && let Err(error) =
                        ledger.record_input(owner_process_id, delivery_process_id, now_ms)
                {
                    eprintln!(
                        "idle timer immediate delivery ledger input failed for process {delivery_process_id}: {error}"
                    );
                }
                // submit_input only guarantees that delivery is queued. There is no cheap
                // callback from the PTY worker's eventual write, so report bookkeeping is
                // intentionally enqueue-time and best-effort. A daemon crash in between can
                // lose both this wake and its re-arm safety net.
                if let Err(error) = ledger.mark_completions_reported(
                    owner_process_id,
                    unseen_by_process
                        .values()
                        .filter(|completion| idle_by_process[&completion.process_id])
                        .copied(),
                ) {
                    eprintln!("idle timer immediate delivery ledger report failed: {error}");
                }
            }
            return Ok(IdleTimerOutcome::AlreadySatisfied {
                watch_process_ids,
                delivery_process_id,
                delivered_at: now_ms,
                already_idle,
                satisfied_by,
            });
        }

        let due_at = now_ms.saturating_add(max_wait_ms);
        let timer = Timer {
            id: self.next_timer_id()?,
            owner_actor,
            owner_process_id,
            delivery_process_id,
            body,
            kind,
            watch_process_ids,
            interval_ms: None,
            repeating: false,
            max_wait_deadline: Some(due_at),
            paused: false,
            fired: false,
            fired_at: None,
            created_at: now_ms,
        };
        let runtime = TimerRuntime {
            due_at,
            paused_at: None,
            watch_state,
            diagnostics: TimerDiagnostics {
                already_idle,
                satisfied_by,
                fire_reason: None,
            },
        };
        self.insert(&timer, &runtime)?;
        self.registry.status_invalidations().invalidate();
        Ok(IdleTimerOutcome::Created(Box::new(
            self.view(timer, runtime)?,
        )))
    }

    pub(crate) fn cancel(
        &mut self,
        owner_actor: &str,
        project_id: ProjectId,
        timer_id: TimerId,
    ) -> TimerResult<TimerView> {
        let timer = self.owned_timer(owner_actor, project_id, timer_id)?;
        let runtime = self.runtime_or_reconstruct(&timer, timer.created_at)?;
        self.registry
            .store()
            .connection()
            .execute("DELETE FROM timers WHERE id = ?1", [timer_id])
            .map_err(persistence)?;
        self.registry.status_invalidations().invalidate();
        self.view(timer, runtime)
    }

    pub(crate) fn pause(
        &mut self,
        owner_actor: &str,
        project_id: ProjectId,
        timer_id: TimerId,
        now_ms: i64,
    ) -> TimerResult<TimerView> {
        let mut timer = self.owned_timer(owner_actor, project_id, timer_id)?;
        if timer.fired && !timer.repeating {
            return Err(TimerError::Inactive(timer_id));
        }
        let mut runtime = self.runtime_or_reconstruct(&timer, now_ms)?;
        if !timer.paused {
            timer.paused = true;
            runtime.paused_at = Some(now_ms);
            self.update(&timer, &runtime)?;
            self.registry.status_invalidations().invalidate();
        }
        self.view(timer, runtime)
    }

    pub(crate) fn resume(
        &mut self,
        owner_actor: &str,
        project_id: ProjectId,
        timer_id: TimerId,
        now_ms: i64,
    ) -> TimerResult<TimerView> {
        let mut timer = self.owned_timer(owner_actor, project_id, timer_id)?;
        if timer.fired && !timer.repeating {
            return Err(TimerError::Inactive(timer_id));
        }
        let mut runtime = self.runtime_or_reconstruct(&timer, now_ms)?;
        if timer.paused {
            if let Some(paused_at) = runtime.paused_at {
                runtime.due_at = runtime
                    .due_at
                    .saturating_add(now_ms.saturating_sub(paused_at).max(0));
            }
            runtime.paused_at = None;
            timer.paused = false;
            timer.max_wait_deadline = Some(runtime.due_at);
            self.update(&timer, &runtime)?;
            self.registry.status_invalidations().invalidate();
        }
        self.view(timer, runtime)
    }

    pub(crate) fn list(
        &mut self,
        project_id: ProjectId,
        limit: usize,
        now_ms: i64,
    ) -> TimerResult<Vec<TimerView>> {
        let timer_ids = {
            let mut statement = self
                .registry
                .store()
                .connection()
                .prepare(
                    "SELECT timer.id
                     FROM timers AS timer
                     JOIN processes AS process ON process.id = timer.delivery_process_id
                     WHERE process.project_id = ?1
                     ORDER BY timer.created_at DESC, timer.id DESC
                     LIMIT ?2",
                )
                .map_err(persistence)?;
            let rows = statement
                .query_map((project_id, limit as i64), |row| row.get(0))
                .map_err(persistence)?;
            let mut timer_ids = Vec::new();
            for row in rows {
                timer_ids.push(row.map_err(persistence)?);
            }
            timer_ids
        };

        let mut views = Vec::with_capacity(timer_ids.len());
        for timer_id in timer_ids {
            let Some(timer) = self.registry.store().get_timer(timer_id)? else {
                continue;
            };
            let runtime = self.runtime_or_reconstruct(&timer, now_ms)?;
            views.push(self.view(timer, runtime)?);
        }
        Ok(views)
    }

    /// Return every timer in one project for the authenticated human control surface.
    pub(crate) fn list_project(
        &mut self,
        project_id: ProjectId,
        now_ms: i64,
    ) -> TimerResult<Vec<TimerView>> {
        let timer_ids = {
            let mut statement = self
                .registry
                .store()
                .connection()
                .prepare(
                    "SELECT timer.id
                     FROM timers AS timer
                     JOIN processes AS process ON process.id = timer.delivery_process_id
                     WHERE process.project_id = ?1
                     ORDER BY timer.fired ASC,
                              timer.paused ASC,
                              COALESCE(
                                (SELECT due_at FROM timer_runtime WHERE timer_id = timer.id),
                                timer.max_wait_deadline,
                                timer.created_at
                              ),
                              timer.id",
                )
                .map_err(persistence)?;
            let rows = statement
                .query_map([project_id], |row| row.get(0))
                .map_err(persistence)?;
            let mut timer_ids = Vec::new();
            for row in rows {
                timer_ids.push(row.map_err(persistence)?);
            }
            timer_ids
        };

        let mut views = Vec::with_capacity(timer_ids.len());
        for timer_id in timer_ids {
            let Some(timer) = self.registry.store().get_timer(timer_id)? else {
                continue;
            };
            let runtime = self.runtime_or_reconstruct(&timer, now_ms)?;
            views.push(self.view(timer, runtime)?);
        }
        Ok(views)
    }

    /// Edit any timer in one project. This is reserved for the authenticated human surface.
    pub(crate) fn edit_project_timer(
        &mut self,
        project_id: ProjectId,
        timer_id: TimerId,
        edit: TimerEdit,
        now_ms: i64,
    ) -> TimerResult<TimerView> {
        if edit.due_at.is_some() && edit.delay_ms.is_some() {
            return Err(TimerError::ConflictingSchedule);
        }
        if edit.delay_ms.is_some_and(|delay| delay < 0) || edit.due_at.is_some_and(|due| due < 0) {
            return Err(TimerError::InvalidDelay);
        }
        if edit.interval_ms.is_some_and(|interval| interval <= 0) {
            return Err(TimerError::InvalidRepeatInterval);
        }

        let mut timer = self.project_timer(project_id, timer_id)?;
        if timer.fired && !timer.repeating {
            return Err(TimerError::Inactive(timer_id));
        }
        if edit.interval_ms.is_some() && !timer.repeating {
            return Err(TimerError::IntervalNotEditable(timer_id));
        }
        let mut runtime = self.runtime_or_reconstruct(&timer, now_ms)?;
        let schedule_changed = edit.due_at.is_some() || edit.delay_ms.is_some();
        if let Some(body) = edit.body {
            timer.body = body;
        }
        if let Some(due_at) = edit
            .due_at
            .or_else(|| edit.delay_ms.map(|delay| now_ms.saturating_add(delay)))
        {
            runtime.due_at = due_at;
            timer.max_wait_deadline = Some(due_at);
        }
        if let Some(interval_ms) = edit.interval_ms {
            timer.interval_ms = Some(interval_ms);
        }
        if let Some(paused) = edit.paused {
            match (timer.paused, paused) {
                (false, true) => {
                    timer.paused = true;
                    runtime.paused_at = Some(now_ms);
                }
                (true, false) => {
                    if !schedule_changed && let Some(paused_at) = runtime.paused_at {
                        runtime.due_at = runtime
                            .due_at
                            .saturating_add(now_ms.saturating_sub(paused_at).max(0));
                        timer.max_wait_deadline = Some(runtime.due_at);
                    }
                    timer.paused = false;
                    runtime.paused_at = None;
                }
                _ => {}
            }
        }
        self.update(&timer, &runtime)?;
        self.view(timer, runtime)
    }

    /// Delete any timer in one project. This is reserved for the authenticated human surface.
    pub(crate) fn delete_project_timer(
        &mut self,
        project_id: ProjectId,
        timer_id: TimerId,
    ) -> TimerResult<TimerView> {
        let timer = self.project_timer(project_id, timer_id)?;
        let runtime = self.runtime_or_reconstruct(&timer, timer.created_at)?;
        self.registry
            .store()
            .connection()
            .execute("DELETE FROM timers WHERE id = ?1", [timer_id])
            .map_err(persistence)?;
        self.registry.status_invalidations().invalidate();
        self.view(timer, runtime)
    }

    /// Return every active or paused timer for status-stream reconciliation.
    pub(crate) fn list_active(&mut self, now_ms: i64) -> TimerResult<Vec<TimerView>> {
        let timer_ids = {
            let mut statement = self
                .registry
                .store()
                .connection()
                .prepare(
                    "SELECT timer.id
                     FROM timers AS timer
                     LEFT JOIN timer_runtime AS runtime ON runtime.timer_id = timer.id
                     WHERE timer.fired = 0
                     ORDER BY COALESCE(runtime.due_at, timer.max_wait_deadline, timer.created_at), timer.id",
                )
                .map_err(persistence)?;
            let rows = statement
                .query_map([], |row| row.get(0))
                .map_err(persistence)?;
            let mut timer_ids = Vec::new();
            for row in rows {
                timer_ids.push(row.map_err(persistence)?);
            }
            timer_ids
        };

        let mut views = Vec::with_capacity(timer_ids.len());
        for timer_id in timer_ids {
            let Some(timer) = self.registry.store().get_timer(timer_id)? else {
                continue;
            };
            let runtime = self.runtime_or_reconstruct(&timer, now_ms)?;
            views.push(self.view(timer, runtime)?);
        }
        Ok(views)
    }

    pub(crate) fn tick(&mut self, now_ms: i64) -> TimerResult<Vec<TimerFire>> {
        let timer_ids = self.pending_timer_ids()?;
        let mut fired = Vec::new();
        for timer_id in timer_ids {
            match self.tick_one(timer_id, now_ms) {
                Ok(Some(fire)) => fired.push(fire),
                Ok(None) => {}
                Err(error) => {
                    // A corrupt/deleted target or one timer's persistence failure must not
                    // spin forever or prevent later timers from being evaluated. Persisting
                    // fired first removes the owner from waiting state; the fallback runtime
                    // makes timer_list readable even when its prior JSON was corrupt.
                    let quarantine_error = self.quarantine_timer(timer_id, now_ms).err();
                    log_timer_error_rate_limited(timer_id, &error, quarantine_error.as_ref());
                }
            }
        }
        Ok(fired)
    }

    fn quarantine_timer(&self, timer_id: TimerId, now_ms: i64) -> TimerResult<()> {
        let Some(mut timer) = self.registry.store().get_timer(timer_id)? else {
            return Ok(());
        };
        if timer.fired {
            return Ok(());
        }
        timer.fired = true;
        timer.fired_at = Some(now_ms);
        self.registry.store().put_timer(&timer)?;
        self.registry.status_invalidations().invalidate();

        let mut diagnostics = TimerDiagnostics::default();
        diagnostics.record_error();
        self.put_runtime(
            timer.id,
            &TimerRuntime {
                due_at: timer.max_wait_deadline.unwrap_or(timer.created_at),
                paused_at: None,
                watch_state: BTreeMap::new(),
                diagnostics,
            },
        )
    }

    fn tick_one(&mut self, timer_id: TimerId, now_ms: i64) -> TimerResult<Option<TimerFire>> {
        let Some(mut timer) = self.registry.store().get_timer(timer_id)? else {
            return Ok(None);
        };
        if timer.paused || timer.fired {
            return Ok(None);
        }
        match self.validate_agent_targets(
            &timer.owner_actor,
            timer.owner_process_id,
            timer.delivery_process_id,
            &timer.watch_process_ids,
        ) {
            Ok(()) => {}
            Err(error @ TimerError::CrossProjectTarget { .. }) => {
                timer.fired = true;
                timer.fired_at = Some(now_ms);
                self.registry.store().put_timer(&timer)?;
                eprintln!("quarantined invalid timer {}: {error}", timer.id);
                return Ok(None);
            }
            Err(error) => return Err(error),
        }
        let mut runtime = self.runtime_or_reconstruct(&timer, now_ms)?;
        let mut reason = None;
        let mut transitioned_process_ids = Vec::new();

        match timer.kind {
            TimerKind::Delay => {
                if now_ms >= runtime.due_at {
                    reason = Some(TimerFireReason::Delay);
                }
            }
            TimerKind::IdleAny | TimerKind::IdleAll => {
                let advanced = self.advance_idle_state(&timer, &mut runtime, now_ms)?;
                transitioned_process_ids = advanced.transitioned_process_ids;
                if idle_condition_satisfied(&timer, &runtime) {
                    reason = Some(TimerFireReason::IdleTransition);
                    runtime.diagnostics.fire_reason = Some(
                        runtime
                            .diagnostics
                            .satisfied_by
                            .iter()
                            .rev()
                            .find_map(|satisfaction| match satisfaction.reason {
                                TimerSatisfactionReason::UnseenCompletion
                                | TimerSatisfactionReason::FreshTransition => {
                                    Some(satisfaction.reason)
                                }
                                TimerSatisfactionReason::Deadline
                                | TimerSatisfactionReason::Error => None,
                            })
                            .unwrap_or(TimerSatisfactionReason::FreshTransition),
                    );
                } else if now_ms >= runtime.due_at {
                    reason = Some(TimerFireReason::MaxWait);
                    runtime.diagnostics.record_deadline();
                }
                if advanced.changed && reason.is_none() {
                    self.put_runtime(timer.id, &runtime)?;
                }
            }
        }

        let Some(reason) = reason else {
            return Ok(None);
        };
        let project_id = self.registry.get(timer.delivery_process_id)?.project_id;
        if self
            .registry
            .submit_input(timer.delivery_process_id, timer.body.as_bytes())
            .is_err()
        {
            // Delivery is at-least-once. Keep the timer pending and retry after
            // the target process is started again.
            return Ok(None);
        }

        // Persist the consumed/rescheduled timer immediately after the externally visible
        // submit_input side effect. No fallible ledger operation may leave it pending and
        // cause the body to be queued again on the next scheduler tick.
        timer.fired_at = Some(now_ms);
        if timer.kind == TimerKind::Delay && timer.repeating {
            let repeat_every_ms = timer.interval_ms.unwrap_or(1).max(1);
            runtime.due_at = now_ms.saturating_add(repeat_every_ms);
            runtime.paused_at = None;
            timer.max_wait_deadline = Some(runtime.due_at);
            timer.fired = false;
        } else {
            timer.fired = true;
        }
        self.update(&timer, &runtime)?;
        self.registry.status_invalidations().invalidate();

        if let Some(owner_process_id) = timer.owner_process_id {
            let ledger = CompletionLedger::new(self.registry.store());
            if owner_process_id != timer.delivery_process_id
                && let Err(error) =
                    ledger.record_input(owner_process_id, timer.delivery_process_id, now_ms)
            {
                eprintln!(
                    "timer {} delivery ledger input failed for process {}: {error}",
                    timer.id, timer.delivery_process_id
                );
            }
            if reason == TimerFireReason::IdleTransition {
                let completions =
                    runtime
                        .watch_state
                        .iter()
                        .filter_map(|(process_id, progress)| {
                            progress.completion_id.map(|id| Completion {
                                id,
                                process_id: *process_id,
                                completed_at_ms: runtime
                                    .diagnostics
                                    .satisfied_by
                                    .iter()
                                    .rev()
                                    .find(|satisfaction| {
                                        satisfaction.process_id == Some(*process_id)
                                    })
                                    .and_then(|satisfaction| satisfaction.completed_at_ms)
                                    .unwrap_or(now_ms),
                            })
                        });
                // See set_idle's immediate path: queue acceptance is the best delivery hook
                // available today, so reporting is enqueue-time and deliberately best-effort.
                if let Err(error) = ledger.mark_completions_reported(owner_process_id, completions)
                {
                    eprintln!("timer {} ledger report failed: {error}", timer.id);
                }
            }
        }

        // Pending timers suppress directly. Once an idle-transition timer
        // is consumed, preserve that transition's watch state across the
        // debounced done-check. Max-wait expiry is only a wake about a
        // still-busy process and deliberately records no marker.
        if reason == TimerFireReason::IdleTransition {
            for process_id in transitioned_process_ids {
                if let Err(error) = self
                    .registry
                    .store()
                    .record_consumed_idle_watch(process_id, timer.id, now_ms)
                {
                    eprintln!(
                        "timer {} consumed-idle marker failed for process {process_id}: {error}",
                        timer.id
                    );
                }
            }
        }

        Ok(Some(TimerFire {
            timer_id: timer.id,
            project_id,
            delivery_process_id: timer.delivery_process_id,
            reason,
            fired_at: now_ms,
            timer: self.view(timer, runtime)?,
        }))
    }

    fn owned_timer(
        &self,
        owner_actor: &str,
        project_id: ProjectId,
        timer_id: TimerId,
    ) -> TimerResult<Timer> {
        let timer = self
            .registry
            .store()
            .get_timer(timer_id)?
            .ok_or(TimerError::NotFound(timer_id))?;
        let delivery = self
            .registry
            .store()
            .get_process(timer.delivery_process_id)?
            .filter(|process| process.project_id == project_id)
            .ok_or(TimerError::NotFound(timer_id))?;
        debug_assert_eq!(delivery.id, timer.delivery_process_id);
        let actor_process_id = self
            .registry
            .store()
            .get_actor(owner_actor)?
            .and_then(|actor| actor.process_id);
        let authorized = match timer.owner_process_id {
            Some(owner_process_id) => actor_process_id == Some(owner_process_id),
            None => timer.owner_actor == owner_actor,
        };
        if !authorized {
            return Err(TimerError::NotFound(timer_id));
        }
        Ok(timer)
    }

    fn project_timer(&self, project_id: ProjectId, timer_id: TimerId) -> TimerResult<Timer> {
        let timer = self
            .registry
            .store()
            .get_timer(timer_id)?
            .ok_or(TimerError::NotFound(timer_id))?;
        self.registry
            .store()
            .get_process(timer.delivery_process_id)?
            .filter(|process| process.project_id == project_id)
            .ok_or(TimerError::NotFound(timer_id))?;
        Ok(timer)
    }

    fn validate_agent_targets(
        &self,
        owner_actor: &str,
        owner_process_id: Option<ProcessId>,
        delivery_process_id: ProcessId,
        watch_process_ids: &[ProcessId],
    ) -> TimerResult<()> {
        let owner_process_id = match owner_process_id {
            Some(process_id) => Some(process_id),
            None => self
                .registry
                .store()
                .get_actor(owner_actor)?
                .and_then(|actor| actor.process_id),
        };
        let Some(owner_process_id) = owner_process_id else {
            return Ok(());
        };
        let Some(owner_process) = self.registry.store().get_process(owner_process_id)? else {
            return Ok(());
        };
        let owner_project_id = owner_process.project_id;
        for target_process_id in
            std::iter::once(delivery_process_id).chain(watch_process_ids.iter().copied())
        {
            let Some(target_process) = self.registry.store().get_process(target_process_id)? else {
                continue;
            };
            if target_process.project_id != owner_project_id {
                return Err(TimerError::CrossProjectTarget {
                    owner_project_id,
                    target_process_id,
                    target_project_id: target_process.project_id,
                });
            }
        }
        Ok(())
    }

    fn owner_process_id(&self, owner_actor: &str) -> TimerResult<Option<ProcessId>> {
        Ok(self
            .registry
            .store()
            .get_actor(owner_actor)?
            .and_then(|actor| actor.process_id))
    }

    fn view(&self, timer: Timer, runtime: TimerRuntime) -> TimerResult<TimerView> {
        let owner_process_name = timer
            .owner_process_id
            .map(|process_id| self.registry.store().get_process(process_id))
            .transpose()?
            .flatten()
            .map(|process| process.name);
        let owner_label = owner_process_name.clone().unwrap_or_else(|| {
            self.registry
                .store()
                .ownership_display_label(&timer.owner_actor, None)
        });
        Ok(TimerView::new(
            timer,
            owner_process_name,
            owner_label,
            runtime,
        ))
    }

    fn next_timer_id(&self) -> TimerResult<TimerId> {
        self.registry
            .store()
            .connection()
            .query_row(
                "SELECT next_id FROM timer_id_sequence WHERE singleton = 1",
                [],
                |row| row.get(0),
            )
            .map_err(persistence)
    }

    fn insert(&self, timer: &Timer, runtime: &TimerRuntime) -> TimerResult<()> {
        self.registry.store().put_timer(timer)?;
        if let Err(error) = self.put_runtime(timer.id, runtime) {
            let _ = self
                .registry
                .store()
                .connection()
                .execute("DELETE FROM timers WHERE id = ?1", [timer.id]);
            return Err(error);
        }
        Ok(())
    }

    fn update(&self, timer: &Timer, runtime: &TimerRuntime) -> TimerResult<()> {
        self.registry.store().put_timer(timer)?;
        self.put_runtime(timer.id, runtime)
    }

    fn put_runtime(&self, timer_id: TimerId, runtime: &TimerRuntime) -> TimerResult<()> {
        let watch_state = serde_json::to_string(&runtime.watch_state)
            .map_err(|error| TimerError::Persistence(error.to_string()))?;
        let diagnostics = serde_json::to_string(&runtime.diagnostics)
            .map_err(|error| TimerError::Persistence(error.to_string()))?;
        self.registry
            .store()
            .connection()
            .execute(
                "INSERT INTO timer_runtime
                    (timer_id, due_at, paused_at, watch_state, diagnostics)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(timer_id) DO UPDATE SET
                    due_at = excluded.due_at,
                    paused_at = excluded.paused_at,
                    watch_state = excluded.watch_state,
                    diagnostics = excluded.diagnostics",
                (
                    timer_id,
                    runtime.due_at,
                    runtime.paused_at,
                    watch_state,
                    diagnostics,
                ),
            )
            .map_err(persistence)?;
        Ok(())
    }

    fn get_runtime(&self, timer_id: TimerId) -> TimerResult<Option<TimerRuntime>> {
        let row = {
            let mut statement = self
                .registry
                .store()
                .connection()
                .prepare(
                    "SELECT due_at, paused_at, watch_state, diagnostics
                     FROM timer_runtime WHERE timer_id = ?1",
                )
                .map_err(persistence)?;
            let mut rows = statement.query([timer_id]).map_err(persistence)?;
            rows.next()
                .map_err(persistence)?
                .map(|row| {
                    Ok::<_, TimerError>((
                        row.get::<_, i64>(0).map_err(persistence)?,
                        row.get::<_, Option<i64>>(1).map_err(persistence)?,
                        row.get::<_, String>(2).map_err(persistence)?,
                        row.get::<_, String>(3).map_err(persistence)?,
                    ))
                })
                .transpose()?
        };
        let Some((due_at, paused_at, watch_state, diagnostics)) = row else {
            return Ok(None);
        };
        let watch_state = serde_json::from_str(&watch_state)
            .map_err(|error| TimerError::Persistence(error.to_string()))?;
        let diagnostics = serde_json::from_str(&diagnostics)
            .map_err(|error| TimerError::Persistence(error.to_string()))?;
        Ok(Some(TimerRuntime {
            due_at,
            paused_at,
            watch_state,
            diagnostics,
        }))
    }

    fn runtime_or_reconstruct(&mut self, timer: &Timer, _now_ms: i64) -> TimerResult<TimerRuntime> {
        if let Some(runtime) = self.get_runtime(timer.id)? {
            return Ok(runtime);
        }
        let due_at = timer.max_wait_deadline.unwrap_or_else(|| {
            timer
                .created_at
                .saturating_add(timer.interval_ms.unwrap_or(0).max(0))
        });
        let mut watch_state = BTreeMap::new();
        let mut already_idle = Vec::new();
        for process_id in &timer.watch_process_ids {
            let idle = self.process_is_idle(*process_id)?;
            if idle {
                already_idle.push(*process_id);
            }
            watch_state.insert(
                *process_id,
                WatchProgress::new(idle, timer.kind == TimerKind::IdleAll && idle),
            );
        }
        let runtime = TimerRuntime {
            due_at,
            paused_at: None,
            watch_state,
            diagnostics: TimerDiagnostics {
                already_idle,
                ..TimerDiagnostics::default()
            },
        };
        self.put_runtime(timer.id, &runtime)?;
        Ok(runtime)
    }

    fn pending_timer_ids(&self) -> TimerResult<Vec<TimerId>> {
        let mut statement = self
            .registry
            .store()
            .connection()
            .prepare(
                "SELECT timer.id
                 FROM timers AS timer
                 LEFT JOIN timer_runtime AS runtime ON runtime.timer_id = timer.id
                 WHERE timer.paused = 0 AND timer.fired = 0
                 ORDER BY COALESCE(runtime.due_at, timer.max_wait_deadline, timer.created_at), timer.id",
            )
            .map_err(persistence)?;
        let rows = statement
            .query_map([], |row| row.get(0))
            .map_err(persistence)?;
        let mut timer_ids = Vec::new();
        for row in rows {
            timer_ids.push(row.map_err(persistence)?);
        }
        Ok(timer_ids)
    }

    fn has_pending_timers(&self) -> TimerResult<bool> {
        self.registry
            .store()
            .connection()
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM timers WHERE paused = 0 AND fired = 0)",
                [],
                |row| row.get(0),
            )
            .map_err(persistence)
    }

    fn process_is_idle(&mut self, process_id: ProcessId) -> TimerResult<bool> {
        match self.registry.get_status(process_id) {
            Ok(status) => Ok(!self.registry.has_pending_prompts(process_id)
                && matches!(
                    status.agent_state.state,
                    AttentionState::Idle | AttentionState::Waiting
                )),
            Err(RegistryError::NotFound(_)) => Ok(false),
            Err(error) => Err(error.into()),
        }
    }

    fn advance_idle_state(
        &mut self,
        timer: &Timer,
        runtime: &mut TimerRuntime,
        now_ms: i64,
    ) -> TimerResult<IdleAdvance> {
        let mut changed = false;
        let mut transitioned_process_ids = Vec::new();
        for process_id in &timer.watch_process_ids {
            let idle = self.process_is_idle(*process_id)?;
            let progress = runtime
                .watch_state
                .entry(*process_id)
                .or_insert(WatchProgress::new(
                    idle,
                    timer.kind == TimerKind::IdleAll && idle,
                ));
            let before = progress.clone();
            // A new prompt invalidates an earlier completion while idle_all is
            // still waiting for the other children (or wake delivery is retrying).
            if self.registry.has_pending_prompts(*process_id) {
                *progress = WatchProgress::new(false, false);
                runtime
                    .diagnostics
                    .satisfied_by
                    .retain(|satisfaction| satisfaction.process_id != Some(*process_id));
            }
            advance_watch_progress(progress, idle);
            changed |= *progress != before;
            if !before.satisfied && progress.satisfied {
                let completion = CompletionLedger::new(self.registry.store())
                    .latest_completion(*process_id)?
                    .filter(|completion| completion.completed_at_ms >= timer.created_at);
                progress.completion_id = completion.map(|completion| completion.id);
                runtime.diagnostics.record_satisfaction(TimerSatisfaction {
                    process_id: Some(*process_id),
                    completed_at_ms: Some(
                        completion.map_or(now_ms, |completion| completion.completed_at_ms),
                    ),
                    reason: TimerSatisfactionReason::FreshTransition,
                });
                transitioned_process_ids.push(*process_id);
                changed = true;
            }
        }
        Ok(IdleAdvance {
            changed,
            transitioned_process_ids,
        })
    }
}

struct IdleAdvance {
    changed: bool,
    transitioned_process_ids: Vec<ProcessId>,
}

pub(crate) fn advance_watch_progress(progress: &mut WatchProgress, idle: bool) {
    if !progress.satisfied {
        if !progress.armed {
            // idle_any ignores a process that was already idle until it first
            // becomes active and arms a subsequent idle transition.
            if !idle {
                progress.armed = true;
            }
        } else if idle && !progress.last_idle {
            progress.satisfied = true;
        }
    }
    progress.last_idle = idle;
}

impl TimerView {
    fn new(
        timer: Timer,
        owner_process_name: Option<String>,
        owner_label: String,
        runtime: TimerRuntime,
    ) -> Self {
        Self {
            timer,
            owner_process_name,
            owner_label,
            due_at: runtime.due_at,
            paused_at: runtime.paused_at,
            already_idle: runtime.diagnostics.already_idle,
            satisfied_by: runtime.diagnostics.satisfied_by,
            fire_reason: runtime.diagnostics.fire_reason,
        }
    }
}

impl TimerDiagnostics {
    fn record_satisfaction(&mut self, satisfaction: TimerSatisfaction) {
        self.satisfied_by
            .retain(|current| current.process_id != satisfaction.process_id);
        self.satisfied_by.push(satisfaction);
    }

    fn record_deadline(&mut self) {
        self.fire_reason = Some(TimerSatisfactionReason::Deadline);
        if !self
            .satisfied_by
            .iter()
            .any(|satisfaction| satisfaction.reason == TimerSatisfactionReason::Deadline)
        {
            self.satisfied_by.push(TimerSatisfaction {
                process_id: None,
                completed_at_ms: None,
                reason: TimerSatisfactionReason::Deadline,
            });
        }
    }

    fn record_error(&mut self) {
        self.fire_reason = Some(TimerSatisfactionReason::Error);
        self.satisfied_by.clear();
        self.satisfied_by.push(TimerSatisfaction {
            process_id: None,
            completed_at_ms: None,
            reason: TimerSatisfactionReason::Error,
        });
    }
}

fn idle_condition_satisfied(timer: &Timer, runtime: &TimerRuntime) -> bool {
    match timer.kind {
        TimerKind::IdleAny => runtime
            .watch_state
            .values()
            .any(|progress| progress.satisfied),
        TimerKind::IdleAll => {
            !runtime.watch_state.is_empty()
                && runtime
                    .watch_state
                    .values()
                    .all(|progress| progress.satisfied)
        }
        TimerKind::Delay => false,
    }
}

fn persistence(error: impl fmt::Display) -> TimerError {
    TimerError::Persistence(error.to_string())
}

pub(crate) fn spawn_timer_scheduler(
    registry: SharedProcessRegistry,
    events: TimerLifecycleHub,
    mut shutdown: watch::Receiver<bool>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = interval(TIMER_POLL_INTERVAL);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
        let mut timer_activity = events.subscribe();
        let mut has_pending_timers = {
            let mut registry = registry.lock().await;
            TimerService::new(&mut registry).has_pending_timers()
        }
        .unwrap_or(true);
        loop {
            if !has_pending_timers {
                tokio::select! {
                    changed = shutdown.changed() => {
                        if changed.is_err() || *shutdown.borrow() {
                            break;
                        }
                    }
                    changed = timer_activity.changed() => {
                        if changed.is_err() {
                            break;
                        }
                        let mut registry = registry.lock().await;
                        has_pending_timers = TimerService::new(&mut registry)
                            .has_pending_timers()
                            .unwrap_or(true);
                    }
                }
                continue;
            }

            tokio::select! {
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        break;
                    }
                }
                changed = timer_activity.changed() => {
                    if changed.is_err() {
                        break;
                    }
                    let mut registry = registry.lock().await;
                    has_pending_timers = TimerService::new(&mut registry)
                        .has_pending_timers()
                        .unwrap_or(true);
                }
                _ = ticker.tick() => {
                    let mut registry = registry.lock().await;
                    if let Ok(fires) = TimerService::new(&mut registry).tick(now_millis()) {
                        if !fires.is_empty() {
                            has_pending_timers = TimerService::new(&mut registry)
                                .has_pending_timers()
                                .unwrap_or(true);
                        }
                        for fire in fires {
                            events.publish(TimerLifecycleEvent::for_timer(
                                TimerLifecycleKind::Fired,
                                fire.project_id,
                                fire.timer.clone(),
                                fire.fired_at,
                                Some(fire.reason),
                            ));
                            events.publish(TimerLifecycleEvent::for_timer(
                                TimerLifecycleKind::Delivered,
                                fire.project_id,
                                fire.timer,
                                fire.fired_at,
                                Some(fire.reason),
                            ));
                        }
                    }
                }
            }
        }
    })
}

pub(crate) fn now_millis() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};

    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, thread, time::Instant};

    use workman_core::{
        Actor, AgentTool, Process, ProcessKind, ProcessSource, ProcessStatus, Project, Store,
    };

    use super::*;

    const PROJECT_ID: ProjectId = 1;
    const DELIVERY_ID: ProcessId = 10;
    const WORKER_ID: ProcessId = 11;
    #[cfg(unix)]
    const PASTE_TUI_ID: ProcessId = 13;

    #[cfg(unix)]
    fn paste_sensitive_tui() -> &'static str {
        r#"true claude; stty raw -echo; printf '\033[?2004h❯ '; exec perl -e '$|=1; my $draft=""; my $enters=0; while (1) { my $n = sysread(STDIN, my $chunk, 4096); exit 2 unless defined($n) && $n > 0; my $redraw=0; for my $character (split //, $chunk) { if ($character eq "\r") { $enters++; next if $enters == 1; print "\r\nSUBMITTED\r\nthinking...\r\nesc to interrupt\r\n"; sleep 5; exit 0; } $draft .= $character; $redraw=1; } print "\r\e[2K❯ DRAFT:$draft" if $redraw; }'"#
    }

    fn test_registry(start_worker: bool) -> ProcessRegistry {
        let store = Store::open_in_memory().unwrap();
        store
            .put_project(&Project {
                id: PROJECT_ID,
                path: "/tmp".into(),
                name: "timers".into(),
                display_name: None,
                icon: None,
                selected: false,
                sort_order: 0,
            })
            .unwrap();
        store
            .put_agent_tool(&AgentTool {
                id: 90,
                name: "Scripted Timer Claude".into(),
                command: "scripted-timer-claude".into(),
                tool_type: "claude_code".into(),
                enabled: true,
                source: workman_core::AgentToolSource::Local,
                resume_args: None,
                continue_args: None,
            })
            .unwrap();
        store
            .put_agent_tool(&AgentTool {
                id: 91,
                name: "Scripted Timer Kimi".into(),
                command: "scripted-timer-kimi".into(),
                tool_type: "kimi".into(),
                enabled: true,
                source: workman_core::AgentToolSource::Local,
                resume_args: None,
                continue_args: None,
            })
            .unwrap();
        let mut registry =
            ProcessRegistry::with_stop_grace_for_test(store, Duration::from_millis(100)).unwrap();
        registry
            .create(process(
                DELIVERY_ID,
                "delivery",
                "while IFS= read -r line; do printf 'received:[%s]\\n' \"$line\"; done",
                None,
            ))
            .unwrap();
        registry.start(DELIVERY_ID).unwrap();

        if start_worker {
            registry.create(process(
                WORKER_ID,
                "worker",
                "printf '❯\\n'; while IFS= read -r line; do if [ \"$line\" = go ]; then printf 'thinking...\\nesc to interrupt\\n'; sleep 0.7; printf '❯\\n'; fi; done",
                Some(90),
            )).unwrap();
            registry.start(WORKER_ID).unwrap();
        }
        registry
    }

    fn process(id: ProcessId, name: &str, command: &str, agent_tool_id: Option<i64>) -> Process {
        Process {
            id,
            project_id: PROJECT_ID,
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
            agent_tool_id,
            spawned_by_process_id: None,
            sort_order: 0,
        }
    }

    fn put_actor(registry: &ProcessRegistry, actor_id: &str, process_id: ProcessId) {
        registry
            .store()
            .put_actor(&Actor {
                id: actor_id.into(),
                session_id: format!("{actor_id}-session"),
                process_id: Some(process_id),
                selected_project_id: Some(PROJECT_ID),
                created_at: 1_000,
                last_seen_at: 1_000,
            })
            .unwrap();
    }

    fn wait_for_state(
        registry: &mut ProcessRegistry,
        process_id: ProcessId,
        expected: AttentionState,
    ) {
        let deadline = Instant::now() + Duration::from_secs(15);
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
        let deadline = Instant::now() + Duration::from_secs(10);
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
    #[test]
    fn delayed_timer_reloads_from_sqlite_and_injects_body_verbatim() {
        let mut registry = test_registry(false);
        let timer_id = TimerService::new(&mut registry)
            .set_delay(
                "actor-delay".into(),
                DELIVERY_ID,
                "wake $(verbatim) [x]".into(),
                50,
                false,
                None,
                1_000,
            )
            .unwrap()
            .timer
            .id;

        // A new service has no in-memory schedule and reloads the pending row.
        assert!(
            TimerService::new(&mut registry)
                .tick(1_049)
                .unwrap()
                .is_empty()
        );
        let fired = TimerService::new(&mut registry).tick(1_050).unwrap();
        assert_eq!(fired.len(), 1);
        assert_eq!(fired[0].timer_id, timer_id);
        assert_eq!(fired[0].reason, TimerFireReason::Delay);
        wait_for_output(
            &mut registry,
            DELIVERY_ID,
            "received:[wake $(verbatim) [x]]",
        );

        let timers = TimerService::new(&mut registry)
            .list(PROJECT_ID, 10, 1_050)
            .unwrap();
        assert!(timers[0].timer.fired);
    }

    #[test]
    fn agent_timers_reject_cross_project_targets_and_quarantine_legacy_rows() {
        const FOREIGN_PROJECT_ID: ProjectId = 2;
        const FOREIGN_PROCESS_ID: ProcessId = 30;
        let mut registry = test_registry(false);
        registry
            .store()
            .put_project(&Project {
                id: FOREIGN_PROJECT_ID,
                path: "/tmp/foreign".into(),
                name: "foreign".into(),
                display_name: None,
                icon: None,
                selected: false,
                sort_order: 1,
            })
            .unwrap();
        let mut foreign = process(FOREIGN_PROCESS_ID, "foreign-agent", "sleep 30", None);
        foreign.project_id = FOREIGN_PROJECT_ID;
        registry.create(foreign).unwrap();
        registry
            .store()
            .put_actor(&Actor {
                id: "jailed-timer-owner".into(),
                session_id: "jailed-timer-session".into(),
                process_id: Some(DELIVERY_ID),
                selected_project_id: Some(PROJECT_ID),
                created_at: 1_000,
                last_seen_at: 1_000,
            })
            .unwrap();

        let delivery_error = TimerService::new(&mut registry)
            .set_delay(
                "jailed-timer-owner".into(),
                FOREIGN_PROCESS_ID,
                "must not deliver".into(),
                1,
                false,
                None,
                1_000,
            )
            .unwrap_err();
        assert!(matches!(
            delivery_error,
            TimerError::CrossProjectTarget {
                target_process_id: FOREIGN_PROCESS_ID,
                ..
            }
        ));

        let watch_error = TimerService::new(&mut registry)
            .set_idle(
                "jailed-timer-owner".into(),
                DELIVERY_ID,
                "must not watch".into(),
                TimerKind::IdleAny,
                vec![FOREIGN_PROCESS_ID],
                1_000,
                1_000,
            )
            .unwrap_err();
        assert!(matches!(
            watch_error,
            TimerError::CrossProjectTarget {
                target_process_id: FOREIGN_PROCESS_ID,
                ..
            }
        ));

        registry
            .store()
            .put_timer(&Timer {
                id: 999,
                owner_actor: "jailed-timer-owner".into(),
                owner_process_id: Some(DELIVERY_ID),
                delivery_process_id: FOREIGN_PROCESS_ID,
                body: "legacy escape".into(),
                kind: TimerKind::Delay,
                watch_process_ids: Vec::new(),
                interval_ms: None,
                repeating: false,
                max_wait_deadline: Some(1_000),
                paused: false,
                fired: false,
                fired_at: None,
                created_at: 900,
            })
            .unwrap();
        assert!(
            TimerService::new(&mut registry)
                .tick(1_001)
                .unwrap()
                .is_empty()
        );
        let quarantined = registry.store().get_timer(999).unwrap().unwrap();
        assert!(quarantined.fired);
        assert_eq!(quarantined.fired_at, Some(1_001));
    }

    #[cfg(unix)]
    #[test]
    fn typing_pause_holds_due_and_already_satisfied_wakeups_on_a_real_pty() {
        for immediate in [false, true] {
            let mut registry = test_registry(false);
            registry
                .create(process(
                    PASTE_TUI_ID,
                    "typing-agent",
                    paste_sensitive_tui(),
                    Some(90),
                ))
                .unwrap();
            registry.start(PASTE_TUI_ID).unwrap();
            wait_for_state(&mut registry, PASTE_TUI_ID, AttentionState::Idle);
            let router = registry.input_router();
            router.set_typing_pause(crate::settings::TypingPauseSettings {
                enabled: true,
                delay_ms: 60_000,
            });
            router
                .send_terminal_input(PASTE_TUI_ID, b"partial ", true)
                .unwrap();
            wait_for_output(&mut registry, PASTE_TUI_ID, "DRAFT:partial");
            if immediate {
                let outcome = TimerService::new(&mut registry)
                    .set_idle(
                        "typing-test".into(),
                        PASTE_TUI_ID,
                        "WAKEUP".into(),
                        TimerKind::IdleAll,
                        vec![PASTE_TUI_ID],
                        10_000,
                        1_000,
                    )
                    .unwrap();
                assert!(matches!(outcome, IdleTimerOutcome::AlreadySatisfied { .. }));
            } else {
                TimerService::new(&mut registry)
                    .set_delay(
                        "typing-test".into(),
                        PASTE_TUI_ID,
                        "WAKEUP".into(),
                        0,
                        false,
                        None,
                        1_000,
                    )
                    .unwrap();
                assert_eq!(
                    TimerService::new(&mut registry).tick(1_000).unwrap().len(),
                    1
                );
            }
            assert!(registry.has_pending_prompts(PASTE_TUI_ID));
            std::thread::sleep(Duration::from_millis(50));
            let output = registry.raw_output(PASTE_TUI_ID, None, usize::MAX).unwrap();
            assert!(!String::from_utf8_lossy(&output.data).contains("WAKEUP"));
            // The preference applies to already queued delivery, without restarting the PTY.
            router.set_typing_pause(crate::settings::TypingPauseSettings {
                enabled: false,
                delay_ms: 60_000,
            });
            wait_for_output(&mut registry, PASTE_TUI_ID, "SUBMITTED");
            let output = registry.raw_output(PASTE_TUI_ID, None, usize::MAX).unwrap();
            assert!(String::from_utf8_lossy(&output.data).contains("WAKEUP"));
        }
    }

    #[cfg(unix)]
    #[test]
    fn short_timer_body_submits_outside_the_paste_burst_on_a_real_pty() {
        let mut registry = test_registry(false);
        registry
            .create(process(
                PASTE_TUI_ID,
                "paste-sensitive-agent",
                paste_sensitive_tui(),
                Some(90),
            ))
            .unwrap();
        registry.start(PASTE_TUI_ID).unwrap();
        wait_for_state(&mut registry, PASTE_TUI_ID, AttentionState::Idle);

        let body = "Reply with exactly WOKE.";
        assert!(body.len() < 100);
        TimerService::new(&mut registry)
            .set_delay(
                "actor-paste".into(),
                PASTE_TUI_ID,
                body.into(),
                0,
                false,
                None,
                1_000,
            )
            .unwrap();
        assert_eq!(
            TimerService::new(&mut registry).tick(1_000).unwrap().len(),
            1
        );
        wait_for_output(&mut registry, PASTE_TUI_ID, "SUBMITTED");
        let status = registry.get_status(PASTE_TUI_ID).unwrap();
        assert_eq!(status.agent_state.state, AttentionState::Working);
        assert!(
            status
                .events
                .iter()
                .any(|event| event.kind == "submit_retry"),
            "draft-visible-but-idle must trigger a verified bare-CR retry"
        );
    }

    #[cfg(unix)]
    #[test]
    fn already_satisfied_idle_all_delivers_immediately_to_a_real_pty() {
        let mut registry = test_registry(false);
        registry
            .create(process(
                PASTE_TUI_ID,
                "paste-sensitive-agent",
                paste_sensitive_tui(),
                Some(90),
            ))
            .unwrap();
        registry.start(PASTE_TUI_ID).unwrap();
        wait_for_state(&mut registry, PASTE_TUI_ID, AttentionState::Idle);

        let body = "Already idle: wake now.";
        assert!(body.len() < 100);
        let outcome = TimerService::new(&mut registry)
            .set_idle(
                "actor-immediate".into(),
                PASTE_TUI_ID,
                body.into(),
                TimerKind::IdleAll,
                vec![PASTE_TUI_ID],
                10_000,
                2_000,
            )
            .unwrap();
        assert!(matches!(
            outcome,
            IdleTimerOutcome::AlreadySatisfied {
                delivery_process_id: PASTE_TUI_ID,
                delivered_at: 2_000,
                ..
            }
        ));
        wait_for_output(&mut registry, PASTE_TUI_ID, "SUBMITTED");
        let status = registry.get_status(PASTE_TUI_ID).unwrap();
        assert_eq!(status.agent_state.state, AttentionState::Working);
        assert!(
            status
                .events
                .iter()
                .any(|event| event.kind == "submit_retry"),
            "already-satisfied delivery must recover a visible idle draft"
        );
        assert!(
            TimerService::new(&mut registry)
                .list(PROJECT_ID, 10, 2_000)
                .unwrap()
                .is_empty()
        );
    }

    #[cfg(unix)]
    #[test]
    fn pending_timer_refines_idle_to_waiting_until_delivery_starts_work() {
        let mut registry = test_registry(false);
        registry
            .create(process(
                PASTE_TUI_ID,
                "parked-agent",
                paste_sensitive_tui(),
                Some(90),
            ))
            .unwrap();
        registry.start(PASTE_TUI_ID).unwrap();
        wait_for_state(&mut registry, PASTE_TUI_ID, AttentionState::Idle);

        let now = now_millis();
        let timer_id = TimerService::new(&mut registry)
            .set_delay(
                "parked-actor".into(),
                PASTE_TUI_ID,
                "wake parked agent".into(),
                100,
                false,
                None,
                now,
            )
            .unwrap()
            .timer
            .id;
        let waiting_status = registry.get_status(PASTE_TUI_ID).unwrap();
        let payload = serde_json::to_value(&waiting_status).unwrap();
        let waiting = waiting_status.agent_state;
        assert_eq!(waiting.state, AttentionState::Waiting);
        assert!(waiting.waiting);
        assert!(waiting.idle, "waiting must retain idle compatibility");
        assert!(!waiting.needs_input);
        assert_eq!(waiting.waiting_on.len(), 1);
        assert_eq!(waiting.waiting_on[0].timer_id, timer_id);
        assert!(waiting.waiting_on[0].remaining_ms <= 100);
        assert_eq!(payload["agent_state"]["state"], "waiting");
        assert_eq!(payload["agent_state"]["waiting"], true);
        assert_eq!(
            payload["agent_state"]["waiting_on"][0]["timer_id"],
            timer_id
        );

        assert_eq!(
            TimerService::new(&mut registry)
                .tick(now.saturating_add(100))
                .unwrap()
                .len(),
            1
        );
        let working = registry.get_status(PASTE_TUI_ID).unwrap().agent_state;
        assert_eq!(working.state, AttentionState::Working);
        assert!(!working.waiting);
        assert!(working.waiting_on.is_empty());
        wait_for_output(&mut registry, PASTE_TUI_ID, "SUBMITTED");
    }

    #[cfg(unix)]
    #[test]
    fn idle_watch_owner_is_waiting_even_when_delivery_targets_another_process() {
        let mut registry = test_registry(true);
        registry
            .create(process(
                PASTE_TUI_ID,
                "orchestrator-agent",
                "true claude; printf '❯ '; sleep 30",
                Some(90),
            ))
            .unwrap();
        registry.start(PASTE_TUI_ID).unwrap();
        wait_for_state(&mut registry, PASTE_TUI_ID, AttentionState::Idle);
        wait_for_state(&mut registry, WORKER_ID, AttentionState::Idle);
        registry
            .store()
            .put_actor(&Actor {
                id: "watch-owner".into(),
                session_id: "watch-session".into(),
                process_id: Some(PASTE_TUI_ID),
                selected_project_id: Some(PROJECT_ID),
                created_at: 1_000,
                last_seen_at: 1_000,
            })
            .unwrap();

        let outcome = TimerService::new(&mut registry)
            .set_idle(
                "watch-owner".into(),
                DELIVERY_ID,
                "other delivery".into(),
                TimerKind::IdleAny,
                vec![WORKER_ID],
                10_000,
                1_000,
            )
            .unwrap();
        assert!(matches!(outcome, IdleTimerOutcome::Created(_)));
        let waiting = registry.get_status(PASTE_TUI_ID).unwrap().agent_state;
        assert_eq!(waiting.state, AttentionState::Waiting);
        assert_eq!(waiting.waiting_on[0].kind, TimerKind::IdleAny);
        assert_eq!(
            waiting.waiting_on[0].watch_processes[0].process_name,
            "worker"
        );
    }

    #[test]
    fn pending_timer_never_overrides_needs_input() {
        const DIALOG_ID: ProcessId = 14;
        let mut registry = test_registry(false);
        registry
            .create(process(
                DIALOG_ID,
                "permission-agent",
                "printf 'Do you want to proceed?\\n❯ 1. Yes, allow\\n  2. No, and tell Claude\\n'; sleep 30",
                Some(90),
            ))
            .unwrap();
        registry.start(DIALOG_ID).unwrap();
        wait_for_state(&mut registry, DIALOG_ID, AttentionState::NeedsInput);
        TimerService::new(&mut registry)
            .set_delay(
                "permission-owner".into(),
                DIALOG_ID,
                "do not hide the dialog".into(),
                10_000,
                false,
                None,
                1_000,
            )
            .unwrap();

        let status = registry.get_status(DIALOG_ID).unwrap().agent_state;
        assert_eq!(status.state, AttentionState::NeedsInput);
        assert!(status.needs_input);
        assert!(!status.waiting);
        assert!(!status.idle);
        assert_eq!(status.waiting_on.len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn recently_prompted_process_does_not_satisfy_idle_all_before_output() {
        let mut registry = test_registry(true);
        wait_for_state(&mut registry, WORKER_ID, AttentionState::Idle);

        registry.send_input(WORKER_ID, b"go\r").unwrap();
        assert_eq!(
            registry.get_status(WORKER_ID).unwrap().agent_state.state,
            AttentionState::Working
        );
        let outcome = TimerService::new(&mut registry)
            .set_idle(
                "actor-race".into(),
                DELIVERY_ID,
                "after real completion".into(),
                TimerKind::IdleAll,
                vec![WORKER_ID],
                10_000,
                now_millis(),
            )
            .unwrap();
        assert!(matches!(outcome, IdleTimerOutcome::Created(_)));
    }

    #[cfg(unix)]
    #[test]
    fn pending_timer_survives_store_and_registry_reopen() {
        let temp = tempfile::tempdir().unwrap();
        let database = temp.path().join("workman.db");
        {
            let store = Store::open(&database).unwrap();
            store
                .put_project(&Project {
                    id: PROJECT_ID,
                    path: "/tmp".into(),
                    name: "timers-restart".into(),
                    display_name: None,
                    icon: None,
                    selected: false,
                    sort_order: 0,
                })
                .unwrap();
            let mut registry =
                ProcessRegistry::with_stop_grace_for_test(store, Duration::from_millis(100))
                    .unwrap();
            registry
                .create(process(
                    DELIVERY_ID,
                    "delivery",
                    "while IFS= read -r line; do printf 'received:[%s]\\n' \"$line\"; done",
                    None,
                ))
                .unwrap();
            registry.start(DELIVERY_ID).unwrap();
            TimerService::new(&mut registry)
                .set_delay(
                    "actor-restart".into(),
                    DELIVERY_ID,
                    "after daemon restart".into(),
                    1_000,
                    false,
                    None,
                    1_000,
                )
                .unwrap();
        }

        let store = Store::open(&database).unwrap();
        let mut registry =
            ProcessRegistry::with_stop_grace_for_test(store, Duration::from_millis(100)).unwrap();
        registry.start(DELIVERY_ID).unwrap();
        assert!(
            TimerService::new(&mut registry)
                .tick(1_999)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            TimerService::new(&mut registry).tick(2_000).unwrap().len(),
            1
        );
        wait_for_output(
            &mut registry,
            DELIVERY_ID,
            "received:[after daemon restart]",
        );
    }

    #[cfg(unix)]
    #[test]
    fn unreported_and_reported_completions_survive_registry_reopen() {
        let temp = tempfile::tempdir().unwrap();
        let database = temp.path().join("workman.db");
        {
            let store = Store::open(&database).unwrap();
            store
                .put_project(&Project {
                    id: PROJECT_ID,
                    path: "/tmp".into(),
                    name: "completion-restart".into(),
                    display_name: None,
                    icon: None,
                    selected: false,
                    sort_order: 0,
                })
                .unwrap();
            store
                .put_agent_tool(&AgentTool {
                    id: 90,
                    name: "Restart Timer Claude".into(),
                    command: "restart-timer-claude".into(),
                    tool_type: "claude_code".into(),
                    enabled: true,
                    source: workman_core::AgentToolSource::Local,
                    resume_args: None,
                    continue_args: None,
                })
                .unwrap();
            let mut registry =
                ProcessRegistry::with_stop_grace_for_test(store, Duration::from_millis(100))
                    .unwrap();
            registry
                .create(process(
                    DELIVERY_ID,
                    "delivery",
                    "while IFS= read -r line; do printf 'received:[%s]\\n' \"$line\"; done",
                    None,
                ))
                .unwrap();
            registry
                .create(process(
                    WORKER_ID,
                    "worker",
                    "printf '❯\\n'; while IFS= read -r line; do if [ \"$line\" = go ]; then printf 'thinking...\\nesc to interrupt\\n'; sleep 0.7; printf '❯\\n'; fi; done",
                    Some(90),
                ))
                .unwrap();
            registry
                .store()
                .put_actor(&Actor {
                    id: "restart-owner".into(),
                    session_id: "restart-owner-session".into(),
                    process_id: Some(DELIVERY_ID),
                    selected_project_id: Some(PROJECT_ID),
                    created_at: 1_000,
                    last_seen_at: 1_000,
                })
                .unwrap();
            registry.start(DELIVERY_ID).unwrap();
            registry.start(WORKER_ID).unwrap();
            wait_for_state(&mut registry, WORKER_ID, AttentionState::Idle);
            registry.send_input(WORKER_ID, b"go\r").unwrap();
            CompletionLedger::new(registry.store())
                .record_input(DELIVERY_ID, WORKER_ID, now_millis())
                .unwrap();
            wait_for_state(&mut registry, WORKER_ID, AttentionState::Working);
            wait_for_state(&mut registry, WORKER_ID, AttentionState::Idle);
            assert_eq!(
                CompletionLedger::new(registry.store())
                    .unreported_completions(DELIVERY_ID, &[WORKER_ID])
                    .unwrap()
                    .len(),
                1
            );
        }

        {
            let store = Store::open(&database).unwrap();
            let mut registry =
                ProcessRegistry::with_stop_grace_for_test(store, Duration::from_millis(100))
                    .unwrap();
            registry.start(DELIVERY_ID).unwrap();
            registry.start(WORKER_ID).unwrap();
            wait_for_state(&mut registry, WORKER_ID, AttentionState::Idle);
            let outcome = TimerService::new(&mut registry)
                .set_idle(
                    "restart-owner".into(),
                    DELIVERY_ID,
                    "completion survived restart".into(),
                    TimerKind::IdleAny,
                    vec![WORKER_ID],
                    10_000,
                    now_millis(),
                )
                .unwrap();
            assert!(matches!(outcome, IdleTimerOutcome::AlreadySatisfied { .. }));
        }

        let store = Store::open(&database).unwrap();
        let mut registry =
            ProcessRegistry::with_stop_grace_for_test(store, Duration::from_millis(100)).unwrap();
        registry.start(DELIVERY_ID).unwrap();
        registry.start(WORKER_ID).unwrap();
        wait_for_state(&mut registry, WORKER_ID, AttentionState::Idle);
        let outcome = TimerService::new(&mut registry)
            .set_idle(
                "restart-owner".into(),
                DELIVERY_ID,
                "reported completion must not refire".into(),
                TimerKind::IdleAny,
                vec![WORKER_ID],
                10_000,
                now_millis(),
            )
            .unwrap();
        assert!(matches!(outcome, IdleTimerOutcome::Created(_)));
    }

    #[test]
    fn repeating_delay_reschedules_from_the_requested_interval() {
        let mut registry = test_registry(false);
        let timer_id = TimerService::new(&mut registry)
            .set_delay(
                "actor-repeat".into(),
                DELIVERY_ID,
                "repeat wake".into(),
                10,
                false,
                Some(20),
                100,
            )
            .unwrap()
            .timer
            .id;

        assert_eq!(TimerService::new(&mut registry).tick(110).unwrap().len(), 1);
        assert!(
            TimerService::new(&mut registry)
                .tick(129)
                .unwrap()
                .is_empty()
        );
        assert_eq!(TimerService::new(&mut registry).tick(130).unwrap().len(), 1);
        let timer = TimerService::new(&mut registry)
            .list(PROJECT_ID, 1, 130)
            .unwrap()
            .pop()
            .unwrap();
        assert_eq!(timer.timer.id, timer_id);
        assert!(!timer.timer.fired);
        assert_eq!(timer.due_at, 150);
    }

    #[cfg(unix)]
    #[test]
    fn pause_resume_preserves_remaining_delay_and_cancel_deletes() {
        let mut registry = test_registry(false);
        let timer_id = TimerService::new(&mut registry)
            .set_delay(
                "actor-control".into(),
                DELIVERY_ID,
                "resumed timer".into(),
                100,
                false,
                None,
                100,
            )
            .unwrap()
            .timer
            .id;
        TimerService::new(&mut registry)
            .pause("actor-control", PROJECT_ID, timer_id, 150)
            .unwrap();
        assert!(
            TimerService::new(&mut registry)
                .tick(1_000)
                .unwrap()
                .is_empty()
        );
        let resumed = TimerService::new(&mut registry)
            .resume("actor-control", PROJECT_ID, timer_id, 500)
            .unwrap();
        assert_eq!(resumed.due_at, 550);
        assert!(
            TimerService::new(&mut registry)
                .tick(549)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            TimerService::new(&mut registry).tick(550).unwrap()[0].timer_id,
            timer_id
        );
        wait_for_output(&mut registry, DELIVERY_ID, "received:[resumed timer]");

        let cancelled_id = TimerService::new(&mut registry)
            .set_delay(
                "actor-control".into(),
                DELIVERY_ID,
                "must not fire".into(),
                10,
                false,
                None,
                1_000,
            )
            .unwrap()
            .timer
            .id;
        TimerService::new(&mut registry)
            .cancel("actor-control", PROJECT_ID, cancelled_id)
            .unwrap();
        assert!(registry.store().get_timer(cancelled_id).unwrap().is_none());
    }

    #[cfg(unix)]
    #[test]
    fn idle_any_consumes_unseen_completion_once_then_waits_for_another_worker() {
        const SECOND_WORKER_ID: ProcessId = 12;

        let mut registry = test_registry(true);
        registry
            .create(process(
                SECOND_WORKER_ID,
                "second-worker",
                "printf '❯\n'; while IFS= read -r line; do if [ \"$line\" = go ]; then printf 'thinking...\\nesc to interrupt\\n'; sleep 0.7; printf '❯\\n'; fi; done",
                Some(90),
            ))
            .unwrap();
        registry.start(SECOND_WORKER_ID).unwrap();
        for (actor_id, owner_process_id) in [
            ("completion-owner", DELIVERY_ID),
            ("never-prompted-owner", SECOND_WORKER_ID),
        ] {
            registry
                .store()
                .put_actor(&Actor {
                    id: actor_id.into(),
                    session_id: format!("{actor_id}-session"),
                    process_id: Some(owner_process_id),
                    selected_project_id: Some(PROJECT_ID),
                    created_at: 1_000,
                    last_seen_at: 1_000,
                })
                .unwrap();
        }
        wait_for_state(&mut registry, WORKER_ID, AttentionState::Idle);
        wait_for_state(&mut registry, SECOND_WORKER_ID, AttentionState::Idle);

        registry.send_input(WORKER_ID, b"go\r").unwrap();
        CompletionLedger::new(registry.store())
            .record_input(DELIVERY_ID, WORKER_ID, now_millis())
            .unwrap();
        // Do not inspect status while the turn runs. Work evidence must be captured
        // eagerly from PTY output so the send -> work -> idle -> arm race is durable.
        thread::sleep(Duration::from_millis(6_500));

        let outcome = TimerService::new(&mut registry)
            .set_idle(
                "completion-owner".into(),
                DELIVERY_ID,
                "unseen completion wake".into(),
                TimerKind::IdleAny,
                vec![WORKER_ID],
                10_000,
                now_millis(),
            )
            .unwrap();
        let IdleTimerOutcome::AlreadySatisfied {
            already_idle,
            satisfied_by,
            ..
        } = outcome
        else {
            panic!("idle_any did not consume the unseen completion immediately");
        };
        assert_eq!(already_idle, vec![WORKER_ID]);
        assert_eq!(satisfied_by.len(), 1);
        assert_eq!(satisfied_by[0].process_id, Some(WORKER_ID));
        assert_eq!(
            satisfied_by[0].reason,
            TimerSatisfactionReason::UnseenCompletion
        );
        assert!(satisfied_by[0].completed_at_ms.is_some());
        wait_for_output(
            &mut registry,
            DELIVERY_ID,
            "received:[unseen completion wake]",
        );

        let old_behavior = TimerService::new(&mut registry)
            .set_idle(
                "never-prompted-owner".into(),
                DELIVERY_ID,
                "must wait for a fresh transition".into(),
                TimerKind::IdleAny,
                vec![WORKER_ID],
                10_000,
                now_millis(),
            )
            .unwrap();
        assert!(matches!(old_behavior, IdleTimerOutcome::Created(_)));

        registry.send_input(SECOND_WORKER_ID, b"go\r").unwrap();
        CompletionLedger::new(registry.store())
            .record_input(DELIVERY_ID, SECOND_WORKER_ID, now_millis())
            .unwrap();
        wait_for_state(&mut registry, SECOND_WORKER_ID, AttentionState::Working);
        let no_loop_timer_id = match TimerService::new(&mut registry)
            .set_idle(
                "completion-owner".into(),
                DELIVERY_ID,
                "second worker wake".into(),
                TimerKind::IdleAny,
                vec![WORKER_ID, SECOND_WORKER_ID],
                10_000,
                now_millis(),
            )
            .unwrap()
        {
            IdleTimerOutcome::Created(timer) => {
                assert_eq!(timer.already_idle, vec![WORKER_ID]);
                assert!(timer.satisfied_by.is_empty());
                timer.timer.id
            }
            IdleTimerOutcome::AlreadySatisfied { .. } => {
                panic!("the reported first worker created an immediate-wake loop")
            }
        };
        assert!(
            TimerService::new(&mut registry)
                .tick(now_millis())
                .unwrap()
                .is_empty()
        );
        wait_for_state(&mut registry, SECOND_WORKER_ID, AttentionState::Waiting);
        let fires = TimerService::new(&mut registry).tick(now_millis()).unwrap();
        let fire = fires
            .iter()
            .find(|fire| fire.timer_id == no_loop_timer_id)
            .expect("second worker completion fires the timer");
        assert_eq!(fire.reason, TimerFireReason::IdleTransition);
        assert_eq!(
            fire.timer.fire_reason,
            Some(TimerSatisfactionReason::FreshTransition)
        );
        assert!(fire.timer.satisfied_by.iter().any(|satisfaction| {
            satisfaction.process_id == Some(SECOND_WORKER_ID)
                && satisfaction.reason == TimerSatisfactionReason::FreshTransition
        }));
    }

    #[cfg(unix)]
    #[test]
    fn echo_only_prompt_never_creates_a_durable_completion() {
        const ECHO_ID: ProcessId = 15;

        let mut registry = test_registry(false);
        registry
            .create(process(
                ECHO_ID,
                "echo-only",
                r#"printf '❯\n'; while IFS= read -r line; do printf '> %s\n❯\n' "$line"; done"#,
                Some(90),
            ))
            .unwrap();
        registry.start(ECHO_ID).unwrap();
        put_actor(&registry, "echo-owner", DELIVERY_ID);
        wait_for_state(&mut registry, ECHO_ID, AttentionState::Idle);

        registry.submit_input(ECHO_ID, b"noop").unwrap();
        CompletionLedger::new(registry.store())
            .record_input(DELIVERY_ID, ECHO_ID, now_millis())
            .unwrap();
        wait_for_output(&mut registry, ECHO_ID, "> noop");
        wait_for_state(&mut registry, ECHO_ID, AttentionState::Idle);

        assert!(
            CompletionLedger::new(registry.store())
                .latest_completion(ECHO_ID)
                .unwrap()
                .is_none(),
            "prompt echo and composer redraw are not work evidence"
        );
        assert!(matches!(
            TimerService::new(&mut registry)
                .set_idle(
                    "echo-owner".into(),
                    DELIVERY_ID,
                    "must wait".into(),
                    TimerKind::IdleAny,
                    vec![ECHO_ID],
                    10_000,
                    now_millis(),
                )
                .unwrap(),
            IdleTimerOutcome::Created(_)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn generic_adapter_uses_output_after_recent_input_grace_as_work_evidence() {
        const KIMI_ID: ProcessId = 16;

        let mut registry = test_registry(false);
        registry
            .create(process(
                KIMI_ID,
                "kimi-style",
                r#"printf '❯\n'; while IFS= read -r line; do sleep 2.2; printf 'answer:%s\n❯\n' "$line"; done"#,
                Some(91),
            ))
            .unwrap();
        registry.start(KIMI_ID).unwrap();
        put_actor(&registry, "kimi-owner", DELIVERY_ID);
        wait_for_state(&mut registry, KIMI_ID, AttentionState::Idle);

        registry.submit_input(KIMI_ID, b"go").unwrap();
        CompletionLedger::new(registry.store())
            .record_input(DELIVERY_ID, KIMI_ID, now_millis())
            .unwrap();
        wait_for_output(&mut registry, KIMI_ID, "answer:go");
        wait_for_state(&mut registry, KIMI_ID, AttentionState::Idle);

        let outcome = TimerService::new(&mut registry)
            .set_idle(
                "kimi-owner".into(),
                DELIVERY_ID,
                "kimi finished".into(),
                TimerKind::IdleAny,
                vec![KIMI_ID],
                10_000,
                now_millis(),
            )
            .unwrap();
        assert!(matches!(outcome, IdleTimerOutcome::AlreadySatisfied { .. }));
    }

    #[cfg(unix)]
    #[test]
    fn generic_idle_repaints_do_not_mint_new_completions_or_refire() {
        const REPAINT_ID: ProcessId = 19;

        let mut registry = test_registry(false);
        registry
            .create(process(
                REPAINT_ID,
                "kimi-idle-repaint",
                r#"printf '❯\n'; IFS= read -r line; sleep 2.5; printf 'answer:%s\n❯ ' "$line"; while :; do sleep 0.3; printf '\r❯ '; done"#,
                Some(91),
            ))
            .unwrap();
        registry.start(REPAINT_ID).unwrap();
        put_actor(&registry, "repaint-owner", DELIVERY_ID);
        wait_for_state(&mut registry, REPAINT_ID, AttentionState::Idle);

        registry.submit_input(REPAINT_ID, b"go").unwrap();
        CompletionLedger::new(registry.store())
            .record_input(DELIVERY_ID, REPAINT_ID, now_millis())
            .unwrap();
        wait_for_output(&mut registry, REPAINT_ID, "answer:go");
        wait_for_state(&mut registry, REPAINT_ID, AttentionState::Idle);
        assert!(matches!(
            TimerService::new(&mut registry)
                .set_idle(
                    "repaint-owner".into(),
                    DELIVERY_ID,
                    "first repaint wake".into(),
                    TimerKind::IdleAny,
                    vec![REPAINT_ID],
                    20_000,
                    now_millis(),
                )
                .unwrap(),
            IdleTimerOutcome::AlreadySatisfied { .. }
        ));
        let completion_count = |registry: &ProcessRegistry| -> i64 {
            registry
                .store()
                .connection()
                .query_row(
                    "SELECT COUNT(*) FROM process_completions WHERE process_id = ?1",
                    [REPAINT_ID],
                    |row| row.get(0),
                )
                .unwrap()
        };
        assert_eq!(completion_count(&registry), 1);

        let poll_until = Instant::now() + Duration::from_secs(2);
        while Instant::now() < poll_until {
            assert_eq!(
                registry.get_status(REPAINT_ID).unwrap().agent_state.state,
                AttentionState::Idle
            );
            thread::sleep(Duration::from_millis(25));
        }
        assert_eq!(
            completion_count(&registry),
            1,
            "cosmetic idle repaints must not advance work evidence"
        );
        assert!(matches!(
            TimerService::new(&mut registry)
                .set_idle(
                    "repaint-owner".into(),
                    DELIVERY_ID,
                    "must wait for real work".into(),
                    TimerKind::IdleAny,
                    vec![REPAINT_ID],
                    20_000,
                    now_millis(),
                )
                .unwrap(),
            IdleTimerOutcome::Created(_)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn later_work_supersedes_a_mid_turn_idle_completion() {
        const BURSTY_ID: ProcessId = 17;

        let mut registry = test_registry(false);
        registry
            .create(process(
                BURSTY_ID,
                "mid-turn-composer",
                r#"printf '❯\n'; while IFS= read -r line; do printf 'thinking...\nesc to interrupt\n'; sleep 0.2; printf 'partial\n❯\n'; sleep 7; printf 'thinking...\nesc to interrupt\n'; sleep 0.2; printf 'final\n❯\n'; done"#,
                Some(90),
            ))
            .unwrap();
        registry.start(BURSTY_ID).unwrap();
        put_actor(&registry, "bursty-owner", DELIVERY_ID);
        wait_for_state(&mut registry, BURSTY_ID, AttentionState::Idle);

        registry.submit_input(BURSTY_ID, b"go").unwrap();
        CompletionLedger::new(registry.store())
            .record_input(DELIVERY_ID, BURSTY_ID, now_millis())
            .unwrap();
        wait_for_state(&mut registry, BURSTY_ID, AttentionState::Working);
        wait_for_state(&mut registry, BURSTY_ID, AttentionState::Idle);
        let early = CompletionLedger::new(registry.store())
            .latest_completion(BURSTY_ID)
            .unwrap()
            .expect("first work episode records the transient idle");
        assert!(matches!(
            TimerService::new(&mut registry)
                .set_idle(
                    "bursty-owner".into(),
                    DELIVERY_ID,
                    "early wake".into(),
                    TimerKind::IdleAny,
                    vec![BURSTY_ID],
                    20_000,
                    now_millis(),
                )
                .unwrap(),
            IdleTimerOutcome::AlreadySatisfied { .. }
        ));

        wait_for_state(&mut registry, BURSTY_ID, AttentionState::Working);
        wait_for_state(&mut registry, BURSTY_ID, AttentionState::Idle);
        let final_completion = CompletionLedger::new(registry.store())
            .latest_completion(BURSTY_ID)
            .unwrap()
            .expect("the resumed work episode records the real turn end");
        assert!(final_completion.id > early.id);
        let final_wake = TimerService::new(&mut registry)
            .set_idle(
                "bursty-owner".into(),
                DELIVERY_ID,
                "final wake".into(),
                TimerKind::IdleAny,
                vec![BURSTY_ID],
                20_000,
                now_millis(),
            )
            .unwrap();
        assert!(matches!(
            final_wake,
            IdleTimerOutcome::AlreadySatisfied { .. }
        ));
        let self_delivery_inputs: i64 = registry
            .store()
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM process_completion_inputs
                 WHERE owner_process_id = ?1 AND process_id = ?1",
                [DELIVERY_ID],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(self_delivery_inputs, 0);
    }

    #[cfg(unix)]
    #[test]
    fn idle_all_fires_once_when_a_satisfied_process_is_closed() {
        const SLOW_ID: ProcessId = 18;

        let mut registry = test_registry(true);
        registry
            .create(process(
                SLOW_ID,
                "slow-worker",
                "printf '❯\\n'; while IFS= read -r line; do printf 'thinking...\\nesc to interrupt\\n'; sleep 1.5; printf '❯\\n'; done",
                Some(90),
            ))
            .unwrap();
        registry.start(SLOW_ID).unwrap();
        put_actor(&registry, "storm-owner", DELIVERY_ID);
        wait_for_state(&mut registry, WORKER_ID, AttentionState::Idle);
        wait_for_state(&mut registry, SLOW_ID, AttentionState::Idle);

        registry.submit_input(WORKER_ID, b"go").unwrap();
        registry.submit_input(SLOW_ID, b"go").unwrap();
        wait_for_state(&mut registry, WORKER_ID, AttentionState::Working);
        wait_for_state(&mut registry, SLOW_ID, AttentionState::Working);
        let timer_id = match TimerService::new(&mut registry)
            .set_idle(
                "storm-owner".into(),
                DELIVERY_ID,
                "all done".into(),
                TimerKind::IdleAll,
                vec![WORKER_ID, SLOW_ID],
                30_000,
                now_millis(),
            )
            .unwrap()
        {
            IdleTimerOutcome::Created(timer) => timer.timer.id,
            IdleTimerOutcome::AlreadySatisfied { .. } => panic!("workers were still active"),
        };

        wait_for_state(&mut registry, WORKER_ID, AttentionState::Idle);
        assert!(
            TimerService::new(&mut registry)
                .tick(now_millis())
                .unwrap()
                .is_empty()
        );
        registry.close(WORKER_ID).unwrap();
        wait_for_state(&mut registry, SLOW_ID, AttentionState::Idle);
        let fires = TimerService::new(&mut registry).tick(now_millis()).unwrap();
        assert_eq!(fires.len(), 1);
        assert_eq!(fires[0].timer_id, timer_id);
        wait_for_output(&mut registry, DELIVERY_ID, "received:[all done]");

        for _ in 0..8 {
            assert!(
                TimerService::new(&mut registry)
                    .tick(now_millis())
                    .unwrap()
                    .is_empty()
            );
        }
        thread::sleep(Duration::from_millis(100));
        let output = registry.rendered_output(DELIVERY_ID).unwrap().text;
        assert_eq!(output.matches("received:[all done]").count(), 1);
        assert!(registry.store().get_timer(timer_id).unwrap().unwrap().fired);
    }

    #[cfg(unix)]
    #[test]
    fn one_bad_timer_does_not_block_later_due_timers() {
        let mut registry = test_registry(false);
        let bad_id = TimerService::new(&mut registry)
            .set_delay(
                "bad-timer".into(),
                DELIVERY_ID,
                "must not deliver".into(),
                0,
                false,
                None,
                1_000,
            )
            .unwrap()
            .timer
            .id;
        let good_id = TimerService::new(&mut registry)
            .set_delay(
                "good-timer".into(),
                DELIVERY_ID,
                "later timer delivered".into(),
                0,
                false,
                None,
                1_000,
            )
            .unwrap()
            .timer
            .id;
        registry
            .store()
            .connection()
            .execute(
                "UPDATE timer_runtime
                 SET diagnostics = '{\"already_idle\":\"not-an-array\"}'
                 WHERE timer_id = ?1",
                [bad_id],
            )
            .unwrap();

        let fires = TimerService::new(&mut registry).tick(1_000).unwrap();
        assert_eq!(fires.len(), 1);
        assert_eq!(fires[0].timer_id, good_id);
        let bad_timer = registry.store().get_timer(bad_id).unwrap().unwrap();
        assert!(bad_timer.fired, "the broken timer must be quarantined");
        assert_eq!(bad_timer.fired_at, Some(1_000));
        let bad_view = TimerService::new(&mut registry)
            .list(PROJECT_ID, 10, 1_000)
            .unwrap()
            .into_iter()
            .find(|view| view.timer.id == bad_id)
            .unwrap();
        assert_eq!(bad_view.fire_reason, Some(TimerSatisfactionReason::Error));
        assert_eq!(
            bad_view.satisfied_by[0].reason,
            TimerSatisfactionReason::Error
        );
        for tick in 1_001..1_041 {
            assert!(
                TimerService::new(&mut registry)
                    .tick(tick)
                    .unwrap()
                    .is_empty(),
                "a quarantined timer must not retry or log on later ticks"
            );
        }
        wait_for_output(
            &mut registry,
            DELIVERY_ID,
            "received:[later timer delivered]",
        );
        let output = registry.rendered_output(DELIVERY_ID).unwrap().text;
        assert!(!output.contains("must not deliver"));
        assert_eq!(
            output.matches("received:[later timer delivered]").count(),
            1
        );
    }

    #[cfg(unix)]
    #[test]
    fn fresh_transition_does_not_attach_or_report_an_old_completion() {
        let mut registry = test_registry(true);
        put_actor(&registry, "fresh-diagnostics-owner", DELIVERY_ID);
        wait_for_state(&mut registry, WORKER_ID, AttentionState::Idle);
        let old_completion = CompletionLedger::new(registry.store())
            .observe_process(
                WORKER_ID,
                AttentionState::Idle,
                Some(50),
                Some(60),
                false,
                100,
            )
            .unwrap()
            .unwrap();
        let created_at = now_millis();
        let timer_id = match TimerService::new(&mut registry)
            .set_idle(
                "fresh-diagnostics-owner".into(),
                DELIVERY_ID,
                "fresh only".into(),
                TimerKind::IdleAny,
                vec![WORKER_ID],
                20_000,
                created_at,
            )
            .unwrap()
        {
            IdleTimerOutcome::Created(timer) => timer.timer.id,
            IdleTimerOutcome::AlreadySatisfied { .. } => {
                panic!("an owner with no recorded input keeps transition-only behavior")
            }
        };

        registry.send_input(WORKER_ID, b"noop\r").unwrap();
        wait_for_state(&mut registry, WORKER_ID, AttentionState::Working);
        assert!(
            TimerService::new(&mut registry)
                .tick(now_millis())
                .unwrap()
                .is_empty()
        );
        wait_for_state(&mut registry, WORKER_ID, AttentionState::Idle);
        let fire = TimerService::new(&mut registry)
            .tick(now_millis())
            .unwrap()
            .into_iter()
            .find(|fire| fire.timer_id == timer_id)
            .expect("fresh transition fires");
        let satisfaction = fire
            .timer
            .satisfied_by
            .iter()
            .find(|satisfaction| satisfaction.process_id == Some(WORKER_ID))
            .unwrap();
        assert_eq!(
            satisfaction.reason,
            TimerSatisfactionReason::FreshTransition
        );
        assert!(satisfaction.completed_at_ms.unwrap() >= created_at);
        let reports: i64 = registry
            .store()
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM process_completion_reports
                 WHERE owner_process_id = ?1 AND completion_id = ?2",
                (DELIVERY_ID, old_completion.id),
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(reports, 0);
    }

    #[test]
    fn prompted_but_not_started_and_deadline_do_not_consume_completions() {
        const STALLED_ID: ProcessId = 12;

        let mut registry = test_registry(true);
        registry
            .store()
            .put_actor(&Actor {
                id: "deadline-owner".into(),
                session_id: "deadline-owner-session".into(),
                process_id: Some(DELIVERY_ID),
                selected_project_id: Some(PROJECT_ID),
                created_at: 1_000,
                last_seen_at: 1_000,
            })
            .unwrap();
        wait_for_state(&mut registry, WORKER_ID, AttentionState::Idle);
        let pending = registry.reserve_prompt(WORKER_ID).unwrap();
        CompletionLedger::new(registry.store())
            .record_input(DELIVERY_ID, WORKER_ID, 100)
            .unwrap();
        let prompted = TimerService::new(&mut registry)
            .set_idle(
                "deadline-owner".into(),
                DELIVERY_ID,
                "not yet".into(),
                TimerKind::IdleAny,
                vec![WORKER_ID],
                10_000,
                200,
            )
            .unwrap();
        assert!(matches!(prompted, IdleTimerOutcome::Created(_)));
        drop(pending);

        registry
            .create(process(STALLED_ID, "stalled-ledger", "sleep 30", None))
            .unwrap();
        let ledger = CompletionLedger::new(registry.store());
        ledger.record_input(DELIVERY_ID, STALLED_ID, 300).unwrap();
        let completion = ledger
            .observe_process(
                STALLED_ID,
                AttentionState::Idle,
                Some(301),
                Some(302),
                false,
                400,
            )
            .unwrap()
            .unwrap();
        let timer_id = match TimerService::new(&mut registry)
            .set_idle(
                "deadline-owner".into(),
                DELIVERY_ID,
                "deadline wake".into(),
                TimerKind::IdleAny,
                vec![STALLED_ID],
                10,
                500,
            )
            .unwrap()
        {
            IdleTimerOutcome::Created(timer) => timer.timer.id,
            IdleTimerOutcome::AlreadySatisfied { .. } => {
                panic!("a stopped process cannot satisfy idle_any")
            }
        };
        let fires = TimerService::new(&mut registry).tick(510).unwrap();
        let fire = fires
            .iter()
            .find(|fire| fire.timer_id == timer_id)
            .expect("deadline fires");
        assert_eq!(fire.reason, TimerFireReason::MaxWait);
        assert_eq!(
            fire.timer.fire_reason,
            Some(TimerSatisfactionReason::Deadline)
        );
        assert_eq!(
            CompletionLedger::new(registry.store())
                .unreported_completions(DELIVERY_ID, &[STALLED_ID])
                .unwrap(),
            vec![completion],
            "deadline delivery must not mark a completion reported"
        );
    }

    #[cfg(unix)]
    #[test]
    fn idle_any_requires_fresh_transition_all_can_be_satisfied_and_timeout_fires() {
        let mut registry = test_registry(true);
        wait_for_state(&mut registry, WORKER_ID, AttentionState::Idle);

        let already = TimerService::new(&mut registry)
            .set_idle(
                "actor-all".into(),
                DELIVERY_ID,
                "already idle".into(),
                TimerKind::IdleAll,
                vec![WORKER_ID],
                10_000,
                0,
            )
            .unwrap();
        assert!(matches!(already, IdleTimerOutcome::AlreadySatisfied { .. }));
        wait_for_output(&mut registry, DELIVERY_ID, "received:[already idle]");
        assert!(
            TimerService::new(&mut registry)
                .list(PROJECT_ID, 10, 0)
                .unwrap()
                .is_empty()
        );

        let any_timer_id = match TimerService::new(&mut registry)
            .set_idle(
                "actor-any".into(),
                DELIVERY_ID,
                "fresh idle wake".into(),
                TimerKind::IdleAny,
                vec![WORKER_ID],
                100_000,
                0,
            )
            .unwrap()
        {
            IdleTimerOutcome::Created(timer) => timer.timer.id,
            IdleTimerOutcome::AlreadySatisfied { .. } => panic!("idle_any fired immediately"),
        };
        assert!(TimerService::new(&mut registry).tick(0).unwrap().is_empty());

        registry.send_input(WORKER_ID, b"go\r").unwrap();
        wait_for_state(&mut registry, WORKER_ID, AttentionState::Working);
        assert!(
            TimerService::new(&mut registry)
                .tick(10)
                .unwrap()
                .is_empty()
        );
        wait_for_state(&mut registry, WORKER_ID, AttentionState::Idle);
        let watched_done = registry.get_status(WORKER_ID).unwrap().agent_state;
        assert!(watched_done.watched);
        assert!(
            !watched_done.unread,
            "a pending idle watch must suppress the human unread notification"
        );
        let fired = TimerService::new(&mut registry).tick(20).unwrap();
        assert!(fired.iter().any(|fire| {
            fire.timer_id == any_timer_id && fire.reason == TimerFireReason::IdleTransition
        }));
        let consumed: i64 = registry
            .store()
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM consumed_idle_watches
                 WHERE process_id = ?1 AND timer_id = ?2",
                (WORKER_ID, any_timer_id),
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(consumed, 1, "idle-transition fire records suppression");
        wait_for_output(&mut registry, DELIVERY_ID, "received:[fresh idle wake]");

        registry
            .create(process(12, "stalled", "sleep 30", None))
            .unwrap();
        let timeout_timer_id = match TimerService::new(&mut registry)
            .set_idle(
                "actor-timeout".into(),
                DELIVERY_ID,
                "timeout wake".into(),
                TimerKind::IdleAny,
                vec![12],
                50,
                100,
            )
            .unwrap()
        {
            IdleTimerOutcome::Created(timer) => timer.timer.id,
            IdleTimerOutcome::AlreadySatisfied { .. } => panic!("stopped process read idle"),
        };
        assert!(
            TimerService::new(&mut registry)
                .tick(149)
                .unwrap()
                .is_empty()
        );
        let fired = TimerService::new(&mut registry).tick(150).unwrap();
        assert!(fired.iter().any(|fire| {
            fire.timer_id == timeout_timer_id && fire.reason == TimerFireReason::MaxWait
        }));
        let consumed: i64 = registry
            .store()
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM consumed_idle_watches WHERE timer_id = ?1",
                [timeout_timer_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            consumed, 0,
            "max-wait wake must not suppress the watched process's later completion"
        );
        wait_for_output(&mut registry, DELIVERY_ID, "received:[timeout wake]");
    }

    #[cfg(unix)]
    #[test]
    fn idle_watches_wait_for_pending_prompts_and_the_resulting_turn() {
        let mut registry = test_registry(true);
        let initial_prompt = registry.reserve_prompt(WORKER_ID).unwrap();
        let another_prompt = registry.reserve_prompt(WORKER_ID).unwrap();
        wait_for_state(&mut registry, WORKER_ID, AttentionState::Idle);

        let mut timer_ids = Vec::new();
        for kind in [TimerKind::IdleAny, TimerKind::IdleAll] {
            let outcome = TimerService::new(&mut registry)
                .set_idle(
                    "pending-prompt-owner".into(),
                    DELIVERY_ID,
                    "child finished".into(),
                    kind,
                    vec![WORKER_ID],
                    100_000,
                    0,
                )
                .unwrap();
            let IdleTimerOutcome::Created(timer) = outcome else {
                panic!("a pending initial prompt must prevent already_satisfied");
            };
            timer_ids.push(timer.timer.id);
        }
        registry.set_notify_on_idle(WORKER_ID, true).unwrap();
        // Even an expired alert debounce must not consume a startup idle frame.
        registry
            .store()
            .connection()
            .execute("UPDATE process_idle_watches SET ready_since = 0", [])
            .unwrap();
        assert!(registry.get_status(WORKER_ID).unwrap().notify_on_idle);
        assert!(TimerService::new(&mut registry).tick(1).unwrap().is_empty());
        drop(another_prompt);
        assert!(TimerService::new(&mut registry).tick(2).unwrap().is_empty());

        registry.submit_input(WORKER_ID, b"go").unwrap();
        drop(initial_prompt);
        wait_for_state(&mut registry, WORKER_ID, AttentionState::Working);
        assert!(TimerService::new(&mut registry).tick(3).unwrap().is_empty());
        wait_for_state(&mut registry, WORKER_ID, AttentionState::Idle);
        let fires = TimerService::new(&mut registry).tick(4).unwrap();
        assert_eq!(fires.len(), 2);
        assert!(
            fires
                .iter()
                .all(|fire| fire.reason == TimerFireReason::IdleTransition)
        );
        assert!(fires.iter().all(|fire| timer_ids.contains(&fire.timer_id)));
        assert!(!registry.has_pending_prompts(WORKER_ID));
        registry
            .store()
            .connection()
            .execute("UPDATE process_idle_watches SET ready_since = 0", [])
            .unwrap();
        assert!(!registry.get_status(WORKER_ID).unwrap().notify_on_idle);
        assert!(TimerService::new(&mut registry).tick(5).unwrap().is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn idle_all_invalidates_a_completed_child_when_another_prompt_is_pending() {
        let mut registry = test_registry(true);
        wait_for_state(&mut registry, WORKER_ID, AttentionState::Idle);
        let other_prompt = registry.reserve_prompt(DELIVERY_ID).unwrap();
        wait_for_state(&mut registry, DELIVERY_ID, AttentionState::Idle);
        let outcome = TimerService::new(&mut registry)
            .set_idle(
                "pending-prompt-owner".into(),
                DELIVERY_ID,
                "all finished".into(),
                TimerKind::IdleAll,
                vec![WORKER_ID, DELIVERY_ID],
                100_000,
                0,
            )
            .unwrap();
        assert!(matches!(outcome, IdleTimerOutcome::Created(_)));
        let new_prompt = registry.reserve_prompt(WORKER_ID).unwrap();
        drop(other_prompt);
        assert!(TimerService::new(&mut registry).tick(1).unwrap().is_empty());
        // Dropping an abandoned prompt releases its reservation too.
        drop(new_prompt);
        let fires = TimerService::new(&mut registry).tick(2).unwrap();
        assert_eq!(fires.len(), 1);
        assert_eq!(fires[0].reason, TimerFireReason::IdleTransition);
    }

    #[cfg(unix)]
    #[test]
    fn pending_prompts_preserve_the_idle_watch_max_wait_deadline() {
        let mut registry = test_registry(true);
        let _pending_prompt = registry.reserve_prompt(WORKER_ID).unwrap();
        TimerService::new(&mut registry)
            .set_idle(
                "pending-prompt-owner".into(),
                DELIVERY_ID,
                "deadline reached".into(),
                TimerKind::IdleAll,
                vec![WORKER_ID],
                100,
                0,
            )
            .unwrap();
        assert!(
            TimerService::new(&mut registry)
                .tick(99)
                .unwrap()
                .is_empty()
        );
        let fires = TimerService::new(&mut registry).tick(100).unwrap();
        assert_eq!(fires.len(), 1);
        assert_eq!(fires[0].reason, TimerFireReason::MaxWait);
    }

    #[cfg(unix)]
    #[test]
    fn idle_watch_ignores_a_transient_prompt_frame_and_fires_after_stable_idle() {
        const FLICKER_ID: ProcessId = 13;

        let mut registry = test_registry(false);
        registry
            .create(process(
                FLICKER_ID,
                "bursty-worker",
                "printf '❯\\n'; while IFS= read -r line; do if [ \"$line\" = go ]; then printf '\\033[2J\\033[Hthinking...\\nesc to interrupt\\n'; sleep 0.2; printf '\\033[2J\\033[Hpartial answer\\n❯\\n'; sleep 1; printf '\\033[2J\\033[Hthinking...\\nesc to interrupt\\n'; sleep 0.2; printf '\\033[2J\\033[Hfinal answer\\n❯\\n'; fi; done",
                Some(90),
            ))
            .unwrap();
        registry.start(FLICKER_ID).unwrap();
        wait_for_state(&mut registry, FLICKER_ID, AttentionState::Idle);

        let timer_id = match TimerService::new(&mut registry)
            .set_idle(
                "actor-flicker".into(),
                DELIVERY_ID,
                "stable idle wake".into(),
                TimerKind::IdleAny,
                vec![FLICKER_ID],
                100_000,
                0,
            )
            .unwrap()
        {
            IdleTimerOutcome::Created(timer) => timer.timer.id,
            IdleTimerOutcome::AlreadySatisfied { .. } => panic!("idle_any fired immediately"),
        };

        registry.send_input(FLICKER_ID, b"go\r").unwrap();
        wait_for_state(&mut registry, FLICKER_ID, AttentionState::Working);
        let transient_window = Instant::now() + Duration::from_millis(1_600);
        while Instant::now() < transient_window {
            let fired = TimerService::new(&mut registry).tick(10).unwrap();
            assert!(
                fired.iter().all(|event| event.timer_id != timer_id),
                "a prompt-shaped frame inside a running turn fired the idle watch"
            );
            assert_eq!(
                registry.get_status(FLICKER_ID).unwrap().agent_state.state,
                AttentionState::Working
            );
            thread::sleep(Duration::from_millis(20));
        }

        wait_for_state(&mut registry, FLICKER_ID, AttentionState::Idle);
        let fired = TimerService::new(&mut registry).tick(20).unwrap();
        assert!(fired.iter().any(|event| {
            event.timer_id == timer_id && event.reason == TimerFireReason::IdleTransition
        }));
        wait_for_output(&mut registry, DELIVERY_ID, "received:[stable idle wake]");
    }

    #[cfg(unix)]
    #[test]
    fn focus_in_and_out_redraws_leave_an_idle_agent_idle_without_notifications() {
        const FOCUSED_ID: ProcessId = 14;

        let mut registry = test_registry(false);
        registry
            .create(process(
                FOCUSED_ID,
                "focus-reporting-worker",
                r#"stty raw -echo; exec perl -e '$|=1; $SIG{WINCH}=sub { print "\e[2J\e[Hresize redraw\r\n❯ " }; print "\e[?1004h❯ "; while (1) { my $count=sysread(STDIN, my $chunk, 3); next unless defined($count); last unless $count; print "\e[2J\e[Hview refresh\r\n❯ "; }'"#,
                Some(90),
            ))
            .unwrap();
        registry.start(FOCUSED_ID).unwrap();
        wait_for_state(&mut registry, FOCUSED_ID, AttentionState::Idle);
        assert!(registry.terminal_focus_reporting(FOCUSED_ID).unwrap());
        assert!(
            registry
                .store()
                .list_notifications(None, 10)
                .unwrap()
                .is_empty()
        );

        registry.send_input(FOCUSED_ID, b"\x1b[I").unwrap();
        wait_for_output(&mut registry, FOCUSED_ID, "view refresh");
        thread::sleep(Duration::from_millis(600));
        assert_eq!(
            registry.get_status(FOCUSED_ID).unwrap().agent_state.state,
            AttentionState::Idle,
            "clicking into the terminal must be attention-neutral"
        );

        registry.send_input(FOCUSED_ID, b"\x1b[O").unwrap();
        thread::sleep(Duration::from_millis(600));
        assert_eq!(
            registry.get_status(FOCUSED_ID).unwrap().agent_state.state,
            AttentionState::Idle,
            "clicking away from the terminal must be attention-neutral"
        );

        registry.resize(FOCUSED_ID, 30, 100, 0, 0).unwrap();
        wait_for_output(&mut registry, FOCUSED_ID, "resize redraw");
        thread::sleep(Duration::from_millis(600));
        assert_eq!(
            registry.get_status(FOCUSED_ID).unwrap().agent_state.state,
            AttentionState::Idle,
            "a UI resize redraw must be attention-neutral"
        );
        assert!(
            registry
                .store()
                .list_notifications(None, 10)
                .unwrap()
                .is_empty(),
            "focus selection produced a completion notification"
        );
    }

    #[cfg(unix)]
    #[test]
    fn unwatched_done_agent_self_clears_without_rapidly_refiring() {
        let mut registry = test_registry(true);
        wait_for_state(&mut registry, WORKER_ID, AttentionState::Idle);
        let baseline = registry.get_status(WORKER_ID).unwrap().agent_state;
        assert!(!baseline.watched);
        assert!(!baseline.unread);

        registry.send_input(WORKER_ID, b"go\r").unwrap();
        wait_for_state(&mut registry, WORKER_ID, AttentionState::Working);
        wait_for_state(&mut registry, WORKER_ID, AttentionState::Idle);
        let unread = registry.get_status(WORKER_ID).unwrap();
        assert!(unread.agent_state.unread);
        assert!(!unread.agent_state.watched);
        let payload = serde_json::to_value(&unread).unwrap();
        assert_eq!(payload["agent_state"]["unread"], true);
        assert_eq!(payload["agent_state"]["watched"], false);

        registry.send_input(WORKER_ID, b"go\r").unwrap();
        wait_for_state(&mut registry, WORKER_ID, AttentionState::Working);
        assert!(
            !registry.get_status(WORKER_ID).unwrap().agent_state.unread,
            "starting another turn must self-clear unread"
        );
        wait_for_state(&mut registry, WORKER_ID, AttentionState::Idle);
        assert!(
            !registry.get_status(WORKER_ID).unwrap().agent_state.unread,
            "a second completion inside the backstop window must not re-fire without a user view"
        );
        registry.stop(WORKER_ID).unwrap();
        let exited = registry.get_status(WORKER_ID).unwrap();
        assert_eq!(exited.agent_state.state, AttentionState::Exited);
        assert!(
            !exited.agent_state.unread,
            "an immediate exit must share the same per-process notification backstop"
        );
    }

    #[test]
    fn watch_progress_ignores_existing_idle_until_work_then_idle() {
        let mut progress = WatchProgress {
            armed: false,
            satisfied: false,
            last_idle: true,
            completion_id: None,
        };
        advance_watch_progress(&mut progress, true);
        assert!(!progress.armed);
        assert!(!progress.satisfied);
        advance_watch_progress(&mut progress, false);
        assert!(progress.armed);
        assert!(!progress.satisfied);
        advance_watch_progress(&mut progress, true);
        assert!(progress.satisfied);
    }
}
