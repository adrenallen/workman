//! Durable, agent-agnostic notifications shown in the desktop notification center.

use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::{ProcessId, ProjectId, Store, StoreResult, TodoId};

pub type NotificationId = i64;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationType {
    AgentDone,
    ProcessCrashed,
    TimerFired,
}

impl NotificationType {
    const fn as_str(self) -> &'static str {
        match self {
            Self::AgentDone => "agent_done",
            Self::ProcessCrashed => "process_crashed",
            Self::TimerFired => "timer_fired",
        }
    }

    fn parse(value: &str) -> Self {
        match value {
            "process_crashed" => Self::ProcessCrashed,
            "timer_fired" => Self::TimerFired,
            _ => Self::AgentDone,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Notification {
    pub id: NotificationId,
    #[serde(rename = "type")]
    pub kind: NotificationType,
    pub project_id: Option<ProjectId>,
    pub process_id: Option<ProcessId>,
    pub todo_id: Option<TodoId>,
    pub body: String,
    pub created_at: i64,
    pub read_at: Option<i64>,
}

impl Store {
    pub(crate) fn create_agent_done_notification(
        &self,
        process_id: ProcessId,
        created_at: i64,
    ) -> StoreResult<Option<NotificationId>> {
        let inserted = self.connection().execute(
            "INSERT INTO notifications (type, project_id, process_id, body, created_at)
             SELECT ?2, project_id, id, name || ' finished and has unread output.', ?3
             FROM processes
             WHERE id = ?1",
            params![process_id, NotificationType::AgentDone.as_str(), created_at],
        )?;
        Ok((inserted > 0).then(|| self.connection().last_insert_rowid()))
    }

    pub(crate) fn mark_process_notifications_read(
        &self,
        process_id: ProcessId,
        read_at: i64,
    ) -> StoreResult<usize> {
        Ok(self.connection().execute(
            "UPDATE notifications
             SET read_at = ?2
             WHERE process_id = ?1 AND read_at IS NULL",
            params![process_id, read_at],
        )?)
    }

    pub fn list_notifications(
        &self,
        read: Option<bool>,
        limit: usize,
    ) -> StoreResult<Vec<Notification>> {
        let mut statement = self.connection().prepare(
            "SELECT id, type, project_id, process_id, todo_id, body, created_at, read_at
             FROM notifications
             WHERE (?1 IS NULL)
                OR (?1 = 0 AND read_at IS NULL)
                OR (?1 = 1 AND read_at IS NOT NULL)
             ORDER BY created_at DESC, id DESC
             LIMIT ?2",
        )?;
        let rows = statement.query_map(params![read, limit.clamp(1, 200)], |row| {
            Ok(Notification {
                id: row.get(0)?,
                kind: NotificationType::parse(&row.get::<_, String>(1)?),
                project_id: row.get(2)?,
                process_id: row.get(3)?,
                todo_id: row.get(4)?,
                body: row.get(5)?,
                created_at: row.get(6)?,
                read_at: row.get(7)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Mark one notification read and clear the matching agent unread marker.
    pub fn mark_notification_read(
        &self,
        notification_id: NotificationId,
        read_at: i64,
    ) -> StoreResult<bool> {
        let updated = self.connection().execute(
            "UPDATE notifications SET read_at = ?2
             WHERE id = ?1 AND read_at IS NULL",
            params![notification_id, read_at],
        )?;
        self.connection().execute(
            "UPDATE agent_notifications
             SET unread = 0, unread_at = NULL
             WHERE process_id = (
                 SELECT process_id FROM notifications WHERE id = ?1
             )
               AND NOT EXISTS (
                 SELECT 1 FROM notifications AS unread
                 WHERE unread.process_id = agent_notifications.process_id
                   AND unread.read_at IS NULL
             )",
            [notification_id],
        )?;
        Ok(updated > 0)
    }

    /// Mark every notification read and clear all corresponding agent markers.
    pub fn mark_all_notifications_read(&self, read_at: i64) -> StoreResult<usize> {
        let updated = self.connection().execute(
            "UPDATE notifications SET read_at = ?1 WHERE read_at IS NULL",
            [read_at],
        )?;
        self.connection().execute(
            "UPDATE agent_notifications SET unread = 0, unread_at = NULL WHERE unread = 1",
            [],
        )?;
        Ok(updated)
    }
}
