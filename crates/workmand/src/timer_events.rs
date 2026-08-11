//! Bounded in-memory timer lifecycle events shared by MCP mutations, the scheduler, and WS clients.

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex, MutexGuard},
};

use serde::Serialize;
use tokio::sync::watch;
use workman_core::{ProcessId, ProjectId, TimerId};

use crate::{
    status_invalidation::StatusInvalidationHub,
    timers::{TimerFireReason, TimerView},
};

const MAX_RETAINED_TIMER_EVENTS: usize = 256;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TimerLifecycleKind {
    Created,
    Fired,
    Delivered,
    Cancelled,
    Paused,
    Resumed,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct TimerLifecycleEvent {
    pub sequence: u64,
    pub kind: TimerLifecycleKind,
    pub timer_id: Option<TimerId>,
    pub project_id: ProjectId,
    pub delivery_process_id: ProcessId,
    pub at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<TimerFireReason>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timer: Option<TimerView>,
}

impl TimerLifecycleEvent {
    pub(crate) fn for_timer(
        kind: TimerLifecycleKind,
        project_id: ProjectId,
        timer: TimerView,
        at: i64,
        reason: Option<TimerFireReason>,
    ) -> Self {
        Self {
            sequence: 0,
            kind,
            timer_id: Some(timer.timer.id),
            project_id,
            delivery_process_id: timer.timer.delivery_process_id,
            at,
            reason,
            timer: Some(timer),
        }
    }

    pub(crate) fn immediate(
        kind: TimerLifecycleKind,
        project_id: ProjectId,
        delivery_process_id: ProcessId,
        at: i64,
    ) -> Self {
        Self {
            sequence: 0,
            kind,
            timer_id: None,
            project_id,
            delivery_process_id,
            at,
            reason: Some(TimerFireReason::AlreadySatisfied),
            timer: None,
        }
    }
}

#[derive(Default)]
struct TimerEventBuffer {
    next_sequence: u64,
    events: VecDeque<TimerLifecycleEvent>,
}

/// A synchronous publisher with a per-WS-client cursor over a bounded retained event log.
#[derive(Clone)]
pub(crate) struct TimerLifecycleHub {
    inner: Arc<Mutex<TimerEventBuffer>>,
    changed: watch::Sender<u64>,
    status_invalidations: StatusInvalidationHub,
}

impl Default for TimerLifecycleHub {
    fn default() -> Self {
        Self::new(StatusInvalidationHub::default())
    }
}

impl TimerLifecycleHub {
    pub(crate) fn new(status_invalidations: StatusInvalidationHub) -> Self {
        let (changed, _) = watch::channel(0);
        Self {
            inner: Arc::new(Mutex::new(TimerEventBuffer::default())),
            changed,
            status_invalidations,
        }
    }

    pub(crate) fn publish(&self, mut event: TimerLifecycleEvent) -> u64 {
        let mut inner = self.lock();
        inner.next_sequence = inner.next_sequence.saturating_add(1);
        event.sequence = inner.next_sequence;
        inner.events.push_back(event);
        while inner.events.len() > MAX_RETAINED_TIMER_EVENTS {
            inner.events.pop_front();
        }
        let sequence = inner.next_sequence;
        drop(inner);
        self.status_invalidations.invalidate();
        self.changed.send_replace(sequence);
        sequence
    }

    pub(crate) fn latest_sequence(&self) -> u64 {
        self.lock().next_sequence
    }

    pub(crate) fn events_since(&self, sequence: u64) -> (u64, Vec<TimerLifecycleEvent>) {
        let inner = self.lock();
        let latest = inner.next_sequence;
        let events = inner
            .events
            .iter()
            .filter(|event| event.sequence > sequence)
            .cloned()
            .collect();
        (latest, events)
    }

    pub(crate) fn subscribe(&self) -> watch::Receiver<u64> {
        self.changed.subscribe()
    }

    fn lock(&self) -> MutexGuard<'_, TimerEventBuffer> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn immediate(at: i64) -> TimerLifecycleEvent {
        TimerLifecycleEvent::immediate(TimerLifecycleKind::Delivered, 1, 2, at)
    }

    #[test]
    fn cursors_receive_ordered_events_without_consuming_other_clients() {
        let hub = TimerLifecycleHub::default();
        hub.publish(immediate(10));
        hub.publish(immediate(20));

        let (latest, first_client) = hub.events_since(0);
        assert_eq!(latest, 2);
        assert_eq!(
            first_client
                .iter()
                .map(|event| event.at)
                .collect::<Vec<_>>(),
            [10, 20]
        );
        let (_, second_client) = hub.events_since(1);
        assert_eq!(second_client.len(), 1);
        assert_eq!(second_client[0].sequence, 2);
    }

    #[tokio::test]
    async fn subscribers_wake_for_new_timer_activity() {
        let invalidations = StatusInvalidationHub::default();
        let hub = TimerLifecycleHub::new(invalidations.clone());
        let mut changed = hub.subscribe();

        hub.publish(immediate(10));

        changed.changed().await.unwrap();
        assert_eq!(*changed.borrow_and_update(), 1);
        assert_eq!(invalidations.version_at(10), 1);
    }
}
