//! Deterministic end-to-end notification pipeline tests.
//!
//! The harness replays rendered PTY fixtures through the real terminal emulator and attention
//! adapters, advances a virtual millisecond clock, persists the authoritative attention edge,
//! and consumes new persisted rows through the same boundary used by the desktop OS layer.

use std::collections::{BTreeMap, BTreeSet};

use workman_core::{
    NewTodo, Notification, NotificationId, NotificationType, Process, ProcessKind, ProcessSource,
    ProcessStatus, Project, Store, Timer, TimerKind, TodoPriority, TodoService, UpdateTodo,
    attention::{
        AgentState, AgentWaitingProcess, AgentWaitingReason, AttentionConfig, AttentionState,
        AttentionTracker,
    },
    terminal::TerminalEmulator,
};

use crate::timers::{WatchProgress, advance_watch_progress};

const PROJECT_ID: i64 = 1;
const PROCESS_ID: i64 = 7;
const CLAUDE_WORKING: &str =
    include_str!("../../workman-core/tests/fixtures/attention/claude_working.txt");
const CLAUDE_RESTING: &str =
    include_str!("../../workman-core/tests/fixtures/attention/claude_resting_with_draft.txt");
const CLAUDE_NEEDS_INPUT: &str =
    include_str!("../../workman-core/tests/fixtures/attention/claude_permission_dialog.txt");
const CODEX_WORKING: &str =
    include_str!("../../workman-core/tests/fixtures/attention/codex_working.txt");
const CODEX_RESTING: &str =
    include_str!("../../workman-core/tests/fixtures/attention/codex_resting.txt");
const PLAIN_WORKING: &str =
    include_str!("../../workman-core/tests/fixtures/attention/plain_terminal_working.txt");
const PLAIN_RESTING: &str =
    include_str!("../../workman-core/tests/fixtures/attention/plain_terminal_prompt.txt");

struct ScriptedPipeline {
    terminal: TerminalEmulator,
    tracker: AttentionTracker,
    store: Store,
    watched: bool,
    turn_started: bool,
    native_baseline_ready: bool,
    seen_native_rows: BTreeSet<NotificationId>,
    native_emissions: Vec<NotificationId>,
    window_focused: bool,
}

impl ScriptedPipeline {
    fn new(tool_type: Option<&str>) -> Self {
        let store = Store::open_in_memory().expect("open notification harness store");
        store
            .put_project(&Project {
                id: PROJECT_ID,
                path: "/tmp/workman-todo433-fixture".into(),
                name: "notification fixtures".into(),
                display_name: None,
                icon: None,
                selected: true,
                sort_order: 0,
            })
            .expect("put fixture project");
        store
            .put_process(&Process {
                id: PROCESS_ID,
                project_id: PROJECT_ID,
                kind: ProcessKind::Agent,
                name: "scripted-agent".into(),
                command: Some("recorded fixture".into()),
                working_dir: "/tmp/workman-todo433-fixture".into(),
                env: BTreeMap::new(),
                auto_start: false,
                auto_restart: false,
                restart_when_changed: Vec::new(),
                source: ProcessSource::Local,
                trust_hash: None,
                status: ProcessStatus::Running,
                pid: Some(433),
                exit_code: None,
                exit_signal: None,
                exited_at: None,
                agent_tool_id: None,
                spawned_by_process_id: None,
                sort_order: 0,
            })
            .expect("put fixture process");

        Self {
            terminal: TerminalEmulator::new(24, 100, 100),
            tracker: AttentionTracker::new_at(
                tool_type.map(str::to_owned),
                AttentionConfig::default(),
                0,
            ),
            store,
            watched: false,
            turn_started: false,
            native_baseline_ready: false,
            seen_native_rows: BTreeSet::new(),
            native_emissions: Vec::new(),
            window_focused: false,
        }
    }

    fn submit_at(&mut self, now_ms: i64) {
        self.turn_started = true;
        self.tracker.observe_input_at(now_ms);
    }

