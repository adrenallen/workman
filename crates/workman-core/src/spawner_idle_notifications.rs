//! Durable opt-in settings for child-to-spawner orchestration notifications.
//!
//! This is deliberately separate from `process_idle_watches`, which represents the desktop
//! user's one-shot "notify me when idle" alert. Completed-turn identity/reporting belongs to the
//! completion ledger; this table stores only the opt-in and delivery state for attention/exit
//! reasons.

use rusqlite::{OptionalExtension, params};

use crate::{ProcessId, Store, StoreResult};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SpawnerIdleNotificationSetting {
    pub process_id: ProcessId,
    pub enabled_at: i64,
    pub last_reported_state: SpawnerReportedState,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SpawnerReportedState {
    #[default]
    Neutral,
    NeedsInput,
    Exited,
    Crashed,
}

impl SpawnerReportedState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Neutral => "neutral",
            Self::NeedsInput => "needs_input",
            Self::Exited => "exited",
            Self::Crashed => "crashed",
        }
    }

    fn parse(value: &str) -> rusqlite::Result<Self> {
        match value {
            "neutral" => Ok(Self::Neutral),
            "needs_input" => Ok(Self::NeedsInput),
            "exited" => Ok(Self::Exited),
            "crashed" => Ok(Self::Crashed),
            other => Err(rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                format!("unknown spawner notification state {other:?}").into(),
            )),
        }
    }
}

impl Store {
    pub fn spawner_idle_notification_enabled(&self, process_id: ProcessId) -> StoreResult<bool> {
        Ok(self.connection().query_row(
            "SELECT EXISTS(
                SELECT 1 FROM process_spawner_idle_notifications WHERE process_id = ?1
             )",
            [process_id],
            |row| row.get(0),
        )?)
    }

    pub fn set_spawner_idle_notification(
        &self,
        process_id: ProcessId,
        enabled: bool,
        now: i64,
    ) -> StoreResult<()> {
        if enabled {
            // Repeated enable calls preserve the original arm boundary and delivered state.
            self.connection().execute(
                "INSERT OR IGNORE INTO process_spawner_idle_notifications
                    (process_id, enabled_at, last_reported_state)
                 VALUES (?1, ?2, 'neutral')",
                params![process_id, now],
            )?;
        } else {
            self.connection().execute(
                "DELETE FROM process_spawner_idle_notifications WHERE process_id = ?1",
                [process_id],
            )?;
        }
        Ok(())
    }

    pub fn list_spawner_idle_notification_settings(
        &self,
    ) -> StoreResult<Vec<SpawnerIdleNotificationSetting>> {
        let mut statement = self.connection().prepare(
            "SELECT process_id, enabled_at, last_reported_state
             FROM process_spawner_idle_notifications
             ORDER BY process_id",
        )?;
        let rows = statement.query_map([], |row| {
            let state = row.get::<_, String>(2)?;
            Ok(SpawnerIdleNotificationSetting {
                process_id: row.get(0)?,
                enabled_at: row.get(1)?,
                last_reported_state: SpawnerReportedState::parse(&state)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn set_spawner_reported_state(
        &self,
        process_id: ProcessId,
        state: SpawnerReportedState,
    ) -> StoreResult<bool> {
        Ok(self.connection().execute(
            "UPDATE process_spawner_idle_notifications
             SET last_reported_state = ?2
             WHERE process_id = ?1",
            params![process_id, state.as_str()],
        )? > 0)
    }

    pub fn spawner_idle_notification_setting(
        &self,
        process_id: ProcessId,
    ) -> StoreResult<Option<SpawnerIdleNotificationSetting>> {
        Ok(self
            .connection()
            .query_row(
                "SELECT process_id, enabled_at, last_reported_state
                 FROM process_spawner_idle_notifications
                 WHERE process_id = ?1",
                [process_id],
                |row| {
                    let state = row.get::<_, String>(2)?;
                    Ok(SpawnerIdleNotificationSetting {
                        process_id: row.get(0)?,
                        enabled_at: row.get(1)?,
                        last_reported_state: SpawnerReportedState::parse(&state)?,
                    })
                },
            )
            .optional()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Store {
        let store = Store::open_in_memory().unwrap();
        store
            .connection()
            .execute_batch(
                "INSERT INTO projects (id, path, name) VALUES (1, '/tmp/spawner', 'Fixture');
                 INSERT INTO processes (id, project_id, kind, name, working_dir, source, status)
                 VALUES (1, 1, 'agent', 'Parent', '/tmp/spawner', 'local', 'running'),
                        (2, 1, 'agent', 'Child', '/tmp/spawner', 'local', 'running');",
            )
            .unwrap();
        store
    }

    #[test]
    fn setting_is_durable_idempotent_and_removed_with_process() {
        let store = fixture();
        store.set_spawner_idle_notification(2, true, 10).unwrap();
        store.set_spawner_idle_notification(2, true, 20).unwrap();
        let setting = store.spawner_idle_notification_setting(2).unwrap().unwrap();
        assert_eq!(setting.enabled_at, 10);
        assert_eq!(setting.last_reported_state, SpawnerReportedState::Neutral);

        assert!(
            store
                .set_spawner_reported_state(2, SpawnerReportedState::NeedsInput)
                .unwrap()
        );
        assert_eq!(
            store.list_spawner_idle_notification_settings().unwrap()[0].last_reported_state,
            SpawnerReportedState::NeedsInput
        );

        store.delete_process(2).unwrap();
        assert!(!store.spawner_idle_notification_enabled(2).unwrap());
    }

    #[test]
    fn opt_in_survives_store_reopen() {
        let temp = tempfile::tempdir().unwrap();
        let database = temp.path().join("workman.sqlite3");
        {
            let store = Store::open(&database).unwrap();
            store
                .connection()
                .execute_batch(
                    "INSERT INTO projects (id, path, name)
                     VALUES (1, '/tmp/spawner-reopen', 'Fixture');
                     INSERT INTO processes
                         (id, project_id, kind, name, working_dir, source, status,
                          spawned_by_process_id)
                     VALUES (1, 1, 'agent', 'Parent', '/tmp/spawner-reopen', 'local', 'running', NULL),
                            (2, 1, 'agent', 'Child', '/tmp/spawner-reopen', 'local', 'running', 1);",
                )
                .unwrap();
            store.set_spawner_idle_notification(2, true, 10).unwrap();
        }
        let reopened = Store::open(&database).unwrap();
        assert!(reopened.spawner_idle_notification_enabled(2).unwrap());
        assert_eq!(
            reopened
                .spawner_idle_notification_setting(2)
                .unwrap()
                .unwrap()
                .enabled_at,
            10
        );
    }
}
