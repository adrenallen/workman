//! Process attention signals and tool-aware state classification.

use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::{ProcessId, TimerId, TimerKind};

/// Short output-silence window before a recognized resting prompt is idle.
pub const DEFAULT_QUIESCENCE: Duration = Duration::from_millis(500);

/// Conservative idle fallback when no adapter recognizes the terminal contents.
pub const DEFAULT_IDLE_AFTER: Duration = Duration::from_secs(5);

/// Stable silence required before a resting prompt becomes orchestration-idle.
///
/// This deliberately matches the established conservative fallback window. A
/// prompt-shaped frame is useful evidence, but interactive agents can briefly
/// redraw their composer between bursts while a turn is still running.
pub const DEFAULT_IDLE_CONFIRMATION: Duration = Duration::from_secs(5);

/// Grace period in which newly delivered input keeps a process out of idle.
pub const RECENT_INPUT_GRACE: Duration = Duration::from_secs(2);

/// Grace window for output caused by UI/protocol activity such as focus reports
/// and SIGWINCH redraws. These bytes still update rendered adapter facts, but
/// do not restart the agent's work/idle clock.
pub const UI_ORIGINATED_OUTPUT_GRACE: Duration = Duration::from_millis(500);

/// The orchestration-relevant state derived from output and terminal contents.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AttentionState {
    Working,
    NeedsInput,
    Waiting,
    Idle,
    Exited,
}

/// A named process participating in an idle-watch condition.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentWaitingProcess {
    pub process_id: ProcessId,
    pub process_name: String,
}

/// Durable timer metadata explaining why an otherwise-idle agent is parked.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentWaitingReason {
    pub timer_id: TimerId,
    pub kind: TimerKind,
    pub due_at: i64,
    /// Original hard-stop window chosen when the timer was armed.
    pub max_wait_ms: i64,
    pub remaining_ms: i64,
    pub paused: bool,
    pub watch_processes: Vec<AgentWaitingProcess>,
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

/// A rendered prompt that must be resolved before ordinary text is delivered.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PendingDialog {
    /// Adapter classification that caused the guard to engage.
    pub classification: String,
    /// Escape-free rendered terminal text shown to the caller.
    pub rendered: String,
    /// Whether this is a narrowly recognized first-run trust prompt whose
    /// affirmative default is safe for workman to acknowledge with Enter.
    pub known_first_run: bool,
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

    /// Sustained no-output window required before this adapter's idle evidence
    /// is published to notifications, timers, and other consumers.
    fn idle_confirmation(&self) -> Duration {
        DEFAULT_IDLE_CONFIRMATION
    }
}

/// Public status payload shaped for orchestration and UI consumers.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct AgentState {
    /// Derived state. This is the authoritative orchestration value.
    pub state: AttentionState,
    /// Compatibility booleans mirroring Solo's agent-state payload.
    pub working: bool,
    pub needs_input: bool,
    /// True only when an idle process has a pending timer or idle watch.
    #[serde(default)]
    pub waiting: bool,
    /// True when a pending timer will react to this process or deliver to it.
    #[serde(default)]
    pub watched: bool,
    /// True when an unwatched completed turn has not yet been viewed.
    #[serde(default)]
    pub unread: bool,
    pub idle: bool,
    pub exited: bool,
    pub thinking: bool,
    pub planning: bool,
    /// Tool family used to select the adapter.
    pub tool_type: Option<String>,
    /// Seconds since the last attention-relevant PTY output, with sub-second precision.
    pub idle_seconds: f64,
    /// Seconds since the last attention-relevant PTY output, with sub-second precision.
    pub last_output_seconds: Option<f64>,
    /// Unix timestamp in milliseconds for the newest attention-relevant PTY bytes.
    pub last_output_at: Option<i64>,
    /// Unix timestamp in milliseconds for the newest attention-relevant content change.
    pub last_content_change_at: Option<i64>,
    /// Unix timestamp in milliseconds for the newest PTY input write.
    #[serde(default)]
    pub last_input_at: Option<i64>,
    /// Seconds since the newest PTY input write.
    #[serde(default)]
    pub last_input_seconds: Option<f64>,
    /// Adapter explanation such as `busy_spinner` or `permission_dialog`.
    pub classification: Option<String>,
    /// Raw pending timers/watches used to derive `waiting`.
    #[serde(default)]
    pub waiting_on: Vec<AgentWaitingReason>,
}

