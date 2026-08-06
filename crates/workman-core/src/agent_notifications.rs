//! Durable notification state for completed agent turns.

use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{OptionalExtension, params};

use crate::{ProcessId, Store, StoreResult, attention::AttentionState};

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
    /// Observe the latest attention state and persist an unread completion edge.
    ///
    /// A newly discovered process establishes a baseline rather than notifying.
    /// Thereafter, working-to-idle and first entry into exited are completion
    /// edges. Watched completions are intentionally suppressed because a timer
    /// will wake an agent to react. Starting work clears any older unread edge.
    pub fn observe_agent_attention(
        &self,
        process_id: ProcessId,
        state: AttentionState,
        watched: bool,
        turn_started: bool,
        now_ms: i64,
    ) -> StoreResult<AgentNotificationState> {
        let current = ObservedState::from_attention(state);
        let previous = self
            .connection()
            .query_row(
                "SELECT observed_state, unread, unread_at
                 FROM agent_notifications
                 WHERE process_id = ?1",
                [process_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, bool>(1)?,
                        row.get::<_, Option<i64>>(2)?,
                    ))
                },
            )
            .optional()?;

        let Some((previous_state, mut unread, mut unread_at)) = previous else {
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

        if current == ObservedState::Working {
            if unread {
                self.mark_process_notifications_read(process_id, now_ms)?;
            }
            unread = false;
            unread_at = None;
        } else {
            let completed_turn = previous == ObservedState::Working
                && current == ObservedState::Idle
                && turn_started;
            let exited = current == ObservedState::Exited && previous != ObservedState::Exited;
            if (completed_turn || exited) && !watched {
                unread = true;
                unread_at = Some(now_ms);
                self.create_agent_done_notification(process_id, now_ms)?;
            }
        }

        self.connection().execute(
            "UPDATE agent_notifications
             SET observed_state = ?2, unread = ?3, unread_at = ?4
             WHERE process_id = ?1",
            params![process_id, current.as_str(), unread, unread_at],
        )?;
        Ok(AgentNotificationState { unread, unread_at })
    }

    /// Clear a process's durable unread completion marker.
    pub fn mark_agent_read(&self, process_id: ProcessId) -> StoreResult<bool> {
        let updated = self.connection().execute(
            "UPDATE agent_notifications
             SET unread = 0, unread_at = NULL
             WHERE process_id = ?1 AND unread = 1",
            [process_id],
        )? > 0;
        self.mark_process_notifications_read(process_id, now_millis())?;
        Ok(updated)
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