    fn frame_at(&mut self, now_ms: i64, rendered: &str) {
        let mut bytes = b"\x1b[2J\x1b[H".to_vec();
        bytes.extend_from_slice(rendered.as_bytes());
        self.terminal.feed(&bytes);
        let viewport = self
            .terminal
            .read_rows(self.terminal.history_rows()..usize::MAX);
        self.tracker
            .observe_output_at(&bytes, &viewport.text(), viewport.alternate_screen, now_ms);
    }

    fn suppress_ui_activity_at(&self, now_ms: i64) {
        self.tracker.suppress_ui_activity_at(now_ms);
    }

    fn consume_idle_watch_at(&self, timer_id: i64, fired_at: i64) {
        self.store
            .put_timer(&Timer {
                id: timer_id,
                owner_actor: "notification-harness".into(),
                delivery_process_id: PROCESS_ID,
                body: "consumed watch delivery".into(),
                kind: TimerKind::IdleAny,
                watch_process_ids: vec![PROCESS_ID],
                interval_ms: None,
                repeating: false,
                max_wait_deadline: Some(60_000),
                paused: false,
                fired: true,
                fired_at: Some(fired_at),
                created_at: 1_200,
            })
            .expect("put consumed watch timer");
        self.store
            .record_consumed_idle_watch(PROCESS_ID, timer_id, fired_at)
            .expect("record consumed idle watch");
    }

    fn observe_at(&mut self, now_ms: i64) -> AgentState {
        let mut state = self.tracker.snapshot_at(now_ms);
        let notification = self
            .store
            .observe_agent_attention_with_activity(
                PROCESS_ID,
                state.state,
                self.watched,
                self.turn_started,
                state.last_output_at.max(state.last_input_at),
                now_ms,
            )
            .expect("persist attention edge");
        state.refine_notifications(self.watched, notification.unread);
        self.capture_native_emission_points();
        state
    }

    fn capture_native_emission_points(&mut self) {
        let rows = self.notifications();
        if !self.native_baseline_ready {
            self.seen_native_rows.extend(rows.iter().map(|row| row.id));
            self.native_baseline_ready = true;
            return;
        }

        let mut fresh = rows
            .iter()
            .filter(|row| row.read_at.is_none() && !self.seen_native_rows.contains(&row.id))
            .collect::<Vec<_>>();
        fresh.sort_by_key(|row| (row.created_at, row.id));
        self.seen_native_rows.extend(rows.iter().map(|row| row.id));
        if !self.window_focused {
            self.native_emissions
                .extend(fresh.into_iter().map(|row| row.id));
        }
    }

    fn notifications(&self) -> Vec<Notification> {
        self.store
            .list_notifications(None, 200)
            .expect("list notifications")
    }

    fn unread_notifications(&self) -> Vec<Notification> {
        self.store
            .list_notifications(Some(false), 200)
            .expect("list unread notifications")
    }
}

fn start_recorded_turn(pipeline: &mut ScriptedPipeline, working: &str) {
    pipeline.submit_at(1_000);
    pipeline.frame_at(1_100, working);
    assert_eq!(pipeline.observe_at(1_100).state, AttentionState::Working);
}

fn finish_recorded_turn(pipeline: &mut ScriptedPipeline, resting: &str) -> AgentState {
    pipeline.frame_at(2_000, resting);
    assert_eq!(
        pipeline.observe_at(6_999).state,
        AttentionState::Working,
        "the debounce boundary must remain exclusive"
    );
    pipeline.observe_at(7_000)
}

fn create_handoff_todo(pipeline: &ScriptedPipeline) -> i64 {
    TodoService::new(&pipeline.store)
        .create(
            PROJECT_ID,
            NewTodo {
                title: "Review the discovered edge case".into(),
                body: "Agent-authored context for the human.".into(),
                priority: TodoPriority::High,
                tags: vec!["handoff".into()],
            },
            1_000,
        )
        .expect("create handoff todo")
        .id
}

