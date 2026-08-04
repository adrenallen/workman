//! Process attention signals and tool-aware state classification.

use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// Short output-silence window before a recognized resting prompt is idle.
pub const DEFAULT_QUIESCENCE: Duration = Duration::from_millis(500);

/// Conservative idle fallback when no adapter recognizes the terminal contents.
pub const DEFAULT_IDLE_AFTER: Duration = Duration::from_secs(5);

/// The orchestration-relevant state derived from output and terminal contents.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AttentionState {
    Working,
    NeedsInput,
    Idle,
    Exited,
}

/// Tool-specific facts found in the rendered terminal.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct AdapterFlags {
    pub busy: bool,
    pub needs_input: bool,
    pub resting_prompt: bool,
    pub thinking: bool,
    pub planning: bool,
    pub classification: Option<String>,
}

/// Input presented to a tool adapter after each emulator update.
#[derive(Clone, Copy, Debug)]
pub struct AdapterObservation<'a> {
    pub rendered: &'a str,
    pub alternate_screen: bool,
}

/// Classifies terminal contents for one family of interactive tools.
pub trait ToolAttentionAdapter: Send + Sync {
    fn inspect(&self, observation: AdapterObservation<'_>) -> AdapterFlags;
}

/// Public status payload shaped for orchestration and UI consumers.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct AgentState {
    /// Derived state. This is the authoritative orchestration value.
    pub state: AttentionState,
    /// Compatibility booleans mirroring Solo's agent-state payload.
    pub working: bool,
    pub needs_input: bool,
    pub idle: bool,
    pub exited: bool,
    pub thinking: bool,
    pub planning: bool,
    /// Tool family used to select the adapter.
    pub tool_type: Option<String>,
    /// Seconds since the last PTY output, with sub-second precision.
    pub idle_seconds: f64,
    /// Seconds since the last PTY output, with sub-second precision.
    pub last_output_seconds: Option<f64>,
    /// Unix timestamp in milliseconds for the newest PTY bytes.
    pub last_output_at: Option<i64>,
    /// Unix timestamp in milliseconds for the newest rendered-content change.
    pub last_content_change_at: Option<i64>,
    /// Adapter explanation such as `busy_spinner` or `permission_dialog`.
    pub classification: Option<String>,
}

impl AgentState {
    fn from_snapshot(
        state: AttentionState,
        tool_type: Option<String>,
        now_ms: i64,
        started_at: i64,
        last_output_at: Option<i64>,
        last_content_change_at: Option<i64>,
        flags: &AdapterFlags,
    ) -> Self {
        let idle_since = last_output_at.unwrap_or(started_at);
        let idle_seconds = elapsed_seconds(now_ms, idle_since);
        Self {
            state,
            working: state == AttentionState::Working,
            needs_input: state == AttentionState::NeedsInput,
            idle: state == AttentionState::Idle,
            exited: state == AttentionState::Exited,
            thinking: flags.thinking,
            planning: flags.planning,
            tool_type,
            idle_seconds,
            last_output_seconds: last_output_at.map(|at| elapsed_seconds(now_ms, at)),
            last_output_at,
            last_content_change_at,
            classification: flags.classification.clone(),
        }
    }

    /// Construct the best available status for a process without a live tracker.
    pub fn exited(tool_type: Option<String>, exited_at: Option<i64>) -> Self {
        let now = now_millis();
        let at = exited_at.unwrap_or(now);
        Self::from_snapshot(
            AttentionState::Exited,
            tool_type,
            now,
            at,
            None,
            None,
            &AdapterFlags::default(),
        )
    }
}

/// Thresholds used by the derived state machine.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AttentionConfig {
    pub quiescence: Duration,
    pub idle_after: Duration,
}

impl Default for AttentionConfig {
    fn default() -> Self {
        Self {
            quiescence: DEFAULT_QUIESCENCE,
            idle_after: DEFAULT_IDLE_AFTER,
        }
    }
}

struct AttentionEngine {
    tool_type: Option<String>,
    adapter: Box<dyn ToolAttentionAdapter>,
    config: AttentionConfig,
    started_at: i64,
    last_output_at: Option<i64>,
    last_content_change_at: Option<i64>,
    last_rendered: String,
    last_alternate_screen: bool,
    flags: AdapterFlags,
    exited: bool,
}

impl AttentionEngine {
    fn observe_output(
        &mut self,
        bytes: &[u8],
        rendered: &str,
        alternate_screen: bool,
        now_ms: i64,
    ) {
        if bytes.is_empty() {
            return;
        }

        self.last_output_at = Some(now_ms);
        if rendered != self.last_rendered || alternate_screen != self.last_alternate_screen {
            self.last_rendered.clear();
            self.last_rendered.push_str(rendered);
            self.last_alternate_screen = alternate_screen;
            self.last_content_change_at = Some(now_ms);
        }
        self.flags = self.adapter.inspect(AdapterObservation {
            rendered,
            alternate_screen,
        });
    }