impl AgentState {
    #[allow(clippy::too_many_arguments)]
    fn from_snapshot(
        state: AttentionState,
        tool_type: Option<String>,
        now_ms: i64,
        started_at: i64,
        last_output_at: Option<i64>,
        last_content_change_at: Option<i64>,
        last_input_at: Option<i64>,
        flags: &AdapterFlags,
    ) -> Self {
        let idle_since = last_output_at.unwrap_or(started_at);
        let idle_seconds = elapsed_seconds(now_ms, idle_since);
        Self {
            state,
            working: state == AttentionState::Working,
            needs_input: state == AttentionState::NeedsInput,
            waiting: state == AttentionState::Waiting,
            watched: false,
            unread: false,
            idle: state == AttentionState::Idle,
            exited: state == AttentionState::Exited,
            thinking: flags.thinking,
            planning: flags.planning,
            tool_type,
            idle_seconds,
            last_output_seconds: last_output_at.map(|at| elapsed_seconds(now_ms, at)),
            last_output_at,
            last_content_change_at,
            last_input_at,
            last_input_seconds: last_input_at.map(|at| elapsed_seconds(now_ms, at)),
            classification: flags.classification.clone(),
            waiting_on: Vec::new(),
        }
    }

    /// Refine an idle snapshot with durable timer state without weakening
    /// working, needs-input, or exited attention semantics.
    pub fn refine_waiting(&mut self, waiting_on: Vec<AgentWaitingReason>) {
        if self.state == AttentionState::Idle && !waiting_on.is_empty() {
            self.state = AttentionState::Waiting;
            self.waiting = true;
            // Waiting is intentionally a refinement of idle for compatibility
            // with existing idle-transition orchestration.
            self.idle = true;
        }
        self.waiting_on = waiting_on;
    }

    /// Attach durable completion-notification metadata to a live snapshot.
    pub fn refine_notifications(&mut self, watched: bool, unread: bool) {
        self.watched = watched;
        self.unread = unread;
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
    last_input_at: Option<i64>,
    last_rendered: String,
    last_alternate_screen: bool,
    flags: AdapterFlags,
    attention_neutral_until: Option<i64>,
    /// Once a recognized resting prompt has become idle, control-only and
    /// cosmetic prompt repaints remain idle until input or an explicit adapter
    /// signal starts a new working episode.
    idle_prompt_latched: bool,
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

        let flags = self.adapter.inspect(AdapterObservation {
            rendered,
            alternate_screen,
        });
        let explicit_attention = flags.busy || flags.needs_input;
        if explicit_attention {
            self.idle_prompt_latched = false;
        }
        let attention_neutral = self
            .attention_neutral_until
            .is_some_and(|until| now_ms <= until)
            || (self.idle_prompt_latched && !explicit_attention);
        if !attention_neutral {
            self.last_output_at = Some(now_ms);
        }
        if rendered != self.last_rendered || alternate_screen != self.last_alternate_screen {
            self.last_rendered.clear();
            self.last_rendered.push_str(rendered);
            self.last_alternate_screen = alternate_screen;
            if !attention_neutral {
                self.last_content_change_at = Some(now_ms);
            }
        }
        self.flags = flags;
    }

    fn snapshot(&mut self, now_ms: i64) -> AgentState {
        let state = self.derive_state(now_ms);
        if state == AttentionState::Idle && self.flags.resting_prompt {
            self.idle_prompt_latched = true;
        }
        AgentState::from_snapshot(
            state,
            self.tool_type.clone(),
            now_ms,
            self.started_at,
            self.last_output_at,
            self.last_content_change_at,
            self.last_input_at,
            &self.flags,
        )
    }

