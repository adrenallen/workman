//! One durable readiness edge after every agent in a project has settled.

use std::collections::HashMap;

use workman_core::{
    ProcessKind, ProcessStatus, ProjectId, Store, StoreResult, attention::AttentionState,
};

use crate::process_registry::ProcessStatusView;

const READY_SETTLE_MS: i64 = 2_000;

#[derive(Default)]
struct ProjectActivity {
    worked: bool,
    ready_since: Option<i64>,
}

#[derive(Default)]
pub(crate) struct ProjectNotifications {
    projects: HashMap<ProjectId, ProjectActivity>,
}

impl ProjectNotifications {
    /// `statuses` must contain every process in `scope`, including child agents.
    pub(crate) fn observe(
        &mut self,
        store: &Store,
        scope: Option<ProjectId>,
        statuses: &[ProcessStatusView],
        now: i64,
    ) -> StoreResult<()> {
        let mut busy_projects = HashMap::<ProjectId, bool>::new();
        for status in statuses
            .iter()
            .filter(|status| status.process.kind == ProcessKind::Agent)
        {
            let busy = match status.process.status {
                ProcessStatus::Starting => true,
                ProcessStatus::Running => status.agent_state.state == AttentionState::Working,
                ProcessStatus::Stopped | ProcessStatus::Exited | ProcessStatus::Crashed => false,
            };
            *busy_projects.entry(status.process.project_id).or_default() |= busy;
        }
        // Removing a project's last agent must not manufacture a ready edge.
        for project_id in self.projects.keys().copied().collect::<Vec<_>>() {
            if scope.is_none_or(|scope| scope == project_id)
                && !busy_projects.contains_key(&project_id)
            {
                if scope.is_some() || store.is_project_in_active_profile(project_id)? {
                    store.clear_project_ready_notifications(project_id, now)?;
                }
                self.projects.remove(&project_id);
            }
        }
        for (project_id, busy) in busy_projects {
            let activity = self.projects.entry(project_id).or_default();
            if busy {
                // Clear once per work cycle, including the first observation after a restart.
                if !activity.worked {
                    store.clear_project_ready_notifications(project_id, now)?;
                }
                activity.worked = true;
                activity.ready_since = None;
            } else if activity.worked {
                let ready_since = activity.ready_since.get_or_insert(now);
                if now.saturating_sub(*ready_since) >= READY_SETTLE_MS {
                    store.create_project_ready_notification(project_id, now)?;
                    activity.worked = false;
                    activity.ready_since = None;
                }
            }
        }
        Ok(())
    }

