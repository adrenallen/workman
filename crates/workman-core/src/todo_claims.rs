//! Process-linked todo claims used by status surfaces.

use serde::{Deserialize, Serialize};

use crate::{ProcessId, ProjectId, Store, StoreResult, TodoId};

/// A live todo lease held by an MCP actor attached to one process.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ClaimedTodo {
    pub id: TodoId,
    pub project_id: ProjectId,
    pub title: String,
    pub claimed_at: Option<i64>,
    pub lock_expiry: i64,
}

impl Store {
    /// Resolve every unexpired todo lease held by `process_id`.
    pub fn claimed_todos_for_process(
        &self,
        process_id: ProcessId,
        now_ms: i64,
    ) -> StoreResult<Vec<ClaimedTodo>> {
        let mut statement = self.connection().prepare(
            "SELECT todo.id, todo.project_id, todo.title,
                    todo.lock_acquired_at, todo.lock_expiry
             FROM todos AS todo
             WHERE todo.lock_process_id = ?1
               AND todo.lock_expiry IS NOT NULL
               AND todo.lock_expiry > ?2
             ORDER BY COALESCE(todo.lock_acquired_at, todo.lock_expiry), todo.id",
        )?;
        let claims = statement
            .query_map((process_id, now_ms), |row| {
                Ok(ClaimedTodo {
                    id: row.get(0)?,
                    project_id: row.get(1)?,
                    title: row.get(2)?,
                    claimed_at: row.get(3)?,
                    lock_expiry: row.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(claims)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::{
        Actor, NewTodo, Process, ProcessKind, ProcessSource, ProcessStatus, Project, TodoPriority,
        TodoService,
    };

    use super::*;

    const PROJECT_ID: ProjectId = 1;
    const PROCESS_ID: ProcessId = 7;
    const ACTOR_ID: &str = "mcp-process-7";

    fn fixture() -> Store {
        let store = Store::open_in_memory().expect("open store");
        store
            .put_project(&Project {
                id: PROJECT_ID,
                path: "/tmp/workman-todo-claim".into(),
                name: "claim fixture".into(),
                display_name: None,
                icon: None,
                selected: true,
                sort_order: 0,
            })
            .expect("put project");
        store
            .put_process(&Process {
                id: PROCESS_ID,
                project_id: PROJECT_ID,
                kind: ProcessKind::Agent,
                name: "codex-w7".into(),
                command: Some("codex".into()),
                working_dir: "/tmp/workman-todo-claim".into(),
                env: BTreeMap::new(),
                auto_start: false,
                auto_restart: false,
                restart_when_changed: Vec::new(),
                source: ProcessSource::Local,
                trust_hash: None,
                status: ProcessStatus::Running,
                pid: None,
                exit_code: None,
                exit_signal: None,
                exited_at: None,
                agent_tool_id: None,
                spawned_by_process_id: None,
                sort_order: 0,
            })
            .expect("put process");
        store
            .put_actor(&Actor {
                id: ACTOR_ID.into(),
                session_id: "session-process-7".into(),
                process_id: Some(PROCESS_ID),
                selected_project_id: Some(PROJECT_ID),
                created_at: 5_000,
                last_seen_at: 5_000,
            })
            .expect("put actor");
        store
    }

    #[test]
    fn claim_join_tracks_acquisition_and_disappears_on_release() {
        let store = fixture();
        let service = TodoService::new(&store);
        let todo = service
            .create(
                PROJECT_ID,
                NewTodo {
                    title: "Build claimed-todo overlay".into(),
                    body: String::new(),
                    priority: TodoPriority::High,
                    tags: Vec::new(),
                },
                10_000,
            )
            .expect("create todo");

        service
            .lock(PROJECT_ID, todo.id, ACTOR_ID, 30_000, 10_000)
            .expect("claim todo");
        assert_eq!(
            store
                .claimed_todos_for_process(PROCESS_ID, 10_001)
                .expect("list claims"),
            vec![ClaimedTodo {
                id: todo.id,
                project_id: PROJECT_ID,
                title: "Build claimed-todo overlay".into(),
                claimed_at: Some(10_000),
                lock_expiry: 40_000,
            }]
        );

        service
            .lock(PROJECT_ID, todo.id, ACTOR_ID, 40_000, 20_000)
            .expect("renew claim");
        let renewed = store
            .claimed_todos_for_process(PROCESS_ID, 20_001)
            .expect("list renewed claim");
        assert_eq!(renewed[0].claimed_at, Some(10_000));
        assert_eq!(renewed[0].lock_expiry, 60_000);

        service
            .unlock(PROJECT_ID, todo.id, ACTOR_ID, 20_002)
            .expect("release claim");
        assert!(
            store
                .claimed_todos_for_process(PROCESS_ID, 20_003)
                .expect("list released claims")
                .is_empty()
        );

        service
            .lock(PROJECT_ID, todo.id, ACTOR_ID, 30_000, 30_000)
            .expect("reclaim todo");
        service
            .complete(PROJECT_ID, todo.id, ACTOR_ID, true, true, 30_001)
            .expect("complete and release todo");
        assert!(
            store
                .claimed_todos_for_process(PROCESS_ID, 30_002)
                .expect("list claims after completion")
                .is_empty()
        );
    }

    #[test]
    fn claim_join_excludes_expired_leases() {
        let store = fixture();
        let service = TodoService::new(&store);
        let todo = service
            .create(
                PROJECT_ID,
                NewTodo {
                    title: "Short lease".into(),
                    body: String::new(),
                    priority: TodoPriority::Low,
                    tags: Vec::new(),
                },
                1_000,
            )
            .expect("create todo");
        service
            .lock(PROJECT_ID, todo.id, ACTOR_ID, 100, 1_000)
            .expect("claim todo");

        assert!(
            store
                .claimed_todos_for_process(PROCESS_ID, 1_100)
                .expect("list expired claims")
                .is_empty()
        );
    }
}