#[test]
fn assigned_to_user_emits_one_notification_even_when_agent_is_watched() {
    let mut pipeline = ScriptedPipeline::new(Some("claude_code"));
    pipeline.capture_native_emission_points();
    pipeline.watched = true;
    let todo_id = create_handoff_todo(&pipeline);

    let assigned = TodoService::new(&pipeline.store)
        .assign(
            PROJECT_ID,
            todo_id,
            Some("user".into()),
            "scripted-agent",
            2_000,
        )
        .expect("assign todo to user");
    pipeline.capture_native_emission_points();

    assert_eq!(assigned.assignee.as_deref(), Some("user"));
    let notifications = pipeline.notifications();
    assert_eq!(notifications.len(), 1);
    assert_eq!(notifications[0].kind, NotificationType::TodoAssignedToYou);
    assert_eq!(notifications[0].todo_id, Some(todo_id));
    assert_eq!(notifications[0].comment_id, None);
    assert_eq!(pipeline.native_emissions.len(), 1);
}

#[test]
fn editing_an_assigned_todo_without_reassignment_emits_no_new_notification() {
    let pipeline = ScriptedPipeline::new(Some("claude_code"));
    let todo_id = create_handoff_todo(&pipeline);
    let service = TodoService::new(&pipeline.store);
    service
        .assign(
            PROJECT_ID,
            todo_id,
            Some("user".into()),
            "scripted-agent",
            2_000,
        )
        .expect("assign todo to user");

    service
        .update(
            PROJECT_ID,
            todo_id,
            UpdateTodo {
                body: Some("More evidence, same assignment.".into()),
                ..UpdateTodo::default()
            },
            3_000,
        )
        .expect("edit assigned todo");
    service
        .assign(
            PROJECT_ID,
            todo_id,
            Some("user".into()),
            "scripted-agent",
            3_100,
        )
        .expect("repeat same assignment");

    assert_eq!(
        pipeline.notifications().len(),
        1,
        "no fresh assignment edge"
    );
}

#[test]
fn user_mention_emits_one_notification_with_a_comment_navigation_anchor() {
    let mut pipeline = ScriptedPipeline::new(Some("claude_code"));
    pipeline.capture_native_emission_points();
    pipeline.watched = true;
    let todo_id = create_handoff_todo(&pipeline);

    let comment = TodoService::new(&pipeline.store)
        .comment_create_as(
            PROJECT_ID,
            todo_id,
            "agent-process-7",
            "scripted-agent",
            "@user, can you choose between these approaches?".into(),
            2_000,
        )
        .expect("create user mention comment");
    pipeline.capture_native_emission_points();

    let notifications = pipeline.notifications();
    assert_eq!(notifications.len(), 1);
    assert_eq!(notifications[0].kind, NotificationType::MentionedInComment);
    assert_eq!(notifications[0].todo_id, Some(todo_id));
    assert_eq!(notifications[0].comment_id, Some(comment.id));
    assert_eq!(pipeline.native_emissions.len(), 1);
}

#[test]
fn bursty_claude_output_masks_transient_idle_gaps_everywhere() {
    let mut pipeline = ScriptedPipeline::new(Some("claude_code"));
    start_recorded_turn(&mut pipeline, CLAUDE_WORKING);

    pipeline.frame_at(2_000, CLAUDE_RESTING);
    assert_eq!(pipeline.observe_at(6_999).state, AttentionState::Working);
    pipeline.frame_at(7_000, CLAUDE_WORKING);
    assert_eq!(pipeline.observe_at(7_000).state, AttentionState::Working);

    pipeline.frame_at(8_000, CLAUDE_RESTING);
    assert_eq!(pipeline.observe_at(12_999).state, AttentionState::Working);
    pipeline.frame_at(13_000, CLAUDE_WORKING);
    assert_eq!(pipeline.observe_at(13_000).state, AttentionState::Working);

    assert!(pipeline.notifications().is_empty());
    assert!(pipeline.native_emissions.is_empty());
}