    pub(crate) fn next_deadline(&self) -> Option<i64> {
        self.projects
            .values()
            .filter_map(|activity| activity.ready_since)
            .map(|since| since.saturating_add(READY_SETTLE_MS))
            .min()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use workman_core::{NotificationType, Process, ProcessSource, Project, attention::AgentState};

    fn store() -> Store {
        let store = Store::open_in_memory().unwrap();
        for id in [1, 2] {
            store
                .put_project(&Project {
                    id,
                    path: format!("/tmp/project-ready-{id}"),
                    name: format!("Project {id}"),
                    display_name: None,
                    icon: None,
                    selected: id == 1,
                    sort_order: id,
                })
                .unwrap();
        }
        store
    }

    fn agent(id: i64, project_id: i64, state: AttentionState) -> ProcessStatusView {
        let mut agent_state = AgentState::exited(None, None);
        agent_state.state = state;
        agent_state.working = state == AttentionState::Working;
        agent_state.exited = state == AttentionState::Exited;
        ProcessStatusView {
            process: Process {
                id,
                project_id,
                kind: ProcessKind::Agent,
                name: format!("Agent {id}"),
                command: None,
                working_dir: String::new(),
                env: Default::default(),
                auto_start: false,
                auto_restart: false,
                restart_when_changed: vec![],
                source: ProcessSource::Local,
                trust_hash: None,
                status: ProcessStatus::Running,
                pid: None,
                exit_code: None,
                exit_signal: None,
                exited_at: None,
                agent_tool_id: None,
                spawned_by_process_id: (id == 2).then_some(1),
                sort_order: id,
            },
            agent_state,
            events: vec![],
            agent_session_id: None,
            agent_launch_mode: None,
            claimed_todos: vec![],
        }
    }

    #[test]
    fn waits_for_children_and_handoffs_then_emits_once_per_work_cycle() {
        let store = store();
        let mut tracker = ProjectNotifications::default();
        let waiting = agent(1, 1, AttentionState::Waiting);
        let child_working = agent(2, 1, AttentionState::Working);
        let child_idle = agent(2, 1, AttentionState::Idle);
        tracker
            .observe(&store, None, &[waiting.clone(), child_working.clone()], 0)
            .unwrap();
        assert_eq!(tracker.next_deadline(), None);
        tracker
            .observe(&store, None, &[waiting.clone(), child_idle.clone()], 100)
            .unwrap();
        assert_eq!(tracker.next_deadline(), Some(2_100));
        // A timer handoff restarts the quiet period, even with the same child already idle.
        tracker
            .observe(
                &store,
                None,
                &[agent(1, 1, AttentionState::Working), child_idle.clone()],
                1_500,
            )
            .unwrap();
        tracker
            .observe(&store, None, &[waiting.clone(), child_idle.clone()], 1_600)
            .unwrap();
        tracker
            .observe(&store, None, &[waiting.clone(), child_idle.clone()], 3_599)
            .unwrap();
        assert!(store.list_notifications(None, 100).unwrap().is_empty());
        tracker
            .observe(&store, None, &[waiting.clone(), child_idle.clone()], 3_600)
            .unwrap();
        tracker
            .observe(&store, None, &[waiting.clone(), child_idle.clone()], 20_000)
            .unwrap();
        let rows = store.list_notifications(None, 100).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].kind, NotificationType::ProjectReady);
        assert_eq!(rows[0].project_id, Some(1));
        assert_eq!(rows[0].process_id, None);
        assert!(rows[0].body.contains("Project 1 is ready for you"));
        assert_eq!(tracker.next_deadline(), None);

