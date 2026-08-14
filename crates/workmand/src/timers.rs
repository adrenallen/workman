//! Durable timer scheduling and idle-transition wake-up delivery.

use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt,
    time::Duration,
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
    timer_events::{TimerLifecycleEvent, TimerLifecycleHub, TimerLifecycleKind},
};

const TIMER_POLL_INTERVAL: Duration = Duration::from_millis(25);

#[derive(Debug)]
pub(crate) enum TimerError {
    Store(StoreError),
    Registry(RegistryError),
    Persistence(String),
    NotFound(TimerId),
    Inactive(TimerId),
    EmptyWatchList,
    InvalidDelay,
    InvalidRepeatInterval,
    InvalidMaxWait,
    CrossProjectTarget {
        owner_project_id: ProjectId,
        target_process_id: ProcessId,
        target_project_id: ProjectId,
    },
}

impl TimerError {
    pub(crate) const fn code(&self) -> &'static str {
        match self {
            Self::Store(_) | Self::Persistence(_) => "timer_store_error",
            Self::Registry(error) => error.code(),
            Self::NotFound(_) => "timer_not_found",
            Self::Inactive(_) => "timer_inactive",
            Self::EmptyWatchList => "empty_watch_list",
            Self::InvalidDelay => "invalid_delay",
            Self::InvalidRepeatInterval => "invalid_repeat_interval",
            Self::InvalidMaxWait => "invalid_max_wait",
            Self::CrossProjectTarget { .. } => "timer_cross_project_target",
        }
    }
}

impl fmt::Display for TimerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(error) => error.fmt(formatter),
            Self::Registry(error) => error.fmt(formatter),
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

pub(crate) type TimerResult<T> = Result<T, TimerError>;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct WatchProgress {
    armed: bool,
    satisfied: bool,
    last_idle: bool,
}

impl WatchProgress {
    pub(crate) const fn new(initial_idle: bool, already_satisfied: bool) -> Self {
        Self {
            armed: !initial_idle,
            satisfied: already_satisfied,
            last_idle: initial_idle,
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
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct TimerView {
    #[serde(flatten)]
    pub timer: Timer,
    pub owner_process_name: Option<String>,
    pub owner_label: String,
    pub due_at: i64,
    pub paused_at: Option<i64>,
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
    Created(TimerView),
    AlreadySatisfied {
        watch_process_ids: Vec<ProcessId>,
        delivery_process_id: ProcessId,
        delivered_at: i64,
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
        let mut watch_state = BTreeMap::new();
        for process_id in &watch_process_ids {
            let idle = self.process_is_idle(*process_id)?;
            watch_state.insert(
                *process_id,
                // idle_any deliberately starts unarmed for an already-idle process.
                WatchProgress::new(idle, kind == TimerKind::IdleAll && idle),
            );
        }
        if kind == TimerKind::IdleAll && watch_state.values().all(|progress| progress.satisfied) {
            self.registry
                .submit_input(delivery_process_id, body.as_bytes())?;
            return Ok(IdleTimerOutcome::AlreadySatisfied {
                watch_process_ids,
                delivery_process_id,
                delivered_at: now_ms,
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
        };
        self.insert(&timer, &runtime)?;
        Ok(IdleTimerOutcome::Created(self.view(timer, runtime)?))
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
            let Some(mut timer) = self.registry.store().get_timer(timer_id)? else {
                continue;
            };
            if timer.paused || timer.fired {
                continue;
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
                    continue;
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
                    let advanced = self.advance_idle_state(&timer, &mut runtime)?;
                    transitioned_process_ids = advanced.transitioned_process_ids;
                    if idle_condition_satisfied(&timer, &runtime) {
                        reason = Some(TimerFireReason::IdleTransition);
                    } else if now_ms >= runtime.due_at {
                        reason = Some(TimerFireReason::MaxWait);
                    }
                    if advanced.changed && reason.is_none() {
                        self.put_runtime(timer.id, &runtime)?;
                    }
                }
            }

            let Some(reason) = reason else { continue };
            if self
                .registry
                .submit_input(timer.delivery_process_id, timer.body.as_bytes())
                .is_err()
            {
                // Delivery is at-least-once. Keep the timer pending and retry after
                // the target process is started again.
                continue;
            }

            // Pending timers suppress directly. Once an idle-transition timer
            // is consumed, preserve that transition's watch state across the
            // debounced done-check. Max-wait expiry is only a wake about a
            // still-busy process and deliberately records no marker.
            if reason == TimerFireReason::IdleTransition {
                for process_id in transitioned_process_ids {
                    self.registry
                        .store()
                        .record_consumed_idle_watch(process_id, timer.id, now_ms)?;
                }
            }

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
            let project_id = self.registry.get(timer.delivery_process_id)?.project_id;
            fired.push(TimerFire {
                timer_id: timer.id,
                project_id,
                delivery_process_id: timer.delivery_process_id,
                reason,
                fired_at: now_ms,
                timer: self.view(timer, runtime)?,
            });
        }
        Ok(fired)
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
            .query_row("SELECT COALESCE(MAX(id), 0) + 1 FROM timers", [], |row| {
                row.get(0)
            })
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
        self.registry
            .store()
            .connection()
            .execute(
                "INSERT INTO timer_runtime (timer_id, due_at, paused_at, watch_state)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(timer_id) DO UPDATE SET
                    due_at = excluded.due_at,
                    paused_at = excluded.paused_at,
                    watch_state = excluded.watch_state",
                (timer_id, runtime.due_at, runtime.paused_at, watch_state),
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
                    "SELECT due_at, paused_at, watch_state
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
                    ))
                })
                .transpose()?
        };
        let Some((due_at, paused_at, watch_state)) = row else {
            return Ok(None);
        };
        let watch_state = serde_json::from_str(&watch_state)
            .map_err(|error| TimerError::Persistence(error.to_string()))?;
        Ok(Some(TimerRuntime {
            due_at,
            paused_at,
            watch_state,
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
        for process_id in &timer.watch_process_ids {
            let idle = self.process_is_idle(*process_id)?;
            watch_state.insert(
                *process_id,
                WatchProgress::new(idle, timer.kind == TimerKind::IdleAll && idle),
            );
        }
        let runtime = TimerRuntime {
            due_at,
            paused_at: None,
            watch_state,
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
            Ok(status) => Ok(matches!(
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
            advance_watch_progress(progress, idle);
            changed |= *progress != before;
            if !before.satisfied && progress.satisfied {
                transitioned_process_ids.push(*process_id);
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
        }
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
    const PASTE_TUI_ID: ProcessId = 13;

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
        let mut registry =
            ProcessRegistry::with_stop_grace(store, Duration::from_millis(100)).unwrap();
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

    fn wait_for_output(registry: &mut ProcessRegistry, process_id: ProcessId, needle: &str) {
        let deadline = Instant::now() + Duration::from_secs(2);
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
                ProcessRegistry::with_stop_grace(store, Duration::from_millis(100)).unwrap();
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
            ProcessRegistry::with_stop_grace(store, Duration::from_millis(100)).unwrap();
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