    fn snapshot(&self, now_ms: i64) -> AgentState {
        let state = self.derive_state(now_ms);
        AgentState::from_snapshot(
            state,
            self.tool_type.clone(),
            now_ms,
            self.started_at,
            self.last_output_at,
            self.last_content_change_at,
            &self.flags,
        )
    }

    fn derive_state(&self, now_ms: i64) -> AttentionState {
        if self.exited {
            return AttentionState::Exited;
        }

        // A pending question always beats the idle fallback. It may sit unchanged
        // forever and must never look like a completed turn.
        if self.flags.needs_input {
            return AttentionState::NeedsInput;
        }
        if self.flags.busy {
            return AttentionState::Working;
        }

        let idle_since = self.last_output_at.unwrap_or(self.started_at);
        let output_quiet = elapsed(now_ms, idle_since);
        if output_quiet < self.config.quiescence {
            return AttentionState::Working;
        }
        if self.flags.resting_prompt {
            return AttentionState::Idle;
        }

        let content_since = self.last_content_change_at.unwrap_or(self.started_at);
        if elapsed(now_ms, content_since) >= self.config.idle_after {
            AttentionState::Idle
        } else {
            AttentionState::Working
        }
    }
}

/// Cloneable, thread-safe attention tracker for one hosted process.
#[derive(Clone)]
pub struct AttentionTracker {
    inner: Arc<Mutex<AttentionEngine>>,
}

impl AttentionTracker {
    /// Create a tracker and select an adapter from `tool_type`.
    pub fn new(tool_type: Option<String>) -> Self {
        Self::new_at(tool_type, AttentionConfig::default(), now_millis())
    }

    /// Create a tracker with deterministic time and thresholds.
    ///
    /// This is public so recorded terminal sessions can be replayed without sleeps.
    pub fn new_at(tool_type: Option<String>, config: AttentionConfig, now_ms: i64) -> Self {
        let adapter = adapter_for(tool_type.as_deref());
        Self::with_adapter_at(tool_type, config, now_ms, adapter)
    }

    /// Create a tracker with a caller-provided tool adapter.
    pub fn with_adapter_at(
        tool_type: Option<String>,
        config: AttentionConfig,
        now_ms: i64,
        adapter: Box<dyn ToolAttentionAdapter>,
    ) -> Self {
        Self {
            inner: Arc::new(Mutex::new(AttentionEngine {
                tool_type,
                adapter,
                config,
                started_at: now_ms,
                last_output_at: None,
                last_content_change_at: None,
                last_rendered: String::new(),
                last_alternate_screen: false,
                flags: AdapterFlags::default(),
                exited: false,
            })),
        }
    }

    fn lock(&self) -> MutexGuard<'_, AttentionEngine> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Record PTY bytes after they have been applied to the terminal emulator.
    pub fn observe_output(&self, bytes: &[u8], rendered: &str, alternate_screen: bool) {
        self.observe_output_at(bytes, rendered, alternate_screen, now_millis());
    }

    /// Deterministic form of [`Self::observe_output`] for recorded sessions.
    pub fn observe_output_at(
        &self,
        bytes: &[u8],
        rendered: &str,
        alternate_screen: bool,
        now_ms: i64,
    ) {
        self.lock()
            .observe_output(bytes, rendered, alternate_screen, now_ms);
    }

    /// Permanently transition this tracker to `exited`.
    pub fn mark_exited(&self) {
        self.mark_exited_at(now_millis());
    }

    /// Deterministic form of [`Self::mark_exited`] for recorded sessions.
    pub fn mark_exited_at(&self, _now_ms: i64) {
        self.lock().exited = true;
    }

    /// Read the current derived and raw state.
    pub fn snapshot(&self) -> AgentState {
        self.snapshot_at(now_millis())
    }

    /// Deterministic form of [`Self::snapshot`] for recorded sessions.
    pub fn snapshot_at(&self, now_ms: i64) -> AgentState {
        self.lock().snapshot(now_ms)
    }
}

impl fmt::Debug for AttentionTracker {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let engine = self.lock();
        formatter
            .debug_struct("AttentionTracker")
            .field("tool_type", &engine.tool_type)
            .field("started_at", &engine.started_at)
            .field("last_output_at", &engine.last_output_at)
            .field("last_content_change_at", &engine.last_content_change_at)
            .field("flags", &engine.flags)
            .field("exited", &engine.exited)
            .finish()
    }
}

