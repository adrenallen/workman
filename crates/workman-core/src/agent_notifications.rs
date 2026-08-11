//! Durable notification state for completed agent turns.

use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{OptionalExtension, params};

use crate::{ProcessId, Store, StoreResult, TimerId, attention::AttentionState};

/// Backstop against repeated completion notifications for one process when an
/// adapter or client oscillates despite attention hysteresis.
const AGENT_DONE_NOTIFICATION_COOLDOWN_MS: i64 = 5 * 60 * 1_000;

/// Persisted human-attention metadata for one agent.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AgentNotificationState {
    pub unread: bool,
    pub unread_at: Option<i64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ObservedState {
    Working,
    NeedsInput,
    Idle,
    Exited,
}

impl ObservedState {
    fn from_attention(state: AttentionState) -> Self {
        match state {
            AttentionState::Working => Self::Working,
            AttentionState::NeedsInput => Self::NeedsInput,
            AttentionState::Waiting | AttentionState::Idle => Self::Idle,
            AttentionState::Exited => Self::Exited,
        }
    }

    const fn as_str(self) -> &'static str {
        match self {
            Self::Working => "working",
            Self::NeedsInput => "needs_input",
            Self::Idle => "idle",
            Self::Exited => "exited",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "working" => Some(Self::Working),
            "needs_input" => Some(Self::NeedsInput),
            "idle" => Some(Self::Idle),
            "exited" => Some(Self::Exited),
            _ => None,
        }
    }
}

impl Store {
    /// Remember that an idle watcher was consumed by this process's current
    /// completion transition. The marker bridges the gap between timer
    /// delivery and the debounced notification decision.
    pub fn record_consumed_idle_watch(
        &self,
        process_id: ProcessId,
        timer_id: TimerId,
        fired_at: i64,
    ) -> StoreResult<()> {
        self.connection().execute(
            "INSERT INTO consumed_idle_watches (process_id, timer_id, fired_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(process_id, timer_id) DO UPDATE SET fired_at = excluded.fired_at",
            params![process_id, timer_id, fired_at],
        )?;
        Ok(())
    }

    /// Observe the latest attention state and persist an unread completion edge.
    ///
    /// A newly discovered process establishes a baseline rather than notifying.
    /// Thereafter, working-to-idle and working-to-exited are completion edges.
    /// Watched completions are intentionally suppressed because a timer will
    /// wake an agent to react. Starting work clears any older unread edge.
    pub fn observe_agent_attention(
        &self,
        process_id: ProcessId,
        state: AttentionState,
        watched: bool,
        turn_started: bool,
        now_ms: i64,
    ) -> StoreResult<AgentNotificationState> {
        self.observe_agent_attention_with_activity(
            process_id,
            state,
            watched,
            turn_started,
            None,
            now_ms,
        )
    }

