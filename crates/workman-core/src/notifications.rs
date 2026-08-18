//! Durable, agent-agnostic notifications shown in the desktop notification center.

use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::{ProcessId, ProjectId, Store, StoreResult, TodoCommentId, TodoId};

pub type NotificationId = i64;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationType {
    AgentDone,
    NeedsInput,
    ProcessCrashed,
    TimerFired,
    TodoAssignedToYou,
    MentionedInComment,
}

impl NotificationType {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::AgentDone => "agent_done",
            Self::NeedsInput => "needs_input",
            Self::ProcessCrashed => "process_crashed",
            Self::TimerFired => "timer_fired",
            Self::TodoAssignedToYou => "todo_assigned_to_you",
            Self::MentionedInComment => "mentioned_in_comment",
        }
    }

    fn parse(value: &str) -> Self {
        match value {
            "needs_input" => Self::NeedsInput,
            "process_crashed" => Self::ProcessCrashed,
            "timer_fired" => Self::TimerFired,
            "todo_assigned_to_you" => Self::TodoAssignedToYou,
            "mentioned_in_comment" => Self::MentionedInComment,
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
    pub comment_id: Option<TodoCommentId>,
    pub body: String,
    pub created_at: i64,
    pub read_at: Option<i64>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProjectMarkReadResult {
    pub notifications_updated: usize,
    pub processes_updated: usize,
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

    pub(crate) fn create_agent_needs_input_notification(
        &self,
        process_id: ProcessId,
        created_at: i64,
    ) -> StoreResult<Option<NotificationId>> {
        let inserted = self.connection().execute(
            "INSERT INTO notifications (type, project_id, process_id, body, created_at)
             SELECT ?2, project_id, id, name || ' needs your input.', ?3
             FROM processes
             WHERE id = ?1",
            params![
                process_id,
                NotificationType::NeedsInput.as_str(),
                created_at
            ],
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
            "SELECT id, type, project_id, process_id, todo_id, comment_id, body, created_at, read_at
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
                comment_id: row.get(5)?,
                body: row.get(6)?,
                created_at: row.get(7)?,
                read_at: row.get(8)?,
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
        if updated > 0 {
            self.connection().execute(
                "UPDATE agent_notifications
                 SET last_viewed_at = ?2
                 WHERE process_id = (
                     SELECT process_id FROM notifications WHERE id = ?1
                 )",
                params![notification_id, read_at],
            )?;
        }
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
        self.connection().execute(
            "UPDATE agent_notifications
             SET last_viewed_at = ?1
             WHERE process_id IN (
                 SELECT process_id FROM notifications WHERE read_at IS NULL
             )",
            [read_at],
        )?;
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

    /// Mark notifications and durable per-process attention for exactly one project read.
    pub fn mark_project_read(
        &self,
        project_id: ProjectId,
        read_at: i64,
    ) -> StoreResult<ProjectMarkReadResult> {
        let transaction = self.connection().unchecked_transaction()?;
        transaction.execute(
            "UPDATE agent_notifications
             SET last_viewed_at = ?2
             WHERE unread = 1
               AND process_id IN (
                   SELECT id FROM processes WHERE project_id = ?1
               )",
            params![project_id, read_at],
        )?;
        let notifications_updated = transaction.execute(
            "UPDATE notifications
             SET read_at = ?2
             WHERE project_id = ?1 AND read_at IS NULL",
            params![project_id, read_at],
        )?;
        let processes_updated = transaction.execute(
            "UPDATE agent_notifications
             SET unread = 0, unread_at = NULL
             WHERE unread = 1
               AND process_id IN (
                   SELECT id FROM processes WHERE project_id = ?1
               )",
            [project_id],
        )?;
        transaction.commit()?;
        Ok(ProjectMarkReadResult {
            notifications_updated,
            processes_updated,
        })
    }
}

#[cfg(test)]
mod project_mark_read_tests {
    use std::collections::BTreeMap;

    use crate::{
        Process, ProcessKind, ProcessSource, ProcessStatus, Project, Store,
        attention::AttentionState,
    };

    fn put_project_agent(store: &Store, project_id: i64, process_id: i64) {
        store
            .put_project(&Project {
                id: project_id,
                path: format!("/tmp/project-{project_id}"),
                name: format!("project-{project_id}"),
                display_name: None,
                icon: None,
                selected: project_id == 1,
                sort_order: project_id,
            })
            .unwrap();
        store
            .put_process(&Process {
                id: process_id,
                project_id,
                kind: ProcessKind::Agent,
                name: format!("agent-{process_id}"),
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
            .unwrap();
        store
            .observe_agent_attention(process_id, AttentionState::Working, false, true, 10)
            .unwrap();
        assert!(
            store
                .observe_agent_attention(process_id, AttentionState::Idle, false, true, 20)
                .unwrap()
                .unread
        );
    }

    #[test]
    fn project_mark_read_clears_only_that_projects_notifications_and_process_markers() {
        let store = Store::open_in_memory().unwrap();
        put_project_agent(&store, 1, 11);
        put_project_agent(&store, 2, 22);

        let result = store.mark_project_read(1, 100).unwrap();

        assert_eq!(result.notifications_updated, 1);
        assert_eq!(result.processes_updated, 1);
        let unread = store.list_notifications(Some(false), 10).unwrap();
        assert_eq!(unread.len(), 1);
        assert_eq!(unread[0].project_id, Some(2));
        let marker = |process_id| {
            store
                .connection()
                .query_row(
                    "SELECT unread FROM agent_notifications WHERE process_id = ?1",
                    [process_id],
                    |row| row.get::<_, bool>(0),
                )
                .unwrap()
        };
        assert!(!marker(11));
        assert!(marker(22));
    }
}