        tracker
            .observe(&store, None, &[waiting.clone(), child_working], 20_001)
            .unwrap();
        assert!(
            store
                .list_notifications(Some(false), 100)
                .unwrap()
                .is_empty()
        );
        tracker
            .observe(&store, None, &[waiting.clone(), child_idle.clone()], 20_002)
            .unwrap();
        tracker
            .observe(&store, None, &[waiting, child_idle], 22_002)
            .unwrap();
        assert_eq!(store.list_notifications(None, 100).unwrap().len(), 2);
        assert_eq!(store.list_notifications(Some(false), 100).unwrap().len(), 1);
    }

    #[test]
    fn idle_baselines_restarts_and_removing_the_last_agent_do_not_ding() {
        let store = store();
        let idle = agent(1, 1, AttentionState::Idle);
        let mut tracker = ProjectNotifications::default();
        tracker.observe(&store, None, &[idle.clone()], 0).unwrap();
        tracker
            .observe(&store, None, &[idle.clone()], 9_000)
            .unwrap();
        assert!(store.list_notifications(None, 100).unwrap().is_empty());
        tracker
            .observe(
                &store,
                None,
                &[agent(1, 1, AttentionState::Working)],
                10_000,
            )
            .unwrap();
        tracker.observe(&store, None, &[], 11_000).unwrap();
        tracker
            .observe(&store, None, &[idle.clone()], 20_000)
            .unwrap();
        tracker
            .observe(&store, None, &[idle.clone()], 30_000)
            .unwrap();
        assert!(store.list_notifications(None, 100).unwrap().is_empty());
        let mut restarted = ProjectNotifications::default();
        restarted
            .observe(&store, None, &[idle.clone()], 40_000)
            .unwrap();
        restarted.observe(&store, None, &[idle], 50_000).unwrap();
        assert!(store.list_notifications(None, 100).unwrap().is_empty());
    }

    #[test]
    fn starting_blocks_readiness_but_input_prompts_and_stopped_agents_do_not() {
        let store = store();
        let mut tracker = ProjectNotifications::default();
        let mut starting = agent(1, 1, AttentionState::Idle);
        starting.process.status = ProcessStatus::Starting;
        let mut stopped = agent(2, 1, AttentionState::Working);
        stopped.process.status = ProcessStatus::Stopped;
        let mut command = agent(3, 1, AttentionState::Working);
        command.process.kind = ProcessKind::Command;
        tracker
            .observe(
                &store,
                None,
                &[starting.clone(), stopped.clone(), command.clone()],
                0,
            )
            .unwrap();
        tracker
            .observe(
                &store,
                None,
                &[starting, stopped.clone(), command.clone()],
                9_000,
            )
            .unwrap();
        assert!(store.list_notifications(None, 100).unwrap().is_empty());
        let ready = [agent(1, 1, AttentionState::NeedsInput), stopped, command];
        tracker.observe(&store, None, &ready, 10_000).unwrap();
        tracker.observe(&store, None, &ready, 12_000).unwrap();
        let row = store.list_notifications(None, 100).unwrap().remove(0);
        assert!(store.mark_notification_read(row.id, 13_000).unwrap());
        tracker.observe(&store, None, &ready, 14_000).unwrap();
        assert!(
            store
                .list_notifications(Some(false), 100)
                .unwrap()
                .is_empty()
        );
        assert_eq!(store.list_notifications(None, 100).unwrap().len(), 1);
    }

    #[test]
    fn projects_settle_independently_and_scoped_reads_do_not_reset_other_projects() {
        let store = store();
        let mut tracker = ProjectNotifications::default();
        let first = agent(1, 1, AttentionState::Working);
        let second = agent(2, 2, AttentionState::Working);
        tracker
            .observe(&store, None, &[first, second.clone()], 0)
            .unwrap();
        let idle = agent(1, 1, AttentionState::Idle);
        tracker
            .observe(&store, Some(1), &[idle.clone()], 10)
            .unwrap();
        tracker
            .observe(&store, Some(2), &[second.clone()], 1_000)
            .unwrap();
        assert_eq!(tracker.next_deadline(), Some(2_010));
        tracker
            .observe(&store, None, &[idle, second], 2_010)
            .unwrap();
        let rows = store.list_notifications(None, 100).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].project_id, Some(1));
    }

    #[tokio::test]
    async fn registry_arms_a_status_deadline_to_deliver_after_output_stops() {
        use crate::process_registry::ProcessRegistry;
        use crate::timers::now_millis;

        let mut registry = ProcessRegistry::new(store()).unwrap();
        let mut process = agent(1, 1, AttentionState::Idle).process;
        process.status = ProcessStatus::Starting;
        registry.store().put_process(&process).unwrap();
        registry.list_statuses(Some(1)).unwrap();
        process.status = ProcessStatus::Stopped;
        registry.store().put_process(&process).unwrap();
        registry.list_statuses(Some(1)).unwrap();
        let hub = registry.status_invalidations();
        let version = hub.version_at(now_millis());
        assert!(
            registry
                .store()
                .list_notifications(None, 100)
                .unwrap()
                .is_empty()
        );
        tokio::time::sleep(std::time::Duration::from_millis(
            READY_SETTLE_MS as u64 + 30,
        ))
        .await;
        assert!(hub.version_at(now_millis()) > version);
        registry.list_statuses(Some(1)).unwrap();
        let rows = registry.store().list_notifications(None, 100).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].kind, NotificationType::ProjectReady);
    }
}