fn adapter_for(tool_type: Option<&str>) -> Box<dyn ToolAttentionAdapter> {
    match tool_type.map(normalize_tool_type).as_deref() {
        Some("claude") | Some("claude_code") => Box::new(ClaudeCodeAdapter),
        _ => Box::new(PromptAdapter),
    }
}

fn normalize_tool_type(tool_type: &str) -> String {
    tool_type
        .trim()
        .to_ascii_lowercase()
        .replace([' ', '-'], "_")
}

/// Claude Code adapter for busy, permission, planning, and resting-prompt UI.
#[derive(Clone, Copy, Debug, Default)]
pub struct ClaudeCodeAdapter;

impl ToolAttentionAdapter for ClaudeCodeAdapter {
    fn inspect(&self, observation: AdapterObservation<'_>) -> AdapterFlags {
        let rendered = observation.rendered;
        let lowercase = rendered.to_lowercase();

        let permission_at = last_pattern(
            &lowercase,
            &[
                "do you want to proceed?",
                "allow this command?",
                "allow this tool?",
                "yes, allow",
                "no, and tell claude",
                "enter to confirm",
                "esc to cancel",
                "approve this action?",
                "would you like to run",
                "[y/n]",
                "(y/n)",
            ],
        );
        let spinner_at = last_pattern(rendered, &["✻", "✽", "✶", "✳", "✢"]);
        let busy_at = last_pattern(
            &lowercase,
            &[
                "thinking…",
                "thinking...",
                "working…",
                "working...",
                "running…",
                "running...",
                "planning…",
                "planning...",
                "esc to interrupt",
            ],
        )
        .max(spinner_at);
        let resting_at = last_resting_prompt(rendered);

        let needs_input = is_latest(permission_at, &[busy_at, resting_at]);
        let busy = !needs_input && is_latest(busy_at, &[permission_at, resting_at]);
        let resting_prompt = !needs_input && !busy && resting_at.is_some();
        let planning = busy
            && (lowercase.contains("planning")
                || lowercase.contains("plan mode")
                || lowercase.contains("creating a plan"));
        let explicitly_running = lowercase.contains("running")
            || lowercase.contains("working")
            || lowercase.contains("esc to interrupt");
        let thinking = busy
            && !planning
            && (lowercase.contains("thinking") || (spinner_at.is_some() && !explicitly_running));

        AdapterFlags {
            busy,
            needs_input,
            resting_prompt,
            thinking,
            planning,
            classification: if needs_input {
                Some("permission_dialog".into())
            } else if planning {
                Some("planning".into())
            } else if busy {
                Some("busy_spinner".into())
            } else if resting_prompt {
                Some("resting_prompt".into())
            } else if observation.alternate_screen {
                Some("alternate_screen".into())
            } else {
                None
            },
        }
    }
}

/// Generic shell/prompt adapter used when a tool has no dedicated adapter yet.
#[derive(Clone, Copy, Debug, Default)]
struct PromptAdapter;

impl ToolAttentionAdapter for PromptAdapter {
    fn inspect(&self, observation: AdapterObservation<'_>) -> AdapterFlags {
        let lowercase = observation.rendered.to_ascii_lowercase();
        let input_at = last_pattern(
            &lowercase,
            &["[y/n]", "(y/n)", "yes/no", "press enter to continue"],
        );
        let resting_at = last_resting_prompt(observation.rendered);
        let needs_input = is_latest(input_at, &[resting_at]);
        let resting_prompt = !needs_input && resting_at.is_some();
        AdapterFlags {
            needs_input,
            resting_prompt,
            classification: if needs_input {
                Some("input_prompt".into())
            } else if resting_prompt {
                Some("resting_prompt".into())
            } else {
                None
            },
            ..AdapterFlags::default()
        }
    }
}

fn last_resting_prompt(rendered: &str) -> Option<usize> {
    let mut offset = 0;
    let mut lines = Vec::new();
    for line in rendered.split_inclusive('\n') {
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            lines.push((offset, trimmed));
        }
        offset += line.len();
    }
    lines
        .into_iter()
        .rev()
        .take(4)
        .find_map(|(offset, line)| matches!(line, "❯" | ">" | "$" | "#").then_some(offset))
}

fn is_latest(candidate: Option<usize>, others: &[Option<usize>]) -> bool {
    candidate.is_some_and(|candidate| others.iter().flatten().all(|other| candidate >= *other))
}

fn last_pattern(haystack: &str, patterns: &[&str]) -> Option<usize> {
    patterns
        .iter()
        .filter_map(|pattern| haystack.rfind(pattern))
        .max()
}

fn elapsed(now_ms: i64, since_ms: i64) -> Duration {
    Duration::from_millis(now_ms.saturating_sub(since_ms).max(0) as u64)
}