    fn derive_state(&self, now_ms: i64) -> AttentionState {
        if self.exited {
            return AttentionState::Exited;
        }

        if self
            .last_input_at
            .is_some_and(|at| elapsed(now_ms, at) < RECENT_INPUT_GRACE)
        {
            return AttentionState::Working;
        }

        // A pending question always beats the idle fallback. It may sit unchanged
        // forever and must never look like a completed turn.
        if self.flags.needs_input {
            return AttentionState::NeedsInput;
        }
        if self.flags.busy {
            return AttentionState::Working;
        }

        // Treat idle as a stable state, not a single prompt-shaped frame. PTY
        // output, rendered state changes, and input all reset the same candidate
        // window so every downstream consumer sees one debounced stream.
        let stable_since = [
            Some(self.started_at),
            self.last_output_at,
            self.last_content_change_at,
            self.last_input_at,
        ]
        .into_iter()
        .flatten()
        .max()
        .unwrap_or(self.started_at);
        let stable_for = elapsed(now_ms, stable_since);
        let confirmation = self.adapter.idle_confirmation().max(self.config.quiescence);
        if stable_for < confirmation {
            return AttentionState::Working;
        }
        if self.flags.resting_prompt {
            return AttentionState::Idle;
        }

        if stable_for >= self.config.idle_after.max(confirmation) {
            AttentionState::Idle
        } else {
            AttentionState::Working
        }
    }

    fn next_transition_at(&self, now_ms: i64) -> Option<i64> {
        if self.exited {
            return None;
        }

        if let Some(last_input_at) = self.last_input_at {
            let recent_input_ends_at =
                last_input_at.saturating_add(duration_millis(RECENT_INPUT_GRACE));
            if recent_input_ends_at > now_ms {
                return Some(recent_input_ends_at);
            }
        }
        if self.flags.needs_input || self.flags.busy {
            return None;
        }

        let stable_since = [
            Some(self.started_at),
            self.last_output_at,
            self.last_content_change_at,
            self.last_input_at,
        ]
        .into_iter()
        .flatten()
        .max()
        .unwrap_or(self.started_at);
        let confirmation = self.adapter.idle_confirmation().max(self.config.quiescence);
        let idle_at = stable_since.saturating_add(duration_millis(confirmation));
        (idle_at > now_ms).then_some(idle_at)
    }
}

type AttentionInvalidation = Arc<dyn Fn(Option<i64>) + Send + Sync>;

/// Cloneable, thread-safe attention tracker for one hosted process.
#[derive(Clone)]
pub struct AttentionTracker {
    inner: Arc<Mutex<AttentionEngine>>,
    invalidation: Arc<Mutex<Option<AttentionInvalidation>>>,
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
                last_input_at: None,
                last_rendered: String::new(),
                last_alternate_screen: false,
                flags: AdapterFlags::default(),
                attention_neutral_until: None,
                idle_prompt_latched: false,
                exited: false,
            })),
            invalidation: Arc::new(Mutex::new(None)),
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
        let next_transition_at = {
            let mut engine = self.lock();
            engine.observe_output(bytes, rendered, alternate_screen, now_ms);
            engine.next_transition_at(now_ms)
        };
        self.notify_invalidation(next_transition_at);
    }

    /// Record input delivered to the process PTY.
    pub fn observe_input(&self) {
        self.observe_input_at(now_millis());
    }

    /// Deterministic form of [`Self::observe_input`].
    pub fn observe_input_at(&self, now_ms: i64) {
        let next_transition_at = {
            let mut engine = self.lock();
            engine.last_input_at = Some(now_ms);
            engine.idle_prompt_latched = false;
            engine.next_transition_at(now_ms)
        };
        self.notify_invalidation(next_transition_at);
    }

    /// Keep UI-originated PTY writes and their immediate redraw output from
    /// perturbing the agent attention clock.
    pub fn suppress_ui_activity(&self) {
        self.suppress_ui_activity_at(now_millis());
    }

    /// Deterministic form of [`Self::suppress_ui_activity`].
    pub fn suppress_ui_activity_at(&self, now_ms: i64) {
        let until = now_ms.saturating_add(
            UI_ORIGINATED_OUTPUT_GRACE
                .as_millis()
                .try_into()
                .unwrap_or(i64::MAX),
        );
        let mut engine = self.lock();
        engine.attention_neutral_until = Some(
            engine
                .attention_neutral_until
                .map_or(until, |existing| existing.max(until)),
        );
    }

    /// Permanently transition this tracker to `exited`.
    pub fn mark_exited(&self) {
        self.mark_exited_at(now_millis());
    }

    /// Deterministic form of [`Self::mark_exited`] for recorded sessions.
    pub fn mark_exited_at(&self, _now_ms: i64) {
        self.lock().exited = true;
        self.notify_invalidation(None);
    }

    /// Read the current derived and raw state.
    pub fn snapshot(&self) -> AgentState {
        self.snapshot_at(now_millis())
    }

    /// Deterministic form of [`Self::snapshot`] for recorded sessions.
    pub fn snapshot_at(&self, now_ms: i64) -> AgentState {
        self.lock().snapshot(now_ms)
    }

    /// Return the next Unix-millisecond time-only state edge, if no activity arrives.
    pub fn next_transition_at(&self, now_ms: i64) -> Option<i64> {
        self.lock().next_transition_at(now_ms)
    }

    /// Notify a host when PTY classification/input changes can affect status.
    /// The optional value is the next Unix-millisecond time-only edge.
    pub fn set_invalidation_callback(
        &self,
        callback: impl Fn(Option<i64>) + Send + Sync + 'static,
    ) {
        *self
            .invalidation
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(Arc::new(callback));
        self.notify_invalidation(self.next_transition_at(now_millis()));
    }

    /// Wake the registered host after an external lifecycle signal such as PTY EOF.
    pub(crate) fn notify_change(&self) {
        self.notify_invalidation(self.next_transition_at(now_millis()));
    }

    fn notify_invalidation(&self, next_transition_at: Option<i64>) {
        let callback = self
            .invalidation
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        if let Some(callback) = callback {
            callback(next_transition_at);
        }
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
            .field("last_input_at", &engine.last_input_at)
            .field("flags", &engine.flags)
            .field("exited", &engine.exited)
            .finish()
    }
}

