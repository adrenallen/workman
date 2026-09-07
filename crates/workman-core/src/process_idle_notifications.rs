//! User-armed, durable, one-shot process alerts. Silence alone is not completion.

use rusqlite::{OptionalExtension, params};

use crate::{ProcessId, Store, StoreResult};

const SETTLE_MS: i64 = 1_000;

impl Store {
    pub fn process_idle_watch_enabled(&self, process_id: ProcessId) -> StoreResult<bool> {
        Ok(self.connection().query_row(
            "SELECT EXISTS(SELECT 1 FROM process_idle_watches WHERE process_id = ?1)",
            [process_id],
            |row| row.get(0),
        )?)
    }

    pub fn has_process_idle_watches(&self) -> StoreResult<bool> {
        Ok(self.connection().query_row(
            "SELECT EXISTS(SELECT 1 FROM process_idle_watches)",
            [],
            |row| row.get(0),
        )?)
    }

    pub fn set_process_idle_watch(
        &self,
        process_id: ProcessId,
        enabled: bool,
        busy: bool,
        now: i64,
    ) -> StoreResult<()> {
        if enabled {
            // Repeated enable requests must not erase a work cycle already observed.
            self.connection().execute(
                "INSERT OR IGNORE INTO process_idle_watches (process_id, armed_at, saw_busy) VALUES (?1, ?2, ?3)",
                params![process_id, now, busy],
            )?;
        } else {
            self.connection().execute(
                "DELETE FROM process_idle_watches WHERE process_id = ?1",
                [process_id],
            )?;
        }
        Ok(())
    }

    /// `None` means the platform cannot establish readiness; it must never fire an alert.
    pub fn observe_process_idle_watch(
        &self,
        process_id: ProcessId,
        ready: Option<bool>,
        now: i64,
    ) -> StoreResult<bool> {
        let transaction = self.connection().unchecked_transaction()?;
        let watch: Option<(bool, Option<i64>)> = transaction
            .query_row(
                "SELECT saw_busy, ready_since FROM process_idle_watches WHERE process_id = ?1",
                [process_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((saw_busy, ready_since)) = watch else {
            return Ok(false);
        };
        if ready != Some(true) || !saw_busy {
            transaction.execute(
                "UPDATE process_idle_watches SET saw_busy = saw_busy OR ?2, ready_since = NULL WHERE process_id = ?1",
                params![process_id, ready == Some(false)],
            )?;
        } else if ready_since.is_none_or(|since| now.saturating_sub(since) < SETTLE_MS) {
            transaction.execute(
                "UPDATE process_idle_watches SET ready_since = COALESCE(ready_since, ?2) WHERE process_id = ?1",
                params![process_id, now],
            )?;
        } else {
            transaction.execute(
                "INSERT INTO notifications (type, project_id, process_id, body, created_at)
                 SELECT 'process_idle', project_id, id, name || ' is ready for you.', ?2 FROM processes WHERE id = ?1",
                params![process_id, now],
            )?;
            transaction.execute(
                "UPDATE agent_notifications SET unread = 1, unread_at = ?2, last_notified_at = ?2 WHERE process_id = ?1",
                params![process_id, now],
            )?;
            transaction.execute(
                "DELETE FROM process_idle_watches WHERE process_id = ?1",
                [process_id],
            )?;
            transaction.commit()?;
            return Ok(true);
        }
        transaction.commit()?;
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NotificationType;

    fn fixture() -> Store {
        let store = Store::open_in_memory().unwrap();
        store
            .connection()
            .execute_batch(
                "INSERT INTO projects (id, path, name) VALUES (1, '/tmp/idle-watch', 'Fixture');
             INSERT INTO processes (id, project_id, kind, name, working_dir, source, status)
             VALUES (1, 1, 'terminal', 'Database', '/tmp/idle-watch', 'local', 'running');",
            )
            .unwrap();
        store
    }

    #[test]
    fn watches_wait_for_work_then_stable_readiness_and_fire_only_once() {
        let store = fixture();
        store.set_process_idle_watch(1, true, false, 0).unwrap();
        for now in [0, 1_000, 10_000] {
            assert!(
                !store
                    .observe_process_idle_watch(1, Some(true), now)
                    .unwrap()
            );
        }
        assert!(
            !store
                .observe_process_idle_watch(1, Some(false), 11_000)
                .unwrap()
        );
        assert!(
            !store
                .observe_process_idle_watch(1, Some(true), 12_000)
                .unwrap()
        );
        assert!(!store.observe_process_idle_watch(1, None, 12_999).unwrap());
        assert!(
            !store
                .observe_process_idle_watch(1, Some(true), 14_000)
                .unwrap()
        );
        assert!(
            store
                .observe_process_idle_watch(1, Some(true), 15_000)
                .unwrap()
        );
        assert!(!store.process_idle_watch_enabled(1).unwrap());
        assert!(
            !store
                .observe_process_idle_watch(1, Some(true), 20_000)
                .unwrap()
        );
        let notifications = store.list_notifications(None, 100).unwrap();
        assert_eq!(notifications.len(), 1);
        assert_eq!(notifications[0].kind, NotificationType::ProcessIdle);
        assert_eq!(notifications[0].process_id, Some(1));
    }

    #[test]
    fn cancellation_and_process_deletion_remove_pending_alerts() {
        let store = fixture();
        store.set_process_idle_watch(1, true, true, 0).unwrap();
        store.set_process_idle_watch(1, false, false, 1).unwrap();
        assert!(
            !store
                .observe_process_idle_watch(1, Some(true), 3_000)
                .unwrap()
        );
        assert!(!store.has_process_idle_watches().unwrap());
        store.set_process_idle_watch(1, true, true, 4_000).unwrap();
        store.delete_process(1).unwrap();
        assert!(!store.has_process_idle_watches().unwrap());
        assert!(store.list_notifications(None, 100).unwrap().is_empty());
    }
}
