//! Durable timer scheduling and idle-transition wake-up delivery.

use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt,
    time::Duration,
};

use gbuild_core::{
    ProcessId, ProjectId, StoreError, Timer, TimerId, TimerKind, attention::AttentionState,
};
use serde::{Deserialize, Serialize};
use tokio::{
    sync::watch,
    task::JoinHandle,
    time::{MissedTickBehavior, interval},
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
struct WatchProgress {
    armed: bool,
    satisfied: bool,
    last_idle: bool,
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
        Ok(TimerView::new(timer, runtime))
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
                WatchProgress {
                    // idle_any deliberately starts unarmed for an already-idle process.
                    armed: !idle,
                    satisfied: kind == TimerKind::IdleAll && idle,
                    last_idle: idle,
                },
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
        Ok(IdleTimerOutcome::Created(TimerView::new(timer, runtime)))
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
        Ok(TimerView::new(timer, runtime))
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
        Ok(TimerView::new(timer, runtime))
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
        Ok(TimerView::new(timer, runtime))
    }

    pub(crate) fn list(
        &mut self,
        owner_actor: &str,
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
                     WHERE timer.owner_actor = ?1 AND process.project_id = ?2
                     ORDER BY timer.created_at DESC, timer.id DESC
                     LIMIT ?3",
                )
                .map_err(persistence)?;
            let rows = statement
                .query_map((owner_actor, project_id, limit as i64), |row| row.get(0))
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
            views.push(TimerView::new(timer, runtime));
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
            views.push(TimerView::new(timer, runtime));
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
            let mut runtime = self.runtime_or_reconstruct(&timer, now_ms)?;
            let mut reason = None;

            match timer.kind {
                TimerKind::Delay => {
                    if now_ms >= runtime.due_at {
                        reason = Some(TimerFireReason::Delay);
                    }
                }
                TimerKind::IdleAny | TimerKind::IdleAll => {
                    let changed = self.advance_idle_state(&timer, &mut runtime)?;
                    if idle_condition_satisfied(&timer, &runtime) {
                        reason = Some(TimerFireReason::IdleTransition);
                    } else if now_ms >= runtime.due_at {
                        reason = Some(TimerFireReason::MaxWait);
                    }
                    if changed && reason.is_none() {
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
                timer: TimerView::new(timer, runtime),
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
            .filter(|timer| timer.owner_actor == owner_actor)
            .ok_or(TimerError::NotFound(timer_id))?;
        let delivery = self
            .registry
            .store()
            .get_process(timer.delivery_process_id)?
            .filter(|process| process.project_id == project_id)
            .ok_or(TimerError::NotFound(timer_id))?;
        debug_assert_eq!(delivery.id, timer.delivery_process_id);
        Ok(timer)
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
                WatchProgress {
                    armed: !idle,
                    satisfied: timer.kind == TimerKind::IdleAll && idle,
                    last_idle: idle,
                },
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

    fn process_is_idle(&mut self, process_id: ProcessId) -> TimerResult<bool> {
        match self.registry.get_status(process_id) {
            Ok(status) => Ok(status.agent_state.state == AttentionState::Idle),
            Err(RegistryError::NotFound(_)) => Ok(false),
            Err(error) => Err(error.into()),
        }
    }

    fn advance_idle_state(
        &mut self,
        timer: &Timer,
        runtime: &mut TimerRuntime,
    ) -> TimerResult<bool> {
        let mut changed = false;
        for process_id in &timer.watch_process_ids {
            let idle = self.process_is_idle(*process_id)?;
            let progress = runtime
                .watch_state
                .entry(*process_id)
                .or_insert(WatchProgress {
                    armed: !idle,
                    satisfied: timer.kind == TimerKind::IdleAll && idle,
                    last_idle: idle,
                });
            let before = progress.clone();
            advance_watch_progress(progress, idle);
            changed |= *progress != before;
        }
        Ok(changed)
    }
}

fn advance_watch_progress(progress: &mut WatchProgress, idle: bool) {
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
    fn new(timer: Timer, runtime: TimerRuntime) -> Self {
        Self {
            timer,
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
        loop {
            tokio::select! {
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        break;
                    }
                }
                _ = ticker.tick() => {
                    let mut registry = registry.lock().await;
                    if let Ok(fires) = TimerService::new(&mut registry).tick(now_millis()) {
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

    use gbuild_core::{
        AgentTool, Process, ProcessKind, ProcessSource, ProcessStatus, Project, Store,
    };

    use super::*;

    const PROJECT_ID: ProjectId = 1;
    const DELIVERY_ID: ProcessId = 10;
    const WORKER_ID: ProcessId = 11;
    const PASTE_TUI_ID: ProcessId = 13;

    fn paste_sensitive_tui() -> &'static str {
        r#"true claude; stty raw -echo; printf '\033[?2004h❯ '; exec perl -e '$|=1; while (1) { my $n = sysread(STDIN, my $chunk, 4096); exit 2 unless defined($n) && $n > 0; if ($chunk eq "\r") { print "\r\nSUBMITTED\r\nthinking...\r\nesc to interrupt\r\n"; sleep 5; exit 0; } print "\r\nPASTED:$n\r\n"; }'"#
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
                source: gbuild_core::AgentToolSource::Local,
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
        let deadline = Instant::now() + Duration::from_secs(3);
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
            .list("actor-delay", PROJECT_ID, 10, 1_050)
            .unwrap();
        assert!(timers[0].timer.fired);
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
        wait_for_state(&mut registry, PASTE_TUI_ID, AttentionState::Working);
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
        wait_for_state(&mut registry, PASTE_TUI_ID, AttentionState::Working);
        assert!(
            TimerService::new(&mut registry)
                .list("actor-immediate", PROJECT_ID, 10, 2_000)
                .unwrap()
                .is_empty()
        );
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
        let database = temp.path().join("gbuild.db");
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
            .list("actor-repeat", PROJECT_ID, 1, 130)
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
                .list("actor-all", PROJECT_ID, 10, 0)
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
        let fired = TimerService::new(&mut registry).tick(20).unwrap();
        assert!(fired.iter().any(|fire| {
            fire.timer_id == any_timer_id && fire.reason == TimerFireReason::IdleTransition
        }));
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
        wait_for_output(&mut registry, DELIVERY_ID, "received:[timeout wake]");
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