fn adapter_for(tool_type: Option<&str>) -> Box<dyn ToolAttentionAdapter> {
    match tool_type.map(normalize_tool_type).as_deref() {
        Some("claude") | Some("claude_code") => Box::new(ClaudeCodeAdapter),
        Some("codex") | Some("codex_cli") => Box::new(CodexAdapter),
        _ => Box::new(PromptAdapter),
    }
}

fn normalize_tool_type(tool_type: &str) -> String {
    tool_type
        .trim()
        .to_ascii_lowercase()
        .replace([' ', '-'], "_")
}

fn duration_millis(duration: Duration) -> i64 {
    duration.as_millis().try_into().unwrap_or(i64::MAX)
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
                "do you trust the files in this folder?",
                "do you trust the contents of this folder?",
                "trust this folder?",
                "[y/n]",
                "(y/n)",
            ],
        );
        let spinner_at = last_claude_active_spinner(rendered);
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
        let resting_at = last_claude_resting_prompt(rendered);

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

/// Codex CLI adapter for its composer, activity line, and confirmation menus.
#[derive(Clone, Copy, Debug, Default)]
pub struct CodexAdapter;

impl ToolAttentionAdapter for CodexAdapter {
    fn inspect(&self, observation: AdapterObservation<'_>) -> AdapterFlags {
        let rendered = observation.rendered;
        let lowercase = rendered.to_lowercase();
        let permission_at = last_pattern(
            &lowercase,
            &[
                "do you trust the contents of this directory?",
                "would you like to run",
                "press enter to continue",
                "yes, continue",
                "no, quit",
            ],
        );
        let busy_at = last_pattern(
            &lowercase,
            &["esc to interrupt", "working (", "thinking (", "running ("],
        );
        let resting_at = last_codex_resting_prompt(rendered);

        let needs_input = is_latest(permission_at, &[busy_at, resting_at]);
        let busy = !needs_input && is_latest(busy_at, &[permission_at, resting_at]);
        let resting_prompt = !needs_input && !busy && resting_at.is_some();
        let thinking = busy && lowercase.contains("thinking");
        let planning = busy && lowercase.contains("planning");
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
        let known_first_run = is_known_first_run_trust_dialog(observation.rendered);
        AdapterFlags {
            needs_input,
            resting_prompt,
            classification: if needs_input {
                Some(if known_first_run {
                    "permission_dialog".into()
                } else {
                    "input_prompt".into()
                })
            } else if resting_prompt {
                Some("resting_prompt".into())
            } else {
                None
            },
            ..AdapterFlags::default()
        }
    }
}