#[test]
fn recorded_tool_completions_emit_exactly_one_persisted_event_and_os_candidate() {
    for (name, tool_type, working, resting) in [
        (
            "claude",
            Some("claude_code"),
            CLAUDE_WORKING,
            CLAUDE_RESTING,
        ),
        ("codex", Some("codex"), CODEX_WORKING, CODEX_RESTING),
        ("plain terminal", None, PLAIN_WORKING, PLAIN_RESTING),
    ] {
        let mut pipeline = ScriptedPipeline::new(tool_type);
        start_recorded_turn(&mut pipeline, working);
        let done = finish_recorded_turn(&mut pipeline, resting);

        assert_eq!(done.state, AttentionState::Idle, "{name}");
        assert!(done.unread, "{name}");
        assert_eq!(pipeline.unread_notifications().len(), 1, "{name}");
        assert_eq!(
            pipeline.notifications()[0].kind,
            NotificationType::AgentDone
        );
        assert_eq!(pipeline.native_emissions.len(), 1, "{name}");

        assert_eq!(pipeline.observe_at(20_000).state, AttentionState::Idle);
        assert_eq!(pipeline.notifications().len(), 1, "{name}");
        assert_eq!(pipeline.native_emissions.len(), 1, "{name}");
    }
}

#[test]
fn garrett_click_in_click_out_selection_resize_replay_and_focus_reports_are_attention_neutral() {
    let mut pipeline = ScriptedPipeline::new(Some("claude_code"));
    pipeline.frame_at(1_000, CLAUDE_RESTING);
    let baseline = pipeline.observe_at(6_000);
    assert_eq!(baseline.state, AttentionState::Idle);
    assert_eq!(baseline.last_output_at, Some(1_000));

    // Click in / focus report, click out, retained replay, and SIGWINCH redraw all pass through
    // ProcessRegistry's UI-activity suppression seam before their immediate PTY redraw.
    for (suppressed_at, redraw_at) in [(6_100, 6_200), (6_800, 6_900), (7_500, 7_600)] {
        pipeline.suppress_ui_activity_at(suppressed_at);
        pipeline.frame_at(redraw_at, CLAUDE_RESTING);
        let parked = pipeline.observe_at(redraw_at);
        assert_eq!(parked.state, AttentionState::Idle);
        assert_eq!(parked.last_output_at, Some(1_000));
        assert_eq!(parked.last_content_change_at, Some(1_000));
    }

    // Merely selecting or viewing the already-read process does not touch the tracker.
    assert!(!pipeline.store.mark_agent_read(PROCESS_ID).unwrap());
    assert_eq!(pipeline.observe_at(9_000).state, AttentionState::Idle);
    assert!(pipeline.notifications().is_empty());
    assert!(pipeline.native_emissions.is_empty());
}

#[test]
fn idle_repaint_no_alert() {
    let mut pipeline = ScriptedPipeline::new(Some("claude_code"));
    pipeline.submit_at(1_000);
    pipeline.frame_at(1_100, CLAUDE_RESTING);

    let baseline = pipeline.observe_at(6_100);
    assert_eq!(baseline.state, AttentionState::Idle);
    assert!(pipeline.notifications().is_empty());

    // Replay the same captured resting screen long after the original turn. A
    // footer/hint repaint is still idle even though it contains real PTY bytes.
    pipeline.frame_at(400_000, CLAUDE_RESTING);
    assert_eq!(pipeline.observe_at(400_000).state, AttentionState::Idle);
    assert_eq!(pipeline.observe_at(405_000).state, AttentionState::Idle);
    assert!(pipeline.notifications().is_empty());
    assert!(pipeline.native_emissions.is_empty());
}

#[test]
fn working_then_idle_alerts_once() {
    let mut pipeline = ScriptedPipeline::new(Some("claude_code"));
    start_recorded_turn(&mut pipeline, CLAUDE_WORKING);

    let done = finish_recorded_turn(&mut pipeline, CLAUDE_RESTING);
    assert_eq!(done.state, AttentionState::Idle);
    assert!(done.unread);
    assert_eq!(pipeline.notifications().len(), 1);
    assert_eq!(pipeline.native_emissions.len(), 1);

    for now_ms in [7_100, 20_000, 300_000] {
        assert_eq!(pipeline.observe_at(now_ms).state, AttentionState::Idle);
    }
    assert_eq!(pipeline.notifications().len(), 1);
    assert_eq!(pipeline.native_emissions.len(), 1);
}