    /// Observe attention while carrying the timestamp of the newest real PTY
    /// output/input. A consumed watch remains valid through debounce polls and
    /// is reset only by activity newer than the watch fire.
    pub fn observe_agent_attention_with_activity(
        &self,
        process_id: ProcessId,
        state: AttentionState,
        watched: bool,
        turn_started: bool,
        last_agent_activity_at: Option<i64>,
        now_ms: i64,
    ) -> StoreResult<AgentNotificationState> {
        let current = ObservedState::from_attention(state);
        let previous = self
            .connection()
            .query_row(
                "SELECT observed_state, unread, unread_at,
                        last_notified_at, last_viewed_at
                 FROM agent_notifications
                 WHERE process_id = ?1",
                [process_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, bool>(1)?,
                        row.get::<_, Option<i64>>(2)?,
                        row.get::<_, Option<i64>>(3)?,
                        row.get::<_, Option<i64>>(4)?,
                    ))
                },
            )
            .optional()?;

        let Some((previous_state, mut unread, mut unread_at, mut last_notified_at, last_viewed_at)) =
            previous
        else {
            self.connection().execute(
                "INSERT INTO agent_notifications
                    (process_id, observed_state, unread, unread_at)
                 VALUES (?1, ?2, 0, NULL)",
                params![process_id, current.as_str()],
            )?;
            return Ok(AgentNotificationState {
                unread: false,
                unread_at: None,
            });
        };
        let previous = ObservedState::parse(&previous_state).unwrap_or(current);
        if let Some(activity_at) = last_agent_activity_at {
            self.connection().execute(
                "DELETE FROM consumed_idle_watches
                 WHERE process_id = ?1 AND fired_at < ?2",
                params![process_id, activity_at],
            )?;
        }

        if current == ObservedState::Working {
            if unread {
                self.mark_process_notifications_read(process_id, now_ms)?;
            }
            unread = false;
            unread_at = None;
        } else {
            let needs_input =
                previous == ObservedState::Working && current == ObservedState::NeedsInput;
            let completed_turn = previous == ObservedState::Working
                && current == ObservedState::Idle
                && turn_started;
            let exited = previous == ObservedState::Working
                && current == ObservedState::Exited
                && turn_started;
            let cooldown_elapsed = last_notified_at.is_none_or(|notified_at| {
                now_ms.saturating_sub(notified_at) >= AGENT_DONE_NOTIFICATION_COOLDOWN_MS
            });
            let viewed_since_notification = last_viewed_at
                .zip(last_notified_at)
                .is_some_and(|(viewed_at, notified_at)| viewed_at >= notified_at);
            let consumed_watch =
                (completed_turn || exited) && self.has_consumed_idle_watch(process_id)?;
            if (completed_turn || exited)
                && !watched
                && !consumed_watch
                && (cooldown_elapsed || viewed_since_notification)
            {
                unread = true;
                unread_at = Some(now_ms);
                last_notified_at = Some(now_ms);
                self.create_agent_done_notification(process_id, now_ms)?;
            }
            if needs_input && !watched {
                unread = true;
                unread_at = Some(now_ms);
                // Needs-input edges intentionally do not advance the separate
                // completion cooldown. A fresh completion after the user
                // responds is still eligible for its own notification.
                self.create_agent_needs_input_notification(process_id, now_ms)?;
            }
            if completed_turn || exited {
                // A marker is transition-scoped. Once the matching completion
                // decision sees it, it cannot suppress another edge.
                self.clear_consumed_idle_watches(process_id)?;
            }
        }

        self.connection().execute(
            "UPDATE agent_notifications
             SET observed_state = ?2, unread = ?3, unread_at = ?4,
                 last_notified_at = ?5
             WHERE process_id = ?1",
            params![
                process_id,
                current.as_str(),
                unread,
                unread_at,
                last_notified_at
            ],
        )?;
        Ok(AgentNotificationState { unread, unread_at })
    }

    fn has_consumed_idle_watch(&self, process_id: ProcessId) -> StoreResult<bool> {
        self.connection()
            .query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM consumed_idle_watches WHERE process_id = ?1
                 )",
                [process_id],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    fn clear_consumed_idle_watches(&self, process_id: ProcessId) -> StoreResult<()> {
        self.connection().execute(
            "DELETE FROM consumed_idle_watches WHERE process_id = ?1",
            [process_id],
        )?;
        Ok(())
    }

    /// Clear a process's durable unread completion marker.
    pub fn mark_agent_read(&self, process_id: ProcessId) -> StoreResult<bool> {
        let was_unread = self
            .connection()
            .query_row(
                "SELECT unread FROM agent_notifications WHERE process_id = ?1",
                [process_id],
                |row| row.get::<_, bool>(0),
            )
            .optional()?
            .unwrap_or(false);
        let updated = self.connection().execute(
            "UPDATE agent_notifications
             SET unread = 0, unread_at = NULL, last_viewed_at = ?2
             WHERE process_id = ?1",
            params![process_id, now_millis()],
        )?;
        self.mark_process_notifications_read(process_id, now_millis())?;
        Ok(updated > 0 && was_unread)
    }
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::{Process, ProcessKind, ProcessSource, ProcessStatus, Project};

    use super::*;

    fn fixture() -> Store {
        let store = Store::open_in_memory().expect("open store");
        put_fixture(&store);
        store
    }

    fn put_fixture(store: &Store) {
        store
            .put_project(&Project {
                id: 1,
                path: "/tmp/workman-unread-agent".into(),
                name: "unread agent".into(),
                display_name: None,
                icon: None,
                selected: true,
                sort_order: 0,
            })
            .expect("put project");
        store
            .put_process(&Process {
                id: 7,
                project_id: 1,
                kind: ProcessKind::Agent,
                name: "worker".into(),
                command: Some("true".into()),
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
                agent_tool_id: None,
                spawned_by_process_id: None,
                sort_order: 0,
            })
            .expect("put process");
    }

    #[test]
    fn unobserved_completion_is_unread_until_viewed_or_work_resumes() {
        let store = fixture();
        assert!(
            !store
                .observe_agent_attention(7, AttentionState::Working, false, true, 10)
                .unwrap()
                .unread
        );
        let done = store
            .observe_agent_attention(7, AttentionState::Idle, false, true, 20)
            .unwrap();
        assert_eq!(
            done,
            AgentNotificationState {
                unread: true,
                unread_at: Some(20)
            }
        );
        let unread = store.list_notifications(Some(false), 10).unwrap();
        assert_eq!(unread.len(), 1);
        assert_eq!(unread[0].kind, crate::NotificationType::AgentDone);
        assert_eq!(unread[0].process_id, Some(7));
        assert_eq!(unread[0].body, "worker finished and has unread output.");

        assert!(store.mark_agent_read(7).unwrap());
        assert!(
            store
                .list_notifications(Some(false), 10)
                .unwrap()
                .is_empty()
        );
        assert_eq!(store.list_notifications(Some(true), 10).unwrap().len(), 1);
        assert!(
            !store
                .observe_agent_attention(7, AttentionState::Idle, false, true, 30)
                .unwrap()
                .unread
        );

        store
            .observe_agent_attention(7, AttentionState::Working, false, true, 40)
            .unwrap();
        assert!(
            !store
                .observe_agent_attention(7, AttentionState::Working, false, true, 50)
                .unwrap()
                .unread
        );
    }

    #[test]
    fn watched_completion_is_suppressed_but_unwatched_exit_is_unread() {
        let store = fixture();
        store
            .observe_agent_attention(7, AttentionState::Working, false, true, 10)
            .unwrap();
        assert!(
            !store
                .observe_agent_attention(7, AttentionState::Waiting, true, true, 20)
                .unwrap()
                .unread
        );
        assert!(store.list_notifications(None, 10).unwrap().is_empty());

        store
            .observe_agent_attention(7, AttentionState::Working, false, true, 30)
            .unwrap();
        assert!(
            store
                .observe_agent_attention(7, AttentionState::Exited, false, true, 40)
                .unwrap()
                .unread
        );
        assert!(
            store
                .observe_agent_attention(7, AttentionState::Exited, false, true, 50)
                .unwrap()
                .unread
        );
    }

    #[test]
    fn needs_input_notifies_only_after_fresh_work_and_respects_watching() {
        let store = fixture();
        store
            .observe_agent_attention(7, AttentionState::Working, false, false, 10)
            .unwrap();

        let needs_input = store
            .observe_agent_attention(7, AttentionState::NeedsInput, false, false, 20)
            .unwrap();
        assert_eq!(
            needs_input,
            AgentNotificationState {
                unread: true,
                unread_at: Some(20)
            }
        );
        let notifications = store.list_notifications(None, 10).unwrap();
        assert_eq!(notifications.len(), 1);
        assert_eq!(notifications[0].kind, crate::NotificationType::NeedsInput);
        assert_eq!(notifications[0].body, "worker needs your input.");

        store
            .observe_agent_attention(7, AttentionState::NeedsInput, false, false, 30)
            .unwrap();
        store
            .observe_agent_attention(7, AttentionState::Idle, false, false, 40)
            .unwrap();
        store
            .observe_agent_attention(7, AttentionState::NeedsInput, false, false, 50)
            .unwrap();
        assert_eq!(
            store.list_notifications(None, 10).unwrap().len(),
            1,
            "leaving needs-input without fresh work must not re-notify"
        );

        store
            .observe_agent_attention(7, AttentionState::Working, false, true, 60)
            .unwrap();
        assert!(
            store
                .list_notifications(Some(false), 10)
                .unwrap()
                .is_empty()
        );
        store
            .observe_agent_attention(7, AttentionState::NeedsInput, true, true, 70)
            .unwrap();
        assert_eq!(
            store.list_notifications(None, 10).unwrap().len(),
            1,
            "watched needs-input edges must remain suppressed"
        );

        store
            .observe_agent_attention(7, AttentionState::Working, false, true, 80)
            .unwrap();
        let second = store
            .observe_agent_attention(7, AttentionState::NeedsInput, false, true, 90)
            .unwrap();
        assert!(second.unread);
        assert_eq!(store.list_notifications(None, 10).unwrap().len(), 2);
    }

    #[test]
    fn needs_input_does_not_consume_the_completion_cooldown() {
        let store = fixture();
        store
            .observe_agent_attention(7, AttentionState::Working, false, true, 10)
            .unwrap();
        store
            .observe_agent_attention(7, AttentionState::NeedsInput, false, true, 20)
            .unwrap();
        store
            .observe_agent_attention(7, AttentionState::Working, false, true, 30)
            .unwrap();
        let completed = store
            .observe_agent_attention(7, AttentionState::Idle, false, true, 40)
            .unwrap();

        assert!(completed.unread);
        let notifications = store.list_notifications(None, 10).unwrap();
        assert_eq!(notifications.len(), 2);
        assert_eq!(notifications[0].kind, crate::NotificationType::AgentDone);
        assert_eq!(notifications[1].kind, crate::NotificationType::NeedsInput);
    }

    #[test]
    fn first_observation_never_creates_a_historical_notification() {
        let store = fixture();
        assert!(
            !store
                .observe_agent_attention(7, AttentionState::Exited, false, false, 10)
                .unwrap()
                .unread
        );
    }

    #[test]
    fn exiting_from_an_existing_idle_period_does_not_renotify() {
        let store = fixture();
        store
            .observe_agent_attention(7, AttentionState::Working, false, true, 10)
            .unwrap();
        store
            .observe_agent_attention(7, AttentionState::Idle, false, true, 20)
            .unwrap();
        assert!(store.mark_agent_read(7).unwrap());

        let exited = store
            .observe_agent_attention(
                7,
                AttentionState::Exited,
                false,
                true,
                20 + AGENT_DONE_NOTIFICATION_COOLDOWN_MS,
            )
            .unwrap();
        assert!(!exited.unread);
        assert_eq!(store.list_notifications(None, 10).unwrap().len(), 1);
    }

    #[test]
    fn notification_center_read_clears_the_agent_marker() {
        let store = fixture();
        store
            .observe_agent_attention(7, AttentionState::Working, false, true, 10)
            .unwrap();
        store
            .observe_agent_attention(7, AttentionState::Idle, false, true, 20)
            .unwrap();
        let notification = store.list_notifications(Some(false), 10).unwrap().remove(0);

        assert!(store.mark_notification_read(notification.id, 30).unwrap());
        assert!(
            store
                .list_notifications(Some(false), 10)
                .unwrap()
                .is_empty()
        );
        assert!(
            !store
                .observe_agent_attention(7, AttentionState::Idle, false, true, 40)
                .unwrap()
                .unread
        );

        store
            .observe_agent_attention(7, AttentionState::Working, false, true, 50)
            .unwrap();
        assert!(
            store
                .observe_agent_attention(7, AttentionState::Idle, false, true, 60)
                .unwrap()
                .unread
        );
        assert!(!store.mark_notification_read(notification.id, 70).unwrap());
        assert!(
            store
                .observe_agent_attention(7, AttentionState::Idle, false, true, 80)
                .unwrap()
                .unread,
            "marking an old history item again must not clear a newer completion"
        );
    }

    #[test]
    fn initial_prompt_quiescence_is_not_a_completed_turn() {
        let store = fixture();
        store
            .observe_agent_attention(7, AttentionState::Working, false, false, 10)
            .unwrap();
        assert!(
            !store
                .observe_agent_attention(7, AttentionState::Idle, false, false, 20)
                .unwrap()
                .unread
        );
    }

    #[test]
    fn rapid_completion_cycles_are_suppressed_until_the_cooldown_expires() {
        let store = fixture();
        store
            .observe_agent_attention(7, AttentionState::Working, false, true, 0)
            .unwrap();
        assert!(
            store
                .observe_agent_attention(7, AttentionState::Idle, false, true, 100)
                .unwrap()
                .unread
        );

        // Resumed activity keeps 381's self-clear behavior, but is not a user
        // view and therefore must not reset the notification backstop.
        store
            .observe_agent_attention(7, AttentionState::Working, false, true, 200)
            .unwrap();
        assert!(
            !store
                .observe_agent_attention(7, AttentionState::Idle, false, true, 300)
                .unwrap()
                .unread
        );
        assert_eq!(store.list_notifications(None, 10).unwrap().len(), 1);

        store
            .observe_agent_attention(7, AttentionState::Working, false, true, 400)
            .unwrap();
        let after_cooldown = 100 + AGENT_DONE_NOTIFICATION_COOLDOWN_MS;
        assert!(
            store
                .observe_agent_attention(7, AttentionState::Idle, false, true, after_cooldown,)
                .unwrap()
                .unread
        );
        assert_eq!(store.list_notifications(None, 10).unwrap().len(), 2);
    }

    #[test]
    fn explicit_user_view_allows_the_next_sustained_completion() {
        let store = fixture();
        store
            .observe_agent_attention(7, AttentionState::Working, false, true, 0)
            .unwrap();
        store
            .observe_agent_attention(7, AttentionState::Idle, false, true, 100)
            .unwrap();
        let notification = store.list_notifications(None, 10).unwrap().remove(0);
        assert!(store.mark_notification_read(notification.id, 150).unwrap());

        store
            .observe_agent_attention(7, AttentionState::Working, false, true, 200)
            .unwrap();
        assert!(
            store
                .observe_agent_attention(7, AttentionState::Idle, false, true, 300)
                .unwrap()
                .unread
        );
        assert_eq!(store.list_notifications(None, 10).unwrap().len(), 2);
    }

    #[test]
    fn viewing_after_activity_self_clear_still_resets_the_backstop() {
        let store = fixture();
        store
            .observe_agent_attention(7, AttentionState::Working, false, true, 0)
            .unwrap();
        store
            .observe_agent_attention(7, AttentionState::Idle, false, true, 100)
            .unwrap();
        store
            .observe_agent_attention(7, AttentionState::Working, false, true, 200)
            .unwrap();

        assert!(
            !store.mark_agent_read(7).unwrap(),
            "activity already cleared unread"
        );
        assert!(
            store
                .observe_agent_attention(7, AttentionState::Idle, false, true, 300)
                .unwrap()
                .unread,
            "opening the process is an intervening user view even after self-clear"
        );
    }

    #[test]
    fn unread_marker_survives_store_reopen() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("workman.sqlite3");
        {
            let store = Store::open(&path).unwrap();
            put_fixture(&store);
            store
                .observe_agent_attention(7, AttentionState::Working, false, true, 10)
                .unwrap();
            assert!(
                store
                    .observe_agent_attention(7, AttentionState::Idle, false, true, 20)
                    .unwrap()
                    .unread
            );
        }

        let store = Store::open(&path).unwrap();
        assert!(
            store
                .observe_agent_attention(7, AttentionState::Idle, false, true, 30)
                .unwrap()
                .unread
        );
    }
}