/// Recognize a currently rendered dialog that should block ordinary text input.
///
/// Permission adapters classify explicit confirmations directly. Generic input
/// prompts are guarded only when they contain a numbered choice menu, avoiding
/// false positives on ordinary shell prompts and informational text.
pub fn pending_dialog(rendered: &str, classification: Option<&str>) -> Option<PendingDialog> {
    let classification = classification?;
    let known_first_run = is_known_first_run_trust_dialog(rendered);
    let guard = known_first_run
        || classification == "permission_dialog"
        || (classification == "input_prompt" && has_numbered_choice_menu(rendered));
    guard.then(|| PendingDialog {
        classification: classification.to_owned(),
        rendered: rendered.trim().to_owned(),
        known_first_run,
    })
}

/// Return true only for known, affirmative-by-default first-run trust prompts.
pub fn is_known_first_run_trust_dialog(rendered: &str) -> bool {
    let lowercase = rendered.to_ascii_lowercase();
    let trust_question = [
        "do you trust the contents of this directory?",
        "do you trust the files in this folder?",
        "do you trust the contents of this folder?",
        "trust this folder?",
    ]
    .iter()
    .any(|pattern| lowercase.contains(pattern));
    let affirmative = [
        "yes, continue",
        "yes, proceed",
        "yes, i trust",
        "yes, trust",
    ]
    .iter()
    .any(|pattern| lowercase.contains(pattern));
    let negative = ["no, quit", "no, exit", "no, go back", "don't trust"]
        .iter()
        .any(|pattern| lowercase.contains(pattern));
    trust_question && affirmative && negative && has_numbered_choice_menu(rendered)
}