#[test]
fn idle_repaint_after_alert_stays_quiet() {
    let mut pipeline = ScriptedPipeline::new(Some("claude_code"));
    start_recorded_turn(&mut pipeline, CLAUDE_WORKING);
    let done = finish_recorded_turn(&mut pipeline, CLAUDE_RESTING);
    assert!(done.unread);
    assert_eq!(pipeline.notifications().len(), 1);

    // Cross the durable cooldown so a repaint-induced fake working episode
    // would otherwise create a second persisted row.
    pipeline.frame_at(307_001, CLAUDE_RESTING);
    assert_eq!(pipeline.observe_at(307_001).state, AttentionState::Idle);
    assert_eq!(pipeline.observe_at(312_001).state, AttentionState::Idle);
    assert_eq!(pipeline.notifications().len(), 1);
    assert_eq!(pipeline.unread_notifications().len(), 1);
    assert_eq!(pipeline.native_emissions.len(), 1);
}

#[test]
fn watched_completion_is_suppressed_for_row_dot_badge_and_os_delivery() {
    let mut pipeline = ScriptedPipeline::new(Some("claude_code"));
    start_recorded_turn(&mut pipeline, CLAUDE_WORKING);
    pipeline
        .store
        .put_timer(&Timer {
            id: 433,
            owner_actor: "notification-harness".into(),
            delivery_process_id: PROCESS_ID,
            body: "react to completion".into(),
            kind: TimerKind::IdleAny,
            watch_process_ids: vec![PROCESS_ID],
            interval_ms: None,
            repeating: false,
            max_wait_deadline: Some(60_000),
            paused: false,
            fired: false,
            fired_at: None,
            created_at: 1_200,
        })
        .expect("arm pending watch timer");
    assert!(
        pipeline
            .store
            .get_timer(433)
            .unwrap()
            .is_some_and(|timer| !timer.fired)
    );
    // This is the exact boolean ProcessRegistry derives from the pending row before calling the
    // durable notification edge observer.
    pipeline.watched = true;
    let done = finish_recorded_turn(&mut pipeline, CLAUDE_RESTING);

    assert_eq!(done.state, AttentionState::Idle);
    assert!(done.watched);
    assert!(!done.unread, "no process-tree unread dot");
    assert!(
        pipeline.unread_notifications().is_empty(),
        "no center row/badge"
    );
    assert!(pipeline.notifications().is_empty());
    assert!(pipeline.native_emissions.is_empty(), "no OS emission point");
}

#[test]
fn watcher_fires_and_is_consumed_before_debounced_done_check_emits_zero_notifications() {
    let mut pipeline = ScriptedPipeline::new(Some("claude_code"));
    start_recorded_turn(&mut pipeline, CLAUDE_WORKING);

    pipeline.frame_at(2_000, CLAUDE_RESTING);
    pipeline.consume_idle_watch_at(451, 7_000);
    pipeline.watched = false;

    assert_eq!(pipeline.observe_at(6_999).state, AttentionState::Working);
    let done = pipeline.observe_at(7_000);
    assert_eq!(done.state, AttentionState::Idle);
    assert!(!done.watched, "the consumed timer is no longer pending");
    assert!(!done.unread, "no process-tree unread dot");
    assert!(pipeline.notifications().is_empty(), "no center row/badge");
    assert!(pipeline.native_emissions.is_empty(), "no OS emission point");
}

#[test]
fn consumed_watch_then_activity_then_later_unwatched_completion_emits_one_notification() {
    let mut pipeline = ScriptedPipeline::new(Some("claude_code"));
    start_recorded_turn(&mut pipeline, CLAUDE_WORKING);

    pipeline.frame_at(2_000, CLAUDE_RESTING);
    pipeline.consume_idle_watch_at(451, 7_000);
    assert_eq!(pipeline.observe_at(7_000).state, AttentionState::Idle);
    assert!(pipeline.notifications().is_empty());

    pipeline.frame_at(8_000, CLAUDE_WORKING);
    assert_eq!(pipeline.observe_at(8_000).state, AttentionState::Working);
    pipeline.frame_at(9_000, CLAUDE_RESTING);
    let later_done = pipeline.observe_at(14_000);

    assert_eq!(later_done.state, AttentionState::Idle);
    assert!(later_done.unread);
    assert_eq!(pipeline.notifications().len(), 1);
    assert_eq!(pipeline.native_emissions.len(), 1);
}