fn elapsed_seconds(now_ms: i64, since_ms: i64) -> f64 {
    elapsed(now_ms, since_ms).as_secs_f64()
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
    use crate::terminal::TerminalEmulator;

    use super::*;

    struct ScriptedSession {
        terminal: TerminalEmulator,
        tracker: AttentionTracker,
    }

    impl ScriptedSession {
        fn claude() -> Self {
            Self {
                terminal: TerminalEmulator::new(12, 80, 100),
                tracker: AttentionTracker::new_at(
                    Some("claude_code".into()),
                    AttentionConfig::default(),
                    1_000,
                ),
            }
        }

        fn emit(&mut self, at: i64, bytes: &[u8]) {
            self.terminal.feed(bytes);
            let rendered = self
                .terminal
                .read_rows(self.terminal.history_rows()..usize::MAX);
            self.tracker
                .observe_output_at(bytes, &rendered.text(), rendered.alternate_screen, at);
        }
    }

    #[test]
    fn claude_script_transitions_working_input_idle_and_exited() {
        let mut session = ScriptedSession::claude();

        session.emit(1_100, b"\r\x1b[2K\xe2\x9c\xbb Thinking\xe2\x80\xa6");
        let working = session.tracker.snapshot_at(3_000);
        assert_eq!(working.state, AttentionState::Working);
        assert!(working.thinking);
        assert_eq!(working.classification.as_deref(), Some("busy_spinner"));

        session.emit(
            3_100,
            b"\x1b[2J\x1b[HClaude wants to use Bash\r\nDo you want to proceed?\r\n\xe2\x9d\xaf 1. Yes, allow\r\n  2. No, and tell Claude",
        );
        let waiting = session.tracker.snapshot_at(60_000);
        assert_eq!(waiting.state, AttentionState::NeedsInput);
        assert!(waiting.needs_input);
        assert!(!waiting.idle);
        assert!(!waiting.exited);
        assert_eq!(waiting.classification.as_deref(), Some("permission_dialog"));

        session.emit(
            60_100,
            b"\x1b[2J\x1b[H\xe2\x9c\xbb Running\xe2\x80\xa6 cargo test\r\nEsc to interrupt",
        );
        assert_eq!(
            session.tracker.snapshot_at(62_000).state,
            AttentionState::Working
        );

        session.emit(
            62_100,
            b"\x1b[2J\x1b[HFinished successfully\r\n\xe2\x9d\xaf ",
        );
        let idle = session.tracker.snapshot_at(62_700);
        assert_eq!(idle.state, AttentionState::Idle);
        assert!(idle.idle);

        session.tracker.mark_exited_at(63_000);
        assert_eq!(
            session.tracker.snapshot_at(63_000).state,
            AttentionState::Exited
        );
    }

    #[test]
    fn raw_activity_and_rendered_content_have_distinct_timestamps() {
        let mut session = ScriptedSession::claude();
        session.emit(2_000, b"hello");
        session.emit(3_000, b"\x07");

        let state = session.tracker.snapshot_at(3_250);
        assert_eq!(state.last_output_at, Some(3_000));
        assert_eq!(state.last_content_change_at, Some(2_000));
        assert_eq!(state.idle_seconds, 0.25);
    }

    #[test]
    fn planning_flag_is_separate_from_thinking() {
        let mut session = ScriptedSession::claude();
        session.emit(2_000, b"\xe2\x9c\xbb Planning\xe2\x80\xa6 creating a plan");

        let state = session.tracker.snapshot_at(10_000);
        assert_eq!(state.state, AttentionState::Working);
        assert!(state.planning);
        assert!(!state.thinking);
    }

    #[test]
    fn newer_resting_prompt_beats_stale_spinner_text() {
        let mut session = ScriptedSession::claude();
        session.emit(
            2_000,
            b"\xe2\x9c\xbb Thinking\xe2\x80\xa6\r\nAnswer complete\r\n\xe2\x9d\xaf ",
        );

        let state = session.tracker.snapshot_at(2_600);
        assert_eq!(state.state, AttentionState::Idle);
        assert_eq!(state.classification.as_deref(), Some("resting_prompt"));
    }

    #[test]
    fn generic_confirmation_never_falls_through_to_idle() {
        let tracker =
            AttentionTracker::new_at(Some("terminal".into()), AttentionConfig::default(), 1_000);
        tracker.observe_output_at(b"Continue? [y/n]", "Continue? [y/n]", false, 2_000);

        let state = tracker.snapshot_at(120_000);
        assert_eq!(state.state, AttentionState::NeedsInput);
        assert!(state.needs_input);
        assert!(!state.idle);
    }
}