fn has_numbered_choice_menu(rendered: &str) -> bool {
    nonempty_lines(rendered)
        .into_iter()
        .filter(|(_, line)| {
            let line = line
                .trim_start_matches(['›', '❯', '>', '*', '•'])
                .trim_start();
            let Some((number, choice)) = line.split_once('.') else {
                return false;
            };
            !number.is_empty()
                && number.chars().all(|character| character.is_ascii_digit())
                && !choice.trim().is_empty()
        })
        .take(2)
        .count()
        >= 2
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

fn last_claude_resting_prompt(rendered: &str) -> Option<usize> {
    nonempty_lines(rendered)
        .into_iter()
        .rev()
        .take(8)
        .find_map(|(offset, line)| {
            let draft = line.strip_prefix('❯')?.trim();
            (!is_claude_dialog_choice(draft)).then_some(offset)
        })
}

fn last_codex_resting_prompt(rendered: &str) -> Option<usize> {
    nonempty_lines(rendered)
        .into_iter()
        .rev()
        .take(8)
        .find_map(|(offset, line)| line.starts_with('›').then_some(offset))
}

fn is_claude_dialog_choice(text: &str) -> bool {
    let Some((number, choice)) = text.split_once('.') else {
        return false;
    };
    !number.is_empty()
        && number.chars().all(|character| character.is_ascii_digit())
        && !choice.trim().is_empty()
}

fn last_claude_active_spinner(rendered: &str) -> Option<usize> {
    const SPINNERS: [&str; 5] = ["✻", "✽", "✶", "✳", "✢"];

    nonempty_lines(rendered)
        .into_iter()
        .filter(|(_, line)| {
            (line.contains('…') || line.contains("..."))
                && SPINNERS.iter().any(|spinner| line.contains(spinner))
        })
        .map(|(offset, _)| offset)
        .max()
}

fn nonempty_lines(rendered: &str) -> Vec<(usize, &str)> {
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

        fn codex() -> Self {
            Self {
                terminal: TerminalEmulator::new(12, 80, 100),
                tracker: AttentionTracker::new_at(
                    Some("codex".into()),
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
    fn codex_adapter_distinguishes_a_visible_draft_from_a_started_turn() {
        let mut session = ScriptedSession::codex();
        session.emit(1_100, "\x1b[2J\x1b[H› queued wake body".as_bytes());
        assert_eq!(
            session.tracker.snapshot_at(6_099).state,
            AttentionState::Working
        );
        let draft = session.tracker.snapshot_at(6_100);
        assert_eq!(draft.state, AttentionState::Idle);
        assert_eq!(draft.classification.as_deref(), Some("resting_prompt"));

        session.emit(
            1_800,
            "\x1b[2J\x1b[H• Working (0s • esc to interrupt)".as_bytes(),
        );
        let working = session.tracker.snapshot_at(2_400);
        assert_eq!(working.state, AttentionState::Working);
        assert_eq!(working.classification.as_deref(), Some("busy_spinner"));
    }

    #[test]
    fn waiting_refines_only_idle_and_keeps_idle_compatibility() {
        let reason = AgentWaitingReason {
            timer_id: 42,
            kind: TimerKind::IdleAll,
            due_at: 10_000,
            max_wait_ms: 9_000,
            remaining_ms: 9_000,
            paused: false,
            watch_processes: vec![AgentWaitingProcess {
                process_id: 7,
                process_name: "codex-w2".into(),
            }],
        };
        let tracker = AttentionTracker::new_at(None, AttentionConfig::default(), 1_000);
        let mut idle = tracker.snapshot_at(10_000);
        assert_eq!(idle.state, AttentionState::Idle);
        idle.refine_waiting(vec![reason.clone()]);
        assert_eq!(idle.state, AttentionState::Waiting);
        assert!(idle.waiting);
        assert!(idle.idle);
        assert!(!idle.needs_input);
        assert_eq!(idle.waiting_on, std::slice::from_ref(&reason));

        let mut working = tracker.snapshot_at(1_100);
        working.refine_waiting(vec![reason]);
        assert_eq!(working.state, AttentionState::Working);
        assert!(!working.waiting);
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
        assert_eq!(
            session.tracker.snapshot_at(67_099).state,
            AttentionState::Working
        );
        let idle = session.tracker.snapshot_at(67_100);
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

        assert_eq!(
            session.tracker.snapshot_at(6_999).state,
            AttentionState::Working
        );
        let state = session.tracker.snapshot_at(7_000);
        assert_eq!(state.state, AttentionState::Idle);
        assert_eq!(state.classification.as_deref(), Some("resting_prompt"));
    }

    #[test]
    fn real_claude_resting_screen_with_draft_is_idle() {
        let rendered = include_str!("../tests/fixtures/attention/claude_resting_with_draft.txt");
        let tracker = AttentionTracker::new_at(
            Some("claude_code".into()),
            AttentionConfig::default(),
            1_000,
        );
        tracker.observe_output_at(rendered.as_bytes(), rendered, false, 2_000);

        assert_eq!(tracker.snapshot_at(6_999).state, AttentionState::Working);
        assert_eq!(tracker.snapshot_at(7_000).state, AttentionState::Idle);
        let state = tracker.snapshot_at(85_000);
        assert_eq!(state.state, AttentionState::Idle);
        assert!(!state.working);
        assert!(!state.thinking);
        assert_eq!(state.classification.as_deref(), Some("resting_prompt"));
    }

    #[test]
    fn idle_prompt_repaints_do_not_start_a_working_episode() {
        let mut session = ScriptedSession::claude();
        session.emit(2_000, b"finished\r\n\xe2\x9d\xaf ");
        assert_eq!(
            session.tracker.snapshot_at(7_000).state,
            AttentionState::Idle
        );

        session.emit(
            400_000,
            b"\x1b[?2026h\x1b[H\x1b[2Kfinished\r\n\xe2\x9d\xaf \r\n1 agent\x1b[?2026l",
        );
        let repaint = session.tracker.snapshot_at(400_000);
        assert_eq!(repaint.state, AttentionState::Idle);
        assert_eq!(repaint.last_output_at, Some(2_000));
        assert_eq!(repaint.last_content_change_at, Some(2_000));

        session.emit(
            401_000,
            b"\x1b[2J\x1b[H\xe2\x9c\xbb Thinking\xe2\x80\xa6\r\nEsc to interrupt",
        );
        let working = session.tracker.snapshot_at(401_000);
        assert_eq!(working.state, AttentionState::Working);
        assert_eq!(working.last_output_at, Some(401_000));
    }

    #[test]
    fn completed_claude_timing_line_is_not_an_active_spinner() {
        let flags = ClaudeCodeAdapter.inspect(AdapterObservation {
            rendered: "✻ Cogitated for 1s",
            alternate_screen: false,
        });

        assert!(!flags.busy);
        assert!(!flags.thinking);
        assert_ne!(flags.classification.as_deref(), Some("busy_spinner"));
    }

    #[test]
    fn real_claude_working_screen_stays_working() {
        let rendered = include_str!("../tests/fixtures/attention/claude_working.txt");
        let tracker = AttentionTracker::new_at(
            Some("claude_code".into()),
            AttentionConfig::default(),
            1_000,
        );
        tracker.observe_output_at(rendered.as_bytes(), rendered, false, 2_000);

        let state = tracker.snapshot_at(85_000);
        assert_eq!(state.state, AttentionState::Working);
        assert!(state.working);
        assert!(state.thinking);
        assert_eq!(state.classification.as_deref(), Some("busy_spinner"));
    }

    #[test]
    fn real_claude_permission_dialog_still_needs_input() {
        let rendered = include_str!("../tests/fixtures/attention/claude_permission_dialog.txt");
        let tracker = AttentionTracker::new_at(
            Some("claude_code".into()),
            AttentionConfig::default(),
            1_000,
        );
        tracker.observe_output_at(rendered.as_bytes(), rendered, false, 2_000);

        let state = tracker.snapshot_at(85_000);
        assert_eq!(state.state, AttentionState::NeedsInput);
        assert!(state.needs_input);
        assert!(!state.idle);
        assert_eq!(state.classification.as_deref(), Some("permission_dialog"));
    }

    #[test]
    fn recent_input_prevents_a_resting_prompt_from_racing_back_to_idle() {
        let mut session = ScriptedSession::claude();
        session.emit(2_000, b"Answer complete\r\n\xe2\x9d\xaf ");
        assert_eq!(
            session.tracker.snapshot_at(7_000).state,
            AttentionState::Idle
        );

        session.tracker.observe_input_at(7_100);
        let prompted = session.tracker.snapshot_at(7_100);
        assert_eq!(prompted.state, AttentionState::Working);
        assert_eq!(prompted.last_input_at, Some(7_100));
        assert_eq!(prompted.last_input_seconds, Some(0.0));
        assert_eq!(
            session.tracker.snapshot_at(12_099).state,
            AttentionState::Working
        );
        assert_eq!(
            session.tracker.snapshot_at(12_100).state,
            AttentionState::Idle
        );
    }

    #[test]
    fn invalidation_callback_reports_output_and_time_driven_edges() {
        let tracker = AttentionTracker::new_at(
            Some("claude_code".into()),
            AttentionConfig::default(),
            1_000,
        );
        let observed = Arc::new(Mutex::new(Vec::new()));
        let callback_observed = observed.clone();
        tracker.set_invalidation_callback(move |deadline| {
            callback_observed.lock().unwrap().push(deadline);
        });
        observed.lock().unwrap().clear();

        tracker.observe_output_at(b"done\r\n\xe2\x9d\xaf ", "done\r\n\u{276f} ", false, 2_000);
        assert_eq!(*observed.lock().unwrap(), [Some(7_000)]);

        tracker.observe_input_at(7_100);
        assert_eq!(observed.lock().unwrap().last(), Some(&Some(9_100)));

        tracker.mark_exited_at(8_000);
        assert_eq!(observed.lock().unwrap().last(), Some(&None));
    }

    #[test]
    fn bursty_prompt_frames_never_publish_idle_until_the_final_stable_gap() {
        let mut session = ScriptedSession::claude();
        session.emit(2_000, b"partial answer\r\n\xe2\x9d\xaf ");
        assert_eq!(
            session.tracker.snapshot_at(6_999).state,
            AttentionState::Working
        );

        session.emit(7_000, b"\x1b[2J\x1b[H\xe2\x9c\xbb Thinking\xe2\x80\xa6");
        assert_eq!(
            session.tracker.snapshot_at(20_000).state,
            AttentionState::Working,
            "an explicit busy frame remains working regardless of silence"
        );
        session.emit(20_100, b"\x1b[2J\x1b[Hfinal answer\r\n\xe2\x9d\xaf ");
        assert_eq!(
            session.tracker.snapshot_at(25_099).state,
            AttentionState::Working
        );
        assert_eq!(
            session.tracker.snapshot_at(25_100).state,
            AttentionState::Idle
        );
    }

    #[test]
    fn adapter_can_override_the_idle_confirmation_window() {
        struct FastRecordedAdapter;

        impl ToolAttentionAdapter for FastRecordedAdapter {
            fn inspect(&self, _observation: AdapterObservation<'_>) -> AdapterFlags {
                AdapterFlags {
                    resting_prompt: true,
                    classification: Some("recorded_resting_prompt".into()),
                    ..AdapterFlags::default()
                }
            }

            fn idle_confirmation(&self) -> Duration {
                Duration::from_secs(1)
            }
        }

        let tracker = AttentionTracker::with_adapter_at(
            Some("recorded".into()),
            AttentionConfig::default(),
            1_000,
            Box::new(FastRecordedAdapter),
        );
        tracker.observe_output_at(b">", ">", false, 2_000);
        assert_eq!(tracker.snapshot_at(2_999).state, AttentionState::Working);
        assert_eq!(tracker.snapshot_at(3_000).state, AttentionState::Idle);
    }

    #[test]
    fn ui_originated_redraws_are_attention_neutral() {
        let mut session = ScriptedSession::claude();
        session.emit(2_000, b"finished\r\n\xe2\x9d\xaf ");
        assert_eq!(
            session.tracker.snapshot_at(7_000).state,
            AttentionState::Idle
        );

        session.tracker.suppress_ui_activity_at(7_100);
        session.emit(7_200, b"\x1b[2J\x1b[Hfocused redraw\r\n\xe2\x9d\xaf ");
        let focused = session.tracker.snapshot_at(7_200);
        assert_eq!(focused.state, AttentionState::Idle);
        assert_eq!(focused.last_output_at, Some(2_000));
        assert_eq!(focused.last_content_change_at, Some(2_000));

        session.emit(7_601, b"\x1b[2J\x1b[Hstatus refresh\r\n\xe2\x9d\xaf ");
        let cosmetic = session.tracker.snapshot_at(7_601);
        assert_eq!(cosmetic.state, AttentionState::Idle);
        assert_eq!(cosmetic.last_output_at, Some(2_000));

        session.emit(
            7_700,
            b"\x1b[2J\x1b[H\xe2\x9c\xbb Thinking\xe2\x80\xa6\r\nEsc to interrupt",
        );
        assert_eq!(
            session.tracker.snapshot_at(7_700).state,
            AttentionState::Working
        );
        assert_eq!(
            session.tracker.snapshot_at(7_700).last_output_at,
            Some(7_700)
        );
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

    #[test]
    fn codex_first_run_trust_menu_is_a_known_permission_dialog() {
        let rendered = concat!(
            "You are in /tmp/new-workspace\n",
            "Do you trust the contents of this directory? Working with untrusted contents poses security risks.\n",
            "› 1. Yes, continue\n",
            "  2. No, quit\n",
            "Press enter to continue",
        );
        let tracker =
            AttentionTracker::new_at(Some("codex".into()), AttentionConfig::default(), 1_000);
        tracker.observe_output_at(rendered.as_bytes(), rendered, true, 2_000);

        let state = tracker.snapshot_at(60_000);
        assert_eq!(state.state, AttentionState::NeedsInput);
        assert_eq!(state.classification.as_deref(), Some("permission_dialog"));
        assert_eq!(
            pending_dialog(rendered, state.classification.as_deref()),
            Some(PendingDialog {
                classification: "permission_dialog".into(),
                rendered: rendered.into(),
                known_first_run: true,
            })
        );
    }

    #[test]
    fn generic_numbered_input_is_guarded_but_plain_continue_text_is_not() {
        let menu = "Choose a mode:\n1. Safe\n2. Quit\nPress enter to continue";
        assert!(pending_dialog(menu, Some("input_prompt")).is_some());
        assert!(pending_dialog("Press enter to continue", Some("input_prompt")).is_none());
    }
}