#[test]
fn focused_window_keeps_the_center_event_but_suppresses_the_os_delivery_point() {
    let mut pipeline = ScriptedPipeline::new(Some("claude_code"));
    pipeline.window_focused = true;
    start_recorded_turn(&mut pipeline, CLAUDE_WORKING);
    let done = finish_recorded_turn(&mut pipeline, CLAUDE_RESTING);

    assert!(done.unread);
    assert_eq!(pipeline.unread_notifications().len(), 1);
    assert!(
        pipeline.native_emissions.is_empty(),
        "focused users consume the persisted row through in-app UI only"
    );
}

#[test]
fn unwatched_completion_row_dot_badge_and_read_cascades_stay_in_sync() {
    let mut pipeline = ScriptedPipeline::new(Some("claude_code"));
    start_recorded_turn(&mut pipeline, CLAUDE_WORKING);
    let done = finish_recorded_turn(&mut pipeline, CLAUDE_RESTING);
    let first = pipeline.unread_notifications().remove(0);

    assert!(done.unread, "process-tree unread dot");
    assert_eq!(
        pipeline.unread_notifications().len(),
        1,
        "center + Dock badge"
    );
    assert!(
        pipeline
            .store
            .mark_notification_read(first.id, 7_100)
            .unwrap()
    );
    let cleared_from_center = pipeline.observe_at(7_200);
    assert!(
        !cleared_from_center.unread,
        "center read clears process dot"
    );
    assert!(
        pipeline.unread_notifications().is_empty(),
        "center read clears badge"
    );

    pipeline.submit_at(8_000);
    pipeline.frame_at(8_100, CLAUDE_WORKING);
    assert_eq!(pipeline.observe_at(8_100).state, AttentionState::Working);
    pipeline.frame_at(9_000, CLAUDE_RESTING);
    let second_done = pipeline.observe_at(14_000);
    assert!(
        second_done.unread,
        "an intervening explicit view permits the next edge"
    );
    assert_eq!(pipeline.unread_notifications().len(), 1);

    assert!(pipeline.store.mark_agent_read(PROCESS_ID).unwrap());
    let cleared_from_process = pipeline.observe_at(14_100);
    assert!(!cleared_from_process.unread, "process read clears dot");
    assert!(
        pipeline.unread_notifications().is_empty(),
        "process read clears center/badge"
    );
    assert!(
        pipeline
            .notifications()
            .iter()
            .all(|row| row.read_at.is_some())
    );
}

#[test]
fn needs_input_is_edge_triggered_and_requires_work_before_renotifying() {
    let mut pipeline = ScriptedPipeline::new(Some("claude_code"));
    start_recorded_turn(&mut pipeline, CLAUDE_WORKING);

    // The production input grace expires at 3_000ms; the permission edge is observed just after
    // that boundary so the test covers needs-input debouncing rather than input submission.
    pipeline.frame_at(3_100, CLAUDE_NEEDS_INPUT);
    let first = pipeline.observe_at(3_100);
    assert_eq!(first.state, AttentionState::NeedsInput);
    assert!(first.unread);
    assert_eq!(pipeline.notifications().len(), 1);
    assert_eq!(
        pipeline.notifications()[0].kind,
        NotificationType::NeedsInput
    );

    for now_ms in [3_200, 3_500, 10_000] {
        assert_eq!(
            pipeline.observe_at(now_ms).state,
            AttentionState::NeedsInput
        );
    }
    assert_eq!(
        pipeline.notifications().len(),
        1,
        "stable/flapping polls must not spam"
    );
    assert_eq!(pipeline.native_emissions.len(), 1);

    // A coarse observer can see NeedsInput -> Idle -> NeedsInput while prompt frames flap. With no
    // authoritative Working observation between those states, the second prompt is not an edge.
    pipeline.frame_at(10_100, CLAUDE_RESTING);
    assert_eq!(pipeline.observe_at(15_100).state, AttentionState::Idle);
    pipeline.frame_at(15_200, CLAUDE_NEEDS_INPUT);
    assert_eq!(
        pipeline.observe_at(15_200).state,
        AttentionState::NeedsInput
    );
    assert_eq!(
        pipeline.notifications().len(),
        1,
        "idle flapping does not re-arm"
    );
    assert_eq!(pipeline.native_emissions.len(), 1);

    pipeline.frame_at(16_000, CLAUDE_WORKING);
    assert_eq!(pipeline.observe_at(16_000).state, AttentionState::Working);
    pipeline.frame_at(16_100, CLAUDE_NEEDS_INPUT);
    assert_eq!(
        pipeline.observe_at(16_100).state,
        AttentionState::NeedsInput
    );
    assert_eq!(
        pipeline.notifications().len(),
        2,
        "fresh work re-arms the edge"
    );
    assert_eq!(pipeline.unread_notifications().len(), 1);
    assert_eq!(pipeline.native_emissions.len(), 2);
}

#[test]
fn parent_delivery_timer_waiting_does_not_mark_the_watched_child_waiting() {
    let mut parent = ScriptedPipeline::new(None);
    parent.frame_at(1_000, PLAIN_RESTING);
    let mut parent_state = parent.observe_at(6_000);
    assert_eq!(parent_state.state, AttentionState::Idle);

    let mut child = ScriptedPipeline::new(Some("claude_code"));
    start_recorded_turn(&mut child, CLAUDE_WORKING);
    let mut child_state = child.observe_at(1_200);
    assert_eq!(child_state.state, AttentionState::Working);

    let reason = AgentWaitingReason {
        timer_id: 426,
        kind: TimerKind::IdleAny,
        due_at: 16_000,
        max_wait_ms: 10_000,
        remaining_ms: 10_000,
        paused: false,
        watch_processes: vec![AgentWaitingProcess {
            process_id: child.store.get_process(PROCESS_ID).unwrap().unwrap().id,
            process_name: "child".into(),
        }],
    };
    parent_state.refine_waiting(vec![reason]);
    parent_state.refine_notifications(true, false);
    child_state.refine_notifications(true, false);

    assert_eq!(parent_state.state, AttentionState::Waiting);
    assert!(parent_state.waiting);
    assert!(parent_state.idle, "waiting retains idle compatibility");
    assert_eq!(parent_state.waiting_on.len(), 1);
    assert!(child_state.watched);
    assert_eq!(child_state.state, AttentionState::Working);
    assert!(!child_state.waiting);
    assert!(child_state.waiting_on.is_empty());
}

#[test]
fn timer_fire_when_idle_ignores_debounce_masked_gaps_and_fires_on_genuine_idle() {
    let mut pipeline = ScriptedPipeline::new(Some("claude_code"));
    start_recorded_turn(&mut pipeline, CLAUDE_WORKING);
    let mut watch = WatchProgress::new(false, false);

    pipeline.frame_at(2_000, CLAUDE_RESTING);
    let transient = pipeline.observe_at(6_999);
    advance_watch_progress(&mut watch, transient.state == AttentionState::Idle);
    assert!(!watch.satisfied());
    assert!(pipeline.notifications().is_empty());

    pipeline.frame_at(7_000, CLAUDE_WORKING);
    let resumed = pipeline.observe_at(7_000);
    advance_watch_progress(&mut watch, resumed.state == AttentionState::Idle);
    assert!(!watch.satisfied());

    pipeline.frame_at(8_000, CLAUDE_RESTING);
    let masked = pipeline.observe_at(12_999);
    advance_watch_progress(&mut watch, masked.state == AttentionState::Idle);
    assert!(
        !watch.satisfied(),
        "timer must not fire inside the debounce window"
    );

    let genuine = pipeline.observe_at(13_000);
    advance_watch_progress(&mut watch, genuine.state == AttentionState::Idle);
    assert!(
        watch.satisfied(),
        "timer fires on the authoritative idle edge"
    );
    assert_eq!(pipeline.notifications().len(), 1);
    assert_eq!(pipeline.native_emissions.len(), 1);
}
