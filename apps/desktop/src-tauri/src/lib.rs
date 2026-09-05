use std::{
    collections::VecDeque,
    env,
    error::Error,
    ffi::OsString,
    io,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

#[cfg(debug_assertions)]
use std::{fs::OpenOptions, io::Write};

use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tauri::{
    Emitter, Manager, RunEvent, State, WindowEvent,
    menu::{Menu, MenuBuilder, MenuItemBuilder, MenuItemKind, SubmenuBuilder, WINDOW_SUBMENU_ID},
};
use tokio::{
    sync::{mpsc, oneshot},
    time::{MissedTickBehavior, interval, sleep, timeout},
};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
    tungstenite::{Message, client::IntoClientRequest},
};
use workmand::{
    BUILD_ID, BUILD_VERSION, CONTROL_PROTOCOL_VERSION, DaemonConfig, DaemonServer, DaemonVersion,
    Discovery, UserEnvironmentResolver, default_data_dir, discover_or_spawn, probe,
    user_config_path,
};

mod external_navigation;
mod native_notifications;
mod recorded_feedback;
mod terminal_clipboard;

const STATUS_EVENT: &str = "daemon://status";
const MESSAGE_EVENT: &str = "daemon://message";
const NATIVE_MENU_EVENT: &str = "menu://action";
const KEEP_AWAKE_RESYNC_EVENT: &str = "keep-awake://resync";
const MENU_ABOUT: &str = "workman.about";
const MENU_SETTINGS: &str = "workman.settings";
const MENU_CHECK_UPDATES: &str = "workman.check_updates";
const MENU_PREVIOUS_VIEW: &str = "view.previous_view";
const MENU_TOGGLE_PROJECT_RAIL: &str = "view.toggle_project_rail";
const MENU_TOGGLE_SECTION_RAIL: &str = "view.toggle_section_rail";
const TERMINAL_FRAME_MAGIC: &[u8; 4] = b"WRK1";
const TERMINAL_FRAME_HEADER_LEN: usize = 21;
const TERMINAL_INPUT_MAGIC: &[u8; 4] = b"WRI1";
const TERMINAL_INPUT_HEADER_LEN: usize = 12;
const HELLO_REQUEST_ID: &str = "__workman_desktop_hello__";
const HELLO_TIMEOUT: Duration = Duration::from_millis(750);
const RESTART_TIMEOUT: Duration = Duration::from_secs(6);
const BRIDGE_WRITE_TIMEOUT: Duration = Duration::from_millis(250);
const BRIDGE_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(1);
const BRIDGE_HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(3);
const BRIDGE_CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
const KEEP_AWAKE_WATCHDOG_INTERVAL: Duration = Duration::from_secs(1);
const KEEP_AWAKE_MAX_RETRY_DELAY: Duration = Duration::from_secs(60);
const KEEP_AWAKE_AUTO_SETTLE: Duration = Duration::from_secs(60);
const KEEP_AWAKE_MAX_OBSERVATION_GAP: Duration = Duration::from_secs(5);
const KEEP_AWAKE_MAX_SNAPSHOT_AGE: Duration = Duration::from_secs(10 * 60);
const KEEP_AWAKE_SUBSCRIPTION_REASSERT_INTERVAL: Duration = Duration::from_secs(30);
const KEEP_AWAKE_STATUS_SUBSCRIBE_ID: &str = "__workman_keep_awake_status_subscribe__";
const KEEP_AWAKE_PREFERENCE_FILE: &str = "desktop-keep-awake.json";

#[derive(Clone)]
struct BridgeState {
    sender: mpsc::Sender<BridgeCommand>,
    input_sender: mpsc::Sender<TerminalInput>,
    status: Arc<Mutex<ConnectionStatus>>,
}

#[derive(Clone)]
struct KeepAwakeState {
    inner: Arc<Mutex<KeepAwakeInner>>,
    preference_path: Option<Arc<PathBuf>>,
}

#[derive(Default)]
struct KeepAwakeInner {
    armed: bool,
    arm_source: Option<KeepAwakeArmSource>,
    manual_requested: bool,
    child: Option<Child>,
    warning: Option<String>,
    notice: Option<String>,
    respawn_count: u32,
    last_loss_reason: Option<String>,
    consecutive_spawn_failures: u32,
    next_spawn_attempt_at: Option<Instant>,
    auto_enabled: bool,
    auto_suppressed_until_activity_edge: bool,
    auto_active_agent_ids: Vec<i64>,
    auto_hold_requested: bool,
    auto_idle_observed: Duration,
    auto_last_idle_observation_at: Option<Instant>,
    auto_observation_continuous: bool,
    daemon_connected: bool,
    auto_last_snapshot_at: Option<Instant>,
    auto_snapshot_stale: bool,
    auto_last_subscription_assertion_at: Option<Instant>,
    auto_preference_warning: Option<String>,
    last_emitted_status: Option<KeepAwakeStatus>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
enum KeepAwakeArmSource {
    Manual,
    Auto,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct KeepAwakeStatus {
    supported: bool,
    armed: bool,
    active: bool,
    arm_source: Option<KeepAwakeArmSource>,
    assertion_pid: Option<u32>,
    warning: Option<String>,
    notice: Option<String>,
    respawn_count: u32,
    last_loss_reason: Option<String>,
    retry_in_ms: Option<u64>,
    auto_enabled: bool,
    auto_should_hold: bool,
    auto_suppressed_until_activity_edge: bool,
    auto_active_agent_ids: Vec<i64>,
    auto_snapshot_stale: bool,
    auto_snapshot_max_age_ms: u64,
    auto_preference_warning: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
struct PersistedKeepAwakePreference {
    #[serde(default)]
    auto_enabled: bool,
    #[serde(default)]
    suppressed_until_activity_edge: bool,
    #[serde(default)]
    suppressed_active_agent_ids: Vec<i64>,
}

#[derive(Debug, Deserialize)]
struct AutoKeepAwakeProcess {
    id: i64,
    kind: String,
    status: String,
    agent_state: AutoKeepAwakeAgentState,
}

#[derive(Debug, Default, Deserialize)]
struct AutoKeepAwakeAgentState {
    #[serde(default)]
    state: String,
    #[serde(default)]
    working: bool,
    #[serde(default)]
    needs_input: bool,
    #[serde(default)]
    thinking: bool,
    #[serde(default)]
    planning: bool,
    #[serde(default)]
    last_output_at: Option<i64>,
    #[serde(default)]
    waiting_on: Vec<AutoKeepAwakeWaitingReason>,
}

#[derive(Debug, Deserialize)]
struct AutoKeepAwakeWaitingReason {
    #[serde(default)]
    max_wait_ms: i64,
    #[serde(default)]
    remaining_ms: i64,
    #[serde(default)]
    paused: bool,
}

impl Default for KeepAwakeState {
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(KeepAwakeInner::default())),
            preference_path: None,
        }
    }
}

impl KeepAwakeState {
    fn persistent(preference_path: PathBuf) -> Self {
        let (preference, auto_preference_warning) =
            match load_keep_awake_preference(&preference_path) {
                Ok(preference) => (preference, None),
                Err(error) => (
                    PersistedKeepAwakePreference::default(),
                    Some(format!(
                        "Could not load the native auto keep-awake preference: {error}"
                    )),
                ),
            };
        let inner = KeepAwakeInner {
            auto_enabled: preference.auto_enabled,
            auto_suppressed_until_activity_edge: preference.suppressed_until_activity_edge,
            auto_active_agent_ids: normalized_agent_ids(preference.suppressed_active_agent_ids),
            // A persisted snapshot is only an edge baseline. The daemon must publish current
            // state before native auto intent can arm or clear suppression.
            auto_observation_continuous: false,
            auto_preference_warning,
            ..KeepAwakeInner::default()
        };
        Self {
            inner: Arc::new(Mutex::new(inner)),
            preference_path: Some(Arc::new(preference_path)),
        }
    }

    fn persist_auto_preference(&self, inner: &mut KeepAwakeInner) {
        let Some(path) = self.preference_path.as_deref() else {
            return;
        };
        let preference = PersistedKeepAwakePreference {
            auto_enabled: inner.auto_enabled,
            suppressed_until_activity_edge: inner.auto_suppressed_until_activity_edge,
            suppressed_active_agent_ids: if inner.auto_suppressed_until_activity_edge {
                inner.auto_active_agent_ids.clone()
            } else {
                Vec::new()
            },
        };
        match save_keep_awake_preference(path, &preference) {
            Ok(()) => inner.auto_preference_warning = None,
            Err(error) => {
                inner.auto_preference_warning = Some(format!(
                    "Auto keep awake is active for this session but its preference could not be saved: {error}"
                ));
            }
        }
    }

    fn emit_status_if_changed(&self, status: KeepAwakeStatus) -> Option<KeepAwakeStatus> {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if inner.last_emitted_status.as_ref() == Some(&status) {
            return None;
        }
        inner.last_emitted_status = Some(status.clone());
        Some(status)
    }

    fn status_subscription_due(&self, now: Instant) -> bool {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !inner.auto_enabled || !inner.daemon_connected {
            return false;
        }
        let due = inner
            .auto_last_subscription_assertion_at
            .is_none_or(|last| {
                now.saturating_duration_since(last) >= KEEP_AWAKE_SUBSCRIPTION_REASSERT_INTERVAL
            });
        if due {
            inner.auto_last_subscription_assertion_at = Some(now);
        }
        due
    }

    fn mark_status_subscription_asserted(&self, now: Instant) {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .auto_last_subscription_assertion_at = Some(now);
    }

    fn request_manual_hold(&self) -> KeepAwakeStatus {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        inner.manual_requested = true;
        reconcile_keep_awake_intent(&mut inner);
        sync_keep_awake_platform(&mut inner, Instant::now());
        keep_awake_status_for_platform(&inner, Instant::now())
    }

    fn stop(&self, suppress_auto: bool) -> Result<KeepAwakeStatus, String> {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        inner.manual_requested = false;
        if suppress_auto && inner.auto_enabled {
            inner.auto_suppressed_until_activity_edge = true;
            inner.auto_hold_requested = false;
            inner.auto_idle_observed = Duration::ZERO;
            inner.auto_last_idle_observation_at = None;
            self.persist_auto_preference(&mut inner);
        }
        reconcile_keep_awake_intent(&mut inner);
        let now = Instant::now();
        sync_keep_awake_platform(&mut inner, now);
        if !inner.armed {
            inner.warning = None;
            inner.notice = None;
            inner.respawn_count = 0;
            inner.last_loss_reason = None;
            if suppress_auto {
                inner.consecutive_spawn_failures = 0;
                inner.next_spawn_attempt_at = None;
            }
        }
        Ok(keep_awake_status_for_platform(&inner, now))
    }

    fn shutdown(&self) -> Result<KeepAwakeStatus, String> {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        inner.manual_requested = false;
        inner.auto_enabled = false;
        inner.auto_hold_requested = false;
        inner.armed = false;
        inner.arm_source = None;
        stop_keep_awake_child(&mut inner)?;
        Ok(keep_awake_status_for_platform(&inner, Instant::now()))
    }

    fn stop_silently(&self) {
        let _ = self.shutdown();
    }

    fn configure_auto(
        &self,
        enabled: bool,
        seed_suppressed_until_activity_edge: bool,
        seed_active_agent_ids: Vec<i64>,
    ) -> KeepAwakeStatus {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        configure_auto_keep_awake_state(
            &mut inner,
            enabled,
            seed_suppressed_until_activity_edge,
            seed_active_agent_ids,
        );
        let now = Instant::now();
        evaluate_auto_keep_awake_tick(
            &mut inner,
            now,
            KEEP_AWAKE_AUTO_SETTLE,
            keep_awake_max_snapshot_age(),
        );
        reconcile_keep_awake_intent(&mut inner);
        self.persist_auto_preference(&mut inner);
        sync_keep_awake_platform(&mut inner, now);
        keep_awake_status_for_platform(&inner, now)
    }

    fn observe_daemon_message(&self, message: &str) -> Option<KeepAwakeStatus> {
        let active_agent_ids = auto_keep_awake_active_agent_ids_from_message(message)?;
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let now = Instant::now();
        let was_suppressed = inner.auto_suppressed_until_activity_edge;
        observe_auto_keep_awake_snapshot(&mut inner, active_agent_ids, now);
        if was_suppressed != inner.auto_suppressed_until_activity_edge {
            self.persist_auto_preference(&mut inner);
        }
        sync_keep_awake_platform(&mut inner, now);
        Some(keep_awake_status_for_platform(&inner, now))
    }

    fn set_daemon_connected(&self, connected: bool) {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        inner.daemon_connected = connected;
        inner.auto_last_subscription_assertion_at = None;
        if !connected {
            inner.auto_observation_continuous = false;
            inner.auto_last_idle_observation_at = None;
        }
    }

    fn auto_enabled(&self) -> bool {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .auto_enabled
    }

    fn reconcile_tick(&self) -> KeepAwakeStatus {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let now = Instant::now();
        evaluate_auto_keep_awake_tick(
            &mut inner,
            now,
            KEEP_AWAKE_AUTO_SETTLE,
            keep_awake_max_snapshot_age(),
        );
        reconcile_keep_awake_intent(&mut inner);
        sync_keep_awake_platform(&mut inner, now);
        keep_awake_status_for_platform(&inner, now)
    }
}

impl Drop for KeepAwakeState {
    fn drop(&mut self) {
        if Arc::strong_count(&self.inner) == 1 {
            self.stop_silently();
        }
    }
}

enum BridgeCommand {
    Send(String),
    Restart(oneshot::Sender<Result<(), String>>),
    Park {
        stop_daemon: bool,
        reply: oneshot::Sender<Result<(), String>>,
    },
}

struct TerminalInput {
    process_id: i64,
    data: Vec<u8>,
}

#[derive(Clone, Serialize)]
struct ConnectionStatus {
    status: &'static str,
    message: Option<String>,
    port: Option<u16>,
    app_version: &'static str,
    app_build_id: &'static str,
    app_control_protocol_version: u32,
    daemon_version: Option<String>,
    daemon_build_id: Option<String>,
    daemon_control_protocol_version: Option<u32>,
    version_compatible: bool,
}

impl ConnectionStatus {
    fn connecting() -> Self {
        Self {
            status: "connecting",
            message: None,
            port: None,
            app_version: BUILD_VERSION,
            app_build_id: BUILD_ID,
            app_control_protocol_version: CONTROL_PROTOCOL_VERSION,
            daemon_version: None,
            daemon_build_id: None,
            daemon_control_protocol_version: None,
            version_compatible: false,
        }
    }

    fn connected(port: u16, daemon: Option<&DaemonVersion>) -> Self {
        Self {
            status: "connected",
            message: None,
            port: Some(port),
            app_version: BUILD_VERSION,
            app_build_id: BUILD_ID,
            app_control_protocol_version: CONTROL_PROTOCOL_VERSION,
            daemon_version: daemon.map(|version| version.version.clone()),
            daemon_build_id: daemon.map(|version| version.build_id.clone()),
            daemon_control_protocol_version: daemon.map(|version| version.control_protocol_version),
            version_compatible: daemon.is_some_and(DaemonVersion::matches_current_build),
        }
    }

    fn disconnected(message: impl Into<String>) -> Self {
        Self {
            status: "disconnected",
            message: Some(message.into()),
            ..Self::connecting()
        }
    }
}

#[derive(Debug, Deserialize)]
struct HelloResponse {
    id: Value,
    ok: bool,
    #[serde(default)]
    result: Option<DaemonVersion>,
}

#[derive(Clone, Serialize)]
struct DaemonRestartResult {
    restarting: bool,
}

#[derive(Clone, Serialize)]
struct DesktopRelaunchCapability {
    supported: bool,
    app_bundle: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ShellOpenTarget {
    Editor,
    Finder,
    Reveal,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ShellOpener {
    Detected { id: String },
    Custom { template: String },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct DetectedEditor {
    id: &'static str,
    label: &'static str,
    bundle_path: String,
}

#[derive(Clone, Copy)]
struct EditorCandidate {
    id: &'static str,
    label: &'static str,
    bundle_names: &'static [&'static str],
}

const EDITOR_CANDIDATES: &[EditorCandidate] = &[
    EditorCandidate {
        id: "vscode",
        label: "Visual Studio Code",
        bundle_names: &["Visual Studio Code.app"],
    },
    EditorCandidate {
        id: "cursor",
        label: "Cursor",
        bundle_names: &["Cursor.app"],
    },
    EditorCandidate {
        id: "zed",
        label: "Zed",
        bundle_names: &["Zed.app"],
    },
    EditorCandidate {
        id: "sublime",
        label: "Sublime Text",
        bundle_names: &["Sublime Text.app"],
    },
    EditorCandidate {
        id: "intellij",
        label: "IntelliJ IDEA",
        bundle_names: &["IntelliJ IDEA.app", "IntelliJ IDEA CE.app"],
    },
];

#[derive(Clone, Serialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
enum DaemonFrame {
    Text(String),
    Binary(Vec<u8>),
    Terminal(TerminalFrame),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum NativeMenuAction {
    About,
    Settings,
    CheckUpdates,
    PreviousView,
    ToggleProjectRail,
    ToggleSectionRail,
}

#[derive(Deserialize)]
struct DesktopMenuAccelerators {
    settings: Option<String>,
    previous_view: Option<String>,
    toggle_project_rail: Option<String>,
    toggle_section_rail: Option<String>,
}

#[derive(Clone, Serialize)]
struct TerminalFrame {
    process_id: i64,
    start_offset: u64,
    gap: bool,
    kitty_keyboard_flags: u8,
    modify_other_keys: u8,
    data: Vec<u8>,
}

#[tauri::command]
fn daemon_send(message: String, state: State<'_, BridgeState>) -> Result<(), String> {
    if message.len() > 1024 * 1024 {
        return Err("control message exceeds the 1 MiB limit".to_owned());
    }
    state
        .sender
        .try_send(BridgeCommand::Send(message))
        .map_err(|error| format!("daemon bridge is not accepting messages: {error}"))
}

/// Queue latency-sensitive PTY input separately from ordinary control traffic.
///
/// This command is intentionally synchronous: the webview only waits for a bounded in-memory
/// enqueue, never for a daemon write, response, timeout, reconnect, or resubscription.
#[tauri::command]
fn daemon_send_input(
    process_id: i64,
    data: Vec<u8>,
    state: State<'_, BridgeState>,
) -> Result<(), String> {
    if data.len() > 1024 * 1024 {
        return Err("terminal input exceeds the 1 MiB limit".to_owned());
    }
    state
        .input_sender
        .try_send(TerminalInput { process_id, data })
        .map_err(|error| format!("daemon input bridge is not accepting messages: {error}"))
}

#[tauri::command]
async fn daemon_restart(
    confirm_processes_stopped: bool,
    state: State<'_, BridgeState>,
) -> Result<DaemonRestartResult, String> {
    if !confirm_processes_stopped {
        return Err("restart requires confirmation that project processes will stop".to_owned());
    }
    let (reply, receive) = oneshot::channel();
    state
        .sender
        .send(BridgeCommand::Restart(reply))
        .await
        .map_err(|_| "daemon bridge is not running".to_owned())?;
    timeout(RESTART_TIMEOUT, receive)
        .await
        .map_err(|_| "timed out waiting for the daemon to stop".to_owned())?
        .map_err(|_| "daemon bridge dropped the restart request".to_owned())??;
    Ok(DaemonRestartResult { restarting: true })
}

#[tauri::command]
fn desktop_relaunch_capability() -> DesktopRelaunchCapability {
    let executable = env::current_exe().ok();
    let supported = executable
        .as_deref()
        .is_some_and(relaunch_supported_from_executable);
    let app_bundle = executable
        .as_deref()
        .and_then(application_bundle_from_executable)
        .and_then(|bundle| bundle.canonicalize().ok())
        .map(|bundle| bundle.to_string_lossy().into_owned());
    let supported = supported && app_bundle.is_some();
    DesktopRelaunchCapability {
        supported,
        app_bundle,
    }
}

/// Stop the replaced daemon before asking Tauri to reopen the replaced application bundle.
///
/// `refresh_application_bundle` swaps the bundle at the same path. Re-validate that the path in
/// the install report is this running app, then park the bridge before requesting restart. The
/// park acknowledgement is the readiness handshake: once it arrives, the old supervisor has no
/// code path that can reconnect or respawn a daemon while the process is exiting.
#[tauri::command]
async fn desktop_restart_after_update(
    confirm_processes_stopped: bool,
    restart_daemon: bool,
    installed_app_bundle: String,
    state: State<'_, BridgeState>,
    app: tauri::AppHandle,
) -> Result<DaemonRestartResult, String> {
    if !confirm_processes_stopped {
        return Err("update restart requires confirmation that project processes will stop".into());
    }
    verify_relaunch_bundle(Path::new(&installed_app_bundle))?;
    let (reply, receive) = oneshot::channel();
    state
        .sender
        .send(BridgeCommand::Park {
            stop_daemon: restart_daemon,
            reply,
        })
        .await
        .map_err(|_| "daemon bridge is not running".to_owned())?;
    timeout(RESTART_TIMEOUT, receive)
        .await
        .map_err(|_| "timed out waiting for the desktop bridge to park".to_owned())?
        .map_err(|_| "daemon bridge dropped the update restart request".to_owned())??;
    app.request_restart();
    Ok(DaemonRestartResult { restarting: true })
}

fn relaunch_supported_from_executable(executable: &Path) -> bool {
    #[cfg(target_os = "macos")]
    {
        let Some(bundle) = application_bundle_from_executable(executable) else {
            return false;
        };
        let path = bundle.to_string_lossy();
        !path.contains("/AppTranslocation/")
            && !path.starts_with("/private/var/folders/")
            && executable.is_file()
            && bundle.join("Contents/Info.plist").is_file()
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = executable;
        false
    }
}

fn application_bundle_from_executable(executable: &Path) -> Option<PathBuf> {
    let contents = executable.parent()?.parent()?;
    if contents.file_name().and_then(|name| name.to_str()) != Some("Contents") {
        return None;
    }
    let bundle = contents.parent()?;
    bundle
        .extension()
        .is_some_and(|extension| extension == "app")
        .then(|| bundle.to_path_buf())
}

fn verify_relaunch_bundle(expected_bundle: &Path) -> Result<(), String> {
    let executable = env::current_exe()
        .map_err(|error| format!("could not locate the running Workman executable: {error}"))?;
    if !relaunch_supported_from_executable(&executable) {
        return Err(
            "the running executable is not a relaunchable Workman application bundle".into(),
        );
    }
    let running_bundle = application_bundle_from_executable(&executable)
        .ok_or_else(|| "could not locate the running Workman application bundle".to_owned())?
        .canonicalize()
        .map_err(|error| format!("could not resolve the running Workman bundle: {error}"))?;
    let expected_bundle = expected_bundle
        .canonicalize()
        .map_err(|error| format!("could not resolve the replaced Workman bundle: {error}"))?;
    if running_bundle != expected_bundle {
        return Err(format!(
            "the update replaced {}, but this process is running from {}",
            expected_bundle.display(),
            running_bundle.display()
        ));
    }
    Ok(())
}

#[tauri::command]
fn daemon_status(state: State<'_, BridgeState>) -> ConnectionStatus {
    lock_status(&state.status).clone()
}

#[tauri::command]
fn keep_awake_start(state: State<'_, KeepAwakeState>) -> Result<KeepAwakeStatus, String> {
    Ok(state.request_manual_hold())
}

#[tauri::command]
fn keep_awake_stop(
    suppress_auto: bool,
    state: State<'_, KeepAwakeState>,
) -> Result<KeepAwakeStatus, String> {
    state.stop(suppress_auto)
}

#[tauri::command]
fn keep_awake_auto_configure(
    enabled: bool,
    seed_suppressed_until_activity_edge: bool,
    seed_active_agent_ids: Vec<i64>,
    state: State<'_, KeepAwakeState>,
    bridge: State<'_, BridgeState>,
) -> KeepAwakeStatus {
    let status = state.configure_auto(
        enabled,
        seed_suppressed_until_activity_edge,
        seed_active_agent_ids,
    );
    if enabled {
        request_keep_awake_status_subscription(&bridge);
    }
    status
}

fn request_keep_awake_status_subscription(bridge: &BridgeState) {
    let sender = bridge.sender.clone();
    let request = keep_awake_status_subscription_request();
    tauri::async_runtime::spawn(async move {
        let _ = sender.send(BridgeCommand::Send(request)).await;
    });
}

fn keep_awake_status_subscription_request() -> String {
    json!({
        "id": KEEP_AWAKE_STATUS_SUBSCRIBE_ID,
        "method": "process.status_subscribe",
        "params": {}
    })
    .to_string()
}

/// Read current intent/liveness and reap an exited owned child. Process repair is handled by the
/// native watchdog so a status probe never spawns a helper or depends on webview timer delivery.
#[tauri::command]
fn keep_awake_status(state: State<'_, KeepAwakeState>) -> KeepAwakeStatus {
    #[cfg(target_os = "macos")]
    {
        let mut inner = state
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        refresh_keep_awake_child(&mut inner);
        keep_awake_status_from(&inner, Instant::now())
    }

    #[cfg(not(target_os = "macos"))]
    {
        let inner = state
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        unsupported_keep_awake_status(&inner)
    }
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn keep_awake_command(pid: u32) -> Command {
    let mut command = Command::new("/usr/bin/caffeinate");
    command.args(["-i", "-w", &pid.to_string()]);
    command
}

fn keep_awake_status_from(inner: &KeepAwakeInner, now: Instant) -> KeepAwakeStatus {
    KeepAwakeStatus {
        supported: cfg!(target_os = "macos"),
        armed: inner.armed,
        active: inner.child.is_some(),
        arm_source: inner.arm_source,
        assertion_pid: inner.child.as_ref().map(Child::id),
        warning: inner.warning.clone(),
        notice: inner.notice.clone(),
        respawn_count: inner.respawn_count,
        last_loss_reason: inner.last_loss_reason.clone(),
        retry_in_ms: inner.next_spawn_attempt_at.map(|deadline| {
            u64::try_from(deadline.saturating_duration_since(now).as_millis()).unwrap_or(u64::MAX)
        }),
        auto_enabled: inner.auto_enabled,
        auto_should_hold: inner.auto_hold_requested,
        auto_suppressed_until_activity_edge: inner.auto_suppressed_until_activity_edge,
        auto_active_agent_ids: inner.auto_active_agent_ids.clone(),
        auto_snapshot_stale: inner.auto_snapshot_stale,
        auto_snapshot_max_age_ms: duration_millis(keep_awake_max_snapshot_age()),
        auto_preference_warning: inner.auto_preference_warning.clone(),
    }
}

#[cfg(not(target_os = "macos"))]
fn unsupported_keep_awake_status(inner: &KeepAwakeInner) -> KeepAwakeStatus {
    KeepAwakeStatus {
        supported: false,
        armed: false,
        active: false,
        arm_source: None,
        assertion_pid: None,
        warning: None,
        notice: None,
        respawn_count: 0,
        last_loss_reason: None,
        retry_in_ms: None,
        auto_enabled: inner.auto_enabled,
        auto_should_hold: false,
        auto_suppressed_until_activity_edge: inner.auto_suppressed_until_activity_edge,
        auto_active_agent_ids: inner.auto_active_agent_ids.clone(),
        auto_snapshot_stale: inner.auto_snapshot_stale,
        auto_snapshot_max_age_ms: duration_millis(keep_awake_max_snapshot_age()),
        auto_preference_warning: inner.auto_preference_warning.clone(),
    }
}

fn duration_millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

fn keep_awake_max_snapshot_age() -> Duration {
    #[cfg(debug_assertions)]
    if let Some(milliseconds) = env::var("WORKMAN_KEEP_AWAKE_MAX_SNAPSHOT_AGE_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value >= 100)
    {
        return Duration::from_millis(milliseconds);
    }
    KEEP_AWAKE_MAX_SNAPSHOT_AGE
}

fn load_keep_awake_preference(path: &Path) -> Result<PersistedKeepAwakePreference, String> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Default::default()),
        Err(error) => return Err(error.to_string()),
    };
    serde_json::from_slice(&bytes).map_err(|error| error.to_string())
}

fn save_keep_awake_preference(
    path: &Path,
    preference: &PersistedKeepAwakePreference,
) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("{} has no parent directory", path.display()))?;
    std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let temporary = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec(preference).map_err(|error| error.to_string())?;
    std::fs::write(&temporary, bytes).map_err(|error| error.to_string())?;
    if let Err(error) = std::fs::rename(&temporary, path) {
        let _ = std::fs::remove_file(&temporary);
        return Err(error.to_string());
    }
    Ok(())
}

fn keep_awake_status_for_platform(inner: &KeepAwakeInner, now: Instant) -> KeepAwakeStatus {
    #[cfg(target_os = "macos")]
    return keep_awake_status_from(inner, now);

    #[cfg(not(target_os = "macos"))]
    {
        let _ = now;
        unsupported_keep_awake_status(inner)
    }
}

fn sync_keep_awake_platform(inner: &mut KeepAwakeInner, now: Instant) {
    #[cfg(target_os = "macos")]
    sync_keep_awake_child_with(inner, now, || {
        keep_awake_command(std::process::id())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
    });

    #[cfg(not(target_os = "macos"))]
    {
        let _ = (inner, now);
    }
}

fn reconcile_keep_awake_intent(inner: &mut KeepAwakeInner) {
    let desired_source = if inner.manual_requested {
        Some(KeepAwakeArmSource::Manual)
    } else if inner.auto_enabled && inner.auto_hold_requested {
        Some(KeepAwakeArmSource::Auto)
    } else {
        None
    };
    match desired_source {
        Some(source) => {
            if !inner.armed {
                begin_keep_awake_session(inner);
            }
            inner.arm_source = Some(source);
        }
        None => {
            inner.armed = false;
            inner.arm_source = None;
        }
    }
}

fn configure_auto_keep_awake_state(
    inner: &mut KeepAwakeInner,
    enabled: bool,
    seed_suppressed_until_activity_edge: bool,
    seed_active_agent_ids: Vec<i64>,
) {
    let was_enabled = inner.auto_enabled;
    inner.auto_enabled = enabled;
    if enabled && !was_enabled {
        inner.auto_suppressed_until_activity_edge = seed_suppressed_until_activity_edge;
        if seed_suppressed_until_activity_edge {
            if inner.auto_active_agent_ids.is_empty() {
                inner.auto_active_agent_ids = normalized_agent_ids(seed_active_agent_ids);
            }
            // Enabling after launch/reload is a rebase, not a fabricated activity edge.
            inner.auto_observation_continuous = false;
        }
    } else if !enabled {
        inner.auto_suppressed_until_activity_edge = false;
        inner.auto_snapshot_stale = false;
    } else {
        // Once enabled, native observations are authoritative. The remaining arguments are
        // intentionally enable-edge seeds so a WebView reload cannot overwrite newer native
        // suppression or activity state.
    }
    if !enabled || inner.auto_suppressed_until_activity_edge {
        inner.auto_hold_requested = false;
        inner.auto_idle_observed = Duration::ZERO;
        inner.auto_last_idle_observation_at = None;
    } else if !inner.auto_active_agent_ids.is_empty() {
        inner.auto_hold_requested = true;
    }
    reconcile_keep_awake_intent(inner);
}

fn observe_auto_keep_awake_snapshot(
    inner: &mut KeepAwakeInner,
    active_agent_ids: Vec<i64>,
    now: Instant,
) {
    let active_agent_ids = normalized_agent_ids(active_agent_ids);
    let activity_edge = inner.auto_observation_continuous
        && active_agent_ids
            .iter()
            .any(|id| inner.auto_active_agent_ids.binary_search(id).is_err());
    inner.auto_active_agent_ids = active_agent_ids;
    inner.auto_observation_continuous = true;
    inner.auto_last_snapshot_at = Some(now);
    inner.auto_snapshot_stale = false;

    if activity_edge {
        inner.auto_suppressed_until_activity_edge = false;
    }
    if !inner.auto_enabled || inner.auto_suppressed_until_activity_edge {
        inner.auto_hold_requested = false;
        inner.auto_idle_observed = Duration::ZERO;
        inner.auto_last_idle_observation_at = None;
    } else if inner.auto_active_agent_ids.is_empty() {
        if inner.auto_hold_requested && inner.daemon_connected {
            inner.auto_last_idle_observation_at.get_or_insert(now);
        }
    } else {
        inner.auto_hold_requested = true;
        inner.auto_idle_observed = Duration::ZERO;
        inner.auto_last_idle_observation_at = None;
    }
    reconcile_keep_awake_intent(inner);
}

fn evaluate_auto_keep_awake_tick(
    inner: &mut KeepAwakeInner,
    now: Instant,
    settle: Duration,
    max_snapshot_age: Duration,
) {
    if !inner.auto_enabled {
        inner.auto_snapshot_stale = false;
        inner.auto_hold_requested = false;
        inner.auto_idle_observed = Duration::ZERO;
        inner.auto_last_idle_observation_at = None;
        return;
    }
    if inner
        .auto_last_snapshot_at
        .is_some_and(|snapshot| now.saturating_duration_since(snapshot) >= max_snapshot_age)
    {
        inner.auto_snapshot_stale = true;
        inner.auto_active_agent_ids.clear();
        inner.auto_observation_continuous = false;
        inner.auto_hold_requested = false;
        inner.auto_idle_observed = Duration::ZERO;
        inner.auto_last_idle_observation_at = None;
        return;
    }
    if inner.auto_suppressed_until_activity_edge {
        inner.auto_hold_requested = false;
        inner.auto_idle_observed = Duration::ZERO;
        inner.auto_last_idle_observation_at = None;
        return;
    }
    if !inner.auto_active_agent_ids.is_empty() {
        inner.auto_hold_requested = true;
        inner.auto_idle_observed = Duration::ZERO;
        inner.auto_last_idle_observation_at = None;
        return;
    }
    if !inner.auto_hold_requested || !inner.daemon_connected {
        inner.auto_last_idle_observation_at = None;
        return;
    }
    let delta = inner
        .auto_last_idle_observation_at
        .map(|previous| now.saturating_duration_since(previous))
        .unwrap_or_default()
        .min(KEEP_AWAKE_MAX_OBSERVATION_GAP);
    inner.auto_idle_observed = inner.auto_idle_observed.saturating_add(delta);
    inner.auto_last_idle_observation_at = Some(now);
    if inner.auto_idle_observed >= settle {
        inner.auto_hold_requested = false;
        inner.auto_idle_observed = Duration::ZERO;
        inner.auto_last_idle_observation_at = None;
    }
}

fn normalized_agent_ids(mut ids: Vec<i64>) -> Vec<i64> {
    ids.retain(|id| *id > 0);
    ids.sort_unstable();
    ids.dedup();
    ids
}

fn auto_keep_awake_active_agent_ids_from_message(message: &str) -> Option<Vec<i64>> {
    let value: Value = serde_json::from_str(message).ok()?;
    if value.get("event").and_then(Value::as_str) != Some("process.statuses") {
        return None;
    }
    let processes =
        serde_json::from_value::<Vec<AutoKeepAwakeProcess>>(value.get("processes")?.clone())
            .ok()?;
    Some(auto_keep_awake_active_agent_ids(&processes))
}

fn auto_keep_awake_active_agent_ids(processes: &[AutoKeepAwakeProcess]) -> Vec<i64> {
    normalized_agent_ids(
        processes
            .iter()
            .filter(|process| auto_keep_awake_process_should_hold(process))
            .map(|process| process.id)
            .collect(),
    )
}

fn auto_keep_awake_process_should_hold(process: &AutoKeepAwakeProcess) -> bool {
    if process.kind != "agent" || !matches!(process.status.as_str(), "starting" | "running") {
        return false;
    }
    let state = &process.agent_state;
    if state.working
        || state.needs_input
        || state.thinking
        || state.planning
        || matches!(state.state.as_str(), "working" | "needs_input")
    {
        return true;
    }
    if state.state == "waiting" {
        return state
            .waiting_on
            .iter()
            .any(|reason| !reason.paused && reason.max_wait_ms > 0 && reason.remaining_ms > 0);
    }
    process.status == "starting" && state.last_output_at.is_none()
}

fn begin_keep_awake_session(inner: &mut KeepAwakeInner) {
    inner.armed = true;
    inner.warning = None;
    inner.notice = None;
    inner.respawn_count = 0;
    inner.last_loss_reason = None;
}

fn sync_keep_awake_child_with(
    inner: &mut KeepAwakeInner,
    now: Instant,
    spawn: impl FnOnce() -> std::io::Result<Child>,
) {
    refresh_keep_awake_child(inner);
    if !inner.armed {
        if let Err(error) = stop_keep_awake_child(inner) {
            inner.warning = Some(error);
        }
        return;
    }
    if inner.child.is_some() {
        return;
    }
    if inner
        .next_spawn_attempt_at
        .is_some_and(|deadline| deadline > now)
    {
        return;
    }

    let prior_loss = inner.last_loss_reason.clone();
    let prior_failures = inner.consecutive_spawn_failures;
    match spawn() {
        Ok(child) => {
            inner.child = Some(child);
            inner.warning = None;
            inner.consecutive_spawn_failures = 0;
            inner.next_spawn_attempt_at = None;
            if let Some(reason) = prior_loss {
                inner.respawn_count = inner.respawn_count.saturating_add(1);
                inner.notice = Some(format!(
                    "Keep awake assertion restored {}× since arming; last loss: {reason}",
                    inner.respawn_count
                ));
            } else if prior_failures > 0 {
                inner.notice = Some(format!(
                    "Keep awake assertion started after {prior_failures} failed {}",
                    if prior_failures == 1 {
                        "attempt"
                    } else {
                        "attempts"
                    }
                ));
            }
        }
        Err(error) => {
            inner.consecutive_spawn_failures = inner.consecutive_spawn_failures.saturating_add(1);
            let retry_delay = keep_awake_retry_delay(inner.consecutive_spawn_failures);
            inner.next_spawn_attempt_at = now.checked_add(retry_delay);
            inner.warning = Some(format!(
                "Could not start macOS keep awake: {error}. Retrying in {}s.",
                retry_delay.as_secs()
            ));
        }
    }
}

fn keep_awake_retry_delay(consecutive_failures: u32) -> Duration {
    let exponent = consecutive_failures.saturating_sub(1).min(6);
    Duration::from_secs(1_u64 << exponent).min(KEEP_AWAKE_MAX_RETRY_DELAY)
}

fn refresh_keep_awake_child(inner: &mut KeepAwakeInner) {
    let Some(child) = inner.child.as_mut() else {
        return;
    };
    match child.try_wait() {
        Ok(None) => {}
        Ok(Some(status)) => {
            inner.child = None;
            let reason = format!("Keep awake helper exited unexpectedly ({status})");
            inner.last_loss_reason = Some(reason.clone());
            inner.warning = Some(reason);
            inner.next_spawn_attempt_at = None;
        }
        Err(error) => {
            if let Some(mut child) = inner.child.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
            let reason = format!("Could not inspect keep awake helper: {error}");
            inner.last_loss_reason = Some(reason.clone());
            inner.warning = Some(reason);
            inner.next_spawn_attempt_at = None;
        }
    }
}

#[cfg(target_os = "macos")]
async fn run_keep_awake_watchdog(
    app: tauri::AppHandle,
    state: KeepAwakeState,
    bridge: BridgeState,
) {
    let mut timer = interval(KEEP_AWAKE_WATCHDOG_INTERVAL);
    timer.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        timer.tick().await;
        let now = Instant::now();
        let status = state.reconcile_tick();
        if state.status_subscription_due(now) {
            request_keep_awake_status_subscription(&bridge);
        }
        if let Some(status) = state.emit_status_if_changed(status) {
            let _ = app.emit(KEEP_AWAKE_RESYNC_EVENT, status);
        }
    }
}

fn stop_keep_awake_child(inner: &mut KeepAwakeInner) -> Result<(), String> {
    let Some(mut child) = inner.child.take() else {
        return Ok(());
    };
    match child.try_wait() {
        Ok(Some(_)) => Ok(()),
        Ok(None) => {
            if let Err(kill_error) = child.kill() {
                if child.try_wait().is_ok_and(|status| status.is_some()) {
                    return Ok(());
                }
                inner.child = Some(child);
                return Err(format!("could not stop macOS keep awake: {kill_error}"));
            }
            child
                .wait()
                .map(|_| ())
                .map_err(|error| format!("could not reap macOS keep awake: {error}"))
        }
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            Err(format!(
                "could not inspect macOS keep awake before stopping: {error}"
            ))
        }
    }
}

/// Open an existing workspace path without invoking a command shell.
#[tauri::command]
fn shell_open_path(path: String, target: ShellOpenTarget) -> Result<(), String> {
    let path = canonical_shell_path(&path)?;
    open_shell_target(&path, target)
}

/// Open an HTTP(S) URL in the system browser without invoking a command shell.
#[tauri::command]
fn shell_open_url(url: String) -> Result<(), String> {
    let url = validated_browser_url(&url)?;
    open_in_browser(url)
}

/// List supported editors found in standard application directories.
#[tauri::command]
fn shell_detect_editors() -> Vec<DetectedEditor> {
    detect_editors_in(&standard_application_roots())
}

/// Open a workspace with a detected editor or an argv-style custom template.
#[tauri::command]
fn shell_open_with(path: String, opener: ShellOpener) -> Result<(), String> {
    let path = canonical_shell_path(&path)?;
    match opener {
        ShellOpener::Detected { id } => open_in_detected_editor(&path, &id),
        ShellOpener::Custom { template } => open_with_template(&path, &template),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let (sender, receiver) = mpsc::channel(256);
    let (input_sender, input_receiver) = mpsc::channel(1_024);
    let state = BridgeState {
        sender,
        input_sender,
        status: Arc::new(Mutex::new(ConnectionStatus::connecting())),
    };
    let task_state = state.clone();
    let watchdog_bridge_state = state.clone();
    let keep_awake_state =
        KeepAwakeState::persistent(default_data_dir().join(KEEP_AWAKE_PREFERENCE_FILE));
    let bridge_keep_awake_state = keep_awake_state.clone();

    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(external_navigation::plugin());
    #[cfg(target_os = "macos")]
    let builder = builder.plugin(tauri_nspanel::init());

    builder
        .manage(state)
        .manage(keep_awake_state)
        .manage(recorded_feedback::FeedbackState::default())
        .manage(native_notifications::NativeNotificationState::default())
        .menu(build_native_menu)
        .on_menu_event(|app, event| {
            if let Some(action) = native_menu_action(event.id().as_ref()) {
                let _ = app.emit(NATIVE_MENU_EVENT, action);
            }
        })
        .invoke_handler(tauri::generate_handler![
            daemon_send,
            daemon_send_input,
            daemon_restart,
            daemon_status,
            desktop_relaunch_capability,
            desktop_restart_after_update,
            desktop_set_menu_accelerators,
            keep_awake_start,
            keep_awake_stop,
            keep_awake_status,
            keep_awake_auto_configure,
            shell_open_path,
            shell_open_url,
            shell_detect_editors,
            shell_open_with,
            terminal_clipboard::terminal_read_clipboard,
            terminal_clipboard::terminal_read_attachment_image,
            terminal_clipboard::terminal_import_draft_image,
            terminal_clipboard::terminal_save_clipboard_image,
            terminal_clipboard::terminal_save_clipboard_png,
            terminal_clipboard::terminal_save_draft_image,
            terminal_clipboard::terminal_write_clipboard_image,
            terminal_clipboard::terminal_write_clipboard_text,
            native_notifications::native_notification_permission_state,
            native_notifications::native_notification_request_permission,
            native_notifications::native_notification_open_settings,
            native_notifications::native_notification_show,
            native_notifications::native_notification_dismiss,
            native_notifications::native_notification_set_badge,
            recorded_feedback::feedback_capability,
            recorded_feedback::dictation_preflight,
            recorded_feedback::dictation_install_model,
            recorded_feedback::dictation_start,
            recorded_feedback::dictation_finish,
            recorded_feedback::dictation_cancel,
            recorded_feedback::feedback_preflight,
            recorded_feedback::feedback_request_screen_access,
            recorded_feedback::feedback_install_model,
            recorded_feedback::feedback_start,
            recorded_feedback::feedback_status,
            recorded_feedback::feedback_audio_inputs,
            recorded_feedback::feedback_toggle_pause,
            recorded_feedback::feedback_toggle_mute,
            recorded_feedback::feedback_set_input_device,
            recorded_feedback::feedback_raise_toolbar,
            recorded_feedback::feedback_set_tool,
            recorded_feedback::feedback_record_stroke,
            recorded_feedback::feedback_undo,
            recorded_feedback::feedback_clear,
            recorded_feedback::feedback_begin_region,
            recorded_feedback::feedback_cancel_region,
            recorded_feedback::feedback_capture_snapshot,
            recorded_feedback::feedback_abort,
            recorded_feedback::feedback_finish,
            recorded_feedback::feedback_read_image
        ])
        .setup(move |app| {
            tauri::async_runtime::spawn(run_bridge(
                app.handle().clone(),
                task_state,
                bridge_keep_awake_state,
                receiver,
                input_receiver,
            ));
            #[cfg(target_os = "macos")]
            tauri::async_runtime::spawn(run_keep_awake_watchdog(
                app.handle().clone(),
                app.state::<KeepAwakeState>().inner().clone(),
                watchdog_bridge_state,
            ));
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("failed to build workman desktop")
        .run(|app, event| {
            let should_stop = matches!(&event, RunEvent::Exit | RunEvent::ExitRequested { .. })
                || matches!(
                    &event,
                    RunEvent::WindowEvent {
                        label,
                        event: WindowEvent::CloseRequested { .. },
                        ..
                    } if label == "main"
                );
            if should_stop {
                app.state::<KeepAwakeState>().stop_silently();
                app.state::<recorded_feedback::FeedbackState>()
                    .shutdown(app);
            }
        });
}

fn build_native_menu(app: &tauri::AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let about = MenuItemBuilder::with_id(MENU_ABOUT, "About Workman").build(app)?;
    let settings = MenuItemBuilder::with_id(MENU_SETTINGS, "Settings…")
        .accelerator("CmdOrCtrl+,")
        .build(app)?;
    let check_updates =
        MenuItemBuilder::with_id(MENU_CHECK_UPDATES, "Check for Updates…").build(app)?;
    let previous_view = MenuItemBuilder::with_id(MENU_PREVIOUS_VIEW, "Switch to Previous View")
        .accelerator("CmdOrCtrl+`")
        .build(app)?;
    let toggle_project_rail =
        MenuItemBuilder::with_id(MENU_TOGGLE_PROJECT_RAIL, "Toggle Project Rail")
            .accelerator("CmdOrCtrl+B")
            .build(app)?;
    let toggle_section_rail =
        MenuItemBuilder::with_id(MENU_TOGGLE_SECTION_RAIL, "Toggle Section Rail")
            .accelerator("CmdOrCtrl+Shift+B")
            .build(app)?;

    let app_menu = SubmenuBuilder::new(app, "Workman")
        .item(&about)
        .separator()
        .item(&settings)
        .item(&check_updates)
        .separator()
        .services()
        .separator()
        .hide_with_text("Hide Workman")
        .hide_others()
        .show_all()
        .separator()
        // The daemon is a separate durable process. Tauri's predefined Quit
        // closes only this desktop application and deliberately leaves workmand running.
        .quit_with_text("Quit Workman")
        .build()?;
    let edit_menu = SubmenuBuilder::new(app, "Edit")
        .undo()
        .redo()
        .separator()
        .cut()
        .copy()
        .paste()
        .select_all()
        .build()?;
    let view_menu = SubmenuBuilder::new(app, "View")
        .item(&previous_view)
        .separator()
        .item(&toggle_project_rail)
        .item(&toggle_section_rail)
        .separator()
        .fullscreen()
        .build()?;
    let window_menu = SubmenuBuilder::with_id(app, WINDOW_SUBMENU_ID, "Window")
        .minimize()
        .maximize_with_text("Zoom")
        .separator()
        .bring_all_to_front()
        .build()?;

    MenuBuilder::new(app)
        .items(&[&app_menu, &edit_menu, &view_menu, &window_menu])
        .build()
}

fn native_menu_action(id: &str) -> Option<NativeMenuAction> {
    match id {
        MENU_ABOUT => Some(NativeMenuAction::About),
        MENU_SETTINGS => Some(NativeMenuAction::Settings),
        MENU_CHECK_UPDATES => Some(NativeMenuAction::CheckUpdates),
        MENU_PREVIOUS_VIEW => Some(NativeMenuAction::PreviousView),
        MENU_TOGGLE_PROJECT_RAIL => Some(NativeMenuAction::ToggleProjectRail),
        MENU_TOGGLE_SECTION_RAIL => Some(NativeMenuAction::ToggleSectionRail),
        _ => None,
    }
}

#[tauri::command]
fn desktop_set_menu_accelerators(
    app: tauri::AppHandle,
    accelerators: DesktopMenuAccelerators,
) -> Result<(), String> {
    let menu = app
        .menu()
        .ok_or_else(|| "desktop menu is unavailable".to_string())?;
    for (id, accelerator) in [
        (MENU_SETTINGS, accelerators.settings),
        (MENU_PREVIOUS_VIEW, accelerators.previous_view),
        (MENU_TOGGLE_PROJECT_RAIL, accelerators.toggle_project_rail),
        (MENU_TOGGLE_SECTION_RAIL, accelerators.toggle_section_rail),
    ] {
        let updated = set_nested_menu_accelerator(
            &menu.items().map_err(|error| error.to_string())?,
            id,
            accelerator.as_deref(),
        )
        .map_err(|error| error.to_string())?;
        if !updated {
            return Err(format!("desktop menu item {id} is unavailable"));
        }
    }
    Ok(())
}

fn set_nested_menu_accelerator(
    items: &[MenuItemKind<tauri::Wry>],
    id: &str,
    accelerator: Option<&str>,
) -> tauri::Result<bool> {
    for item in items {
        if item.id().as_ref() == id
            && let Some(menu_item) = item.as_menuitem()
        {
            menu_item.set_accelerator(accelerator)?;
            return Ok(true);
        }
        if let Some(submenu) = item.as_submenu()
            && set_nested_menu_accelerator(&submenu.items()?, id, accelerator)?
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Detect the private daemon-process mode used when no standalone `workmand` sits beside the app.
pub fn embedded_daemon_data_dir(args: impl IntoIterator<Item = OsString>) -> Option<PathBuf> {
    let mut args = args.into_iter().skip(1);
    while let Some(argument) = args.next() {
        if argument == "--data-dir" {
            return args.next().map(PathBuf::from);
        }
    }
    None
}

/// Parse the argument fallback used by older macOS `open` versions without `--env` support.
pub fn launch_environment(
    args: impl IntoIterator<Item = OsString>,
) -> (Option<OsString>, Option<OsString>, Option<OsString>) {
    let mut args = args.into_iter().skip(1);
    let mut data_dir = None;
    let mut config = None;
    let mut daemon_bin = None;
    while let Some(argument) = args.next() {
        let destination = if argument == "--workman-data-dir" {
            Some(&mut data_dir)
        } else if argument == "--workman-config" {
            Some(&mut config)
        } else if argument == "--workman-daemon-bin" {
            Some(&mut daemon_bin)
        } else {
            None
        };
        if let Some(destination) = destination
            && let Some(value) = args.next()
        {
            *destination = Some(value);
        }
    }
    (data_dir, config, daemon_bin)
}

const NATIVE_VISUAL_QA_BUNDLE_PREFIX: &str = "com.workman.";

/// Fail closed before Tauri starts whenever a per-todo visual-QA bundle has lost its
/// isolated launch environment. Computer-use clients may transparently reopen a closed
/// macOS bundle through LaunchServices, so shell-only environment overrides are not enough.
pub fn enforce_native_visual_qa_isolation() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let executable = std::env::current_exe().map_err(|error| {
            format!("native visual QA isolation guard could not resolve the executable: {error}")
        })?;
        let Some(identifier) = macos_bundle_identifier_from_executable(&executable)? else {
            return Ok(());
        };
        validate_native_visual_qa_environment(
            &identifier,
            std::env::var_os("WORKMAN_DATA_DIR").as_deref(),
            std::env::var_os("WORKMAN_CONFIG").as_deref(),
            std::env::var_os("WORKMAN_DAEMON_BIN").as_deref(),
        )
    }

    #[cfg(not(target_os = "macos"))]
    Ok(())
}

#[cfg(target_os = "macos")]
fn macos_bundle_identifier_from_executable(executable: &Path) -> Result<Option<String>, String> {
    let Some(contents) = executable.parent().and_then(Path::parent) else {
        return Ok(None);
    };
    if contents.file_name().and_then(|name| name.to_str()) != Some("Contents") {
        return Ok(None);
    }
    let info_plist = contents.join("Info.plist");
    if !info_plist.is_file() {
        return Ok(None);
    }
    let value = plist::Value::from_file(&info_plist).map_err(|error| {
        format!(
            "native visual QA isolation guard could not read {}: {error}",
            info_plist.display()
        )
    })?;
    Ok(value
        .as_dictionary()
        .and_then(|dictionary| dictionary.get("CFBundleIdentifier"))
        .and_then(plist::Value::as_string)
        .map(str::to_owned))
}

fn native_visual_qa_identity(identifier: &str) -> Option<&str> {
    let identity = identifier.strip_prefix(NATIVE_VISUAL_QA_BUNDLE_PREFIX)?;
    let digits = identity
        .strip_prefix("todo")
        .or_else(|| identity.strip_prefix("fix"))?;
    (!digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())).then_some(identity)
}

fn validate_native_visual_qa_environment(
    identifier: &str,
    data_dir: Option<&std::ffi::OsStr>,
    config: Option<&std::ffi::OsStr>,
    daemon_bin: Option<&std::ffi::OsStr>,
) -> Result<(), String> {
    let Some(suffix) = identifier.strip_prefix(NATIVE_VISUAL_QA_BUNDLE_PREFIX) else {
        return Ok(());
    };
    if !suffix.starts_with("todo") && !suffix.starts_with("fix") {
        return Ok(());
    }
    let identity = native_visual_qa_identity(identifier).ok_or_else(|| {
        native_visual_qa_error(identifier, "QA bundle identity must be todoNNN or fixNNN")
    })?;
    let data_dir =
        required_native_visual_qa_path(identifier, "WORKMAN_DATA_DIR", data_dir, identity)?;
    let config = required_native_visual_qa_path(identifier, "WORKMAN_CONFIG", config, identity)?;
    if data_dir == config {
        return Err(native_visual_qa_error(
            identifier,
            "WORKMAN_DATA_DIR and WORKMAN_CONFIG resolve to the same path",
        ));
    }
    if let Some(daemon_bin) = daemon_bin {
        let daemon_bin = Path::new(daemon_bin);
        if !daemon_bin.is_absolute() {
            return Err(native_visual_qa_error(
                identifier,
                "WORKMAN_DAEMON_BIN must be an absolute path when set",
            ));
        }
    }
    Ok(())
}

fn required_native_visual_qa_path(
    identifier: &str,
    name: &str,
    value: Option<&std::ffi::OsStr>,
    token: &str,
) -> Result<PathBuf, String> {
    let value =
        value.ok_or_else(|| native_visual_qa_error(identifier, &format!("{name} is missing")))?;
    let path = Path::new(value);
    if !path.is_absolute() {
        return Err(native_visual_qa_error(
            identifier,
            &format!(
                "{name} must be an absolute isolated path under /tmp containing {token}; got {}",
                path.display()
            ),
        ));
    }
    let canonical = std::fs::canonicalize(path).map_err(|error| {
        native_visual_qa_error(
            identifier,
            &format!("{name} must already exist and resolve safely: {error}"),
        )
    })?;
    let under_tmp = canonical.starts_with("/tmp") || canonical.starts_with("/private/tmp");
    if !under_tmp || !canonical.to_string_lossy().contains(token) {
        return Err(native_visual_qa_error(
            identifier,
            &format!(
                "{name} must resolve to an isolated path under /tmp containing {token}; got {}",
                canonical.display()
            ),
        ));
    }
    Ok(canonical)
}

fn native_visual_qa_error(identifier: &str, reason: &str) -> String {
    format!(
        "native visual QA isolation guard refused {identifier}: {reason}. Relaunch with scripts/native-visual-qa.sh so LaunchServices preserves the isolated contract"
    )
}

/// Run the loopback daemon from the desktop executable as a headless child-process fallback.
pub fn run_embedded_daemon(data_dir: PathBuf) -> Result<(), Box<dyn Error>> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async move {
        let server = DaemonServer::bind(DaemonConfig { data_dir, port: 0 }).await?;
        server.serve_until(embedded_shutdown_signal()).await
    })?;
    Ok(())
}

async fn run_bridge(
    app: tauri::AppHandle,
    state: BridgeState,
    keep_awake: KeepAwakeState,
    mut receiver: mpsc::Receiver<BridgeCommand>,
    mut input_receiver: mpsc::Receiver<TerminalInput>,
) {
    let mut reconnect_delay = Duration::from_millis(250);
    let mut pending_input = VecDeque::<TerminalInput>::new();
    loop {
        publish_status(&app, &state, ConnectionStatus::connecting());
        match connect_daemon().await {
            Ok((discovery, mut socket)) => {
                reconnect_delay = Duration::from_millis(250);
                let daemon_version = negotiate_daemon_version(&mut socket).await;
                log_daemon_version(daemon_version.as_ref());
                keep_awake.set_daemon_connected(true);
                if keep_awake.auto_enabled()
                    && !send_bridge_message(
                        &mut socket,
                        Message::Text(keep_awake_status_subscription_request().into()),
                    )
                    .await
                {
                    keep_awake.set_daemon_connected(false);
                    publish_status(
                        &app,
                        &state,
                        ConnectionStatus::disconnected(
                            "Daemon stopped accepting keep-awake status subscription; reconnecting",
                        ),
                    );
                    continue;
                }
                if keep_awake.auto_enabled() {
                    keep_awake.mark_status_subscription_asserted(Instant::now());
                }
                publish_status(
                    &app,
                    &state,
                    ConnectionStatus::connected(discovery.port, daemon_version.as_ref()),
                );

                let mut disconnect_message = "Daemon connection closed; retrying".to_owned();
                let mut last_incoming = Instant::now();
                let mut heartbeat = interval(BRIDGE_HEARTBEAT_INTERVAL);
                heartbeat.set_missed_tick_behavior(MissedTickBehavior::Skip);
                heartbeat.tick().await;
                loop {
                    if let Some(input) = pending_input.pop_front() {
                        let frame = encode_terminal_input(&input);
                        if !send_bridge_message(&mut socket, Message::Binary(frame.into())).await {
                            pending_input.push_front(input);
                            disconnect_message =
                                "Daemon stopped accepting terminal input; reconnecting".to_owned();
                            break;
                        }
                        continue;
                    }
                    tokio::select! {
                        incoming = socket.next() => {
                            let Some(incoming) = incoming else { break };
                            last_incoming = Instant::now();
                            match incoming {
                                Ok(Message::Text(text)) => {
                                    let text = text.to_string();
                                    if let Some(status) = keep_awake.observe_daemon_message(&text)
                                        && let Some(status) = keep_awake.emit_status_if_changed(status)
                                    {
                                        let _ = app.emit(KEEP_AWAKE_RESYNC_EVENT, status);
                                    }
                                    let _ = app.emit(MESSAGE_EVENT, DaemonFrame::Text(text));
                                }
                                Ok(Message::Binary(bytes)) => {
                                    let frame = parse_terminal_frame(&bytes)
                                        .map(DaemonFrame::Terminal)
                                        .unwrap_or_else(|| DaemonFrame::Binary(bytes.to_vec()));
                                    let _ = app.emit(MESSAGE_EVENT, frame);
                                }
                                Ok(Message::Close(_)) | Err(_) => break,
                                Ok(Message::Ping(bytes)) => {
                                    if !send_bridge_message(&mut socket, Message::Pong(bytes)).await {
                                        break;
                                    }
                                }
                                Ok(Message::Pong(_) | Message::Frame(_)) => {}
                            }
                        }
                        outgoing = input_receiver.recv() => {
                            let Some(input) = outgoing else { return };
                            let frame = encode_terminal_input(&input);
                            if !send_bridge_message(&mut socket, Message::Binary(frame.into())).await {
                                pending_input.push_back(input);
                                disconnect_message = "Daemon stopped accepting terminal input; reconnecting".to_owned();
                                break;
                            }
                        }
                        outgoing = receiver.recv() => {
                            let Some(outgoing) = outgoing else { return };
                            match outgoing {
                                BridgeCommand::Send(message) => {
                                    if !send_bridge_message(&mut socket, Message::Text(message.into())).await {
                                        disconnect_message = "Daemon stopped accepting control traffic; reconnecting".to_owned();
                                        break;
                                    }
                                }
                                BridgeCommand::Restart(reply) => {
                                    let result = stop_discovered_daemon(&discovery).await;
                                    let restarting = result.is_ok();
                                    let _ = reply.send(result);
                                    if restarting {
                                        break;
                                    }
                                }
                                BridgeCommand::Park { stop_daemon, reply } => {
                                    let result = if stop_daemon {
                                        stop_discovered_daemon(&discovery).await
                                    } else {
                                        Ok(())
                                    };
                                    let parked = result.is_ok();
                                    let _ = reply.send(result);
                                    if parked {
                                        return;
                                    }
                                }
                            }
                        }
                        _ = heartbeat.tick() => {
                            if last_incoming.elapsed() >= BRIDGE_HEARTBEAT_TIMEOUT {
                                disconnect_message = "Daemon is unresponsive; reconnecting".to_owned();
                                break;
                            }
                            if !send_bridge_message(&mut socket, Message::Ping(Vec::new().into())).await {
                                disconnect_message = "Daemon heartbeat failed; reconnecting".to_owned();
                                break;
                            }
                        }
                    }
                }
                keep_awake.set_daemon_connected(false);
                publish_status(
                    &app,
                    &state,
                    ConnectionStatus::disconnected(disconnect_message),
                );
            }
            Err(error) => {
                keep_awake.set_daemon_connected(false);
                publish_status(&app, &state, ConnectionStatus::disconnected(error));
            }
        }
        tokio::time::sleep(reconnect_delay).await;
        reconnect_delay = (reconnect_delay * 2).min(Duration::from_secs(3));
    }
}

async fn send_bridge_message(
    socket: &mut WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
    message: Message,
) -> bool {
    timeout(BRIDGE_WRITE_TIMEOUT, socket.send(message))
        .await
        .is_ok_and(|result| result.is_ok())
}

async fn negotiate_daemon_version(
    socket: &mut WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
) -> Option<DaemonVersion> {
    socket
        .send(Message::Text(
            json!({ "id": HELLO_REQUEST_ID, "method": "daemon.hello", "params": {} })
                .to_string()
                .into(),
        ))
        .await
        .ok()?;

    timeout(HELLO_TIMEOUT, async {
        loop {
            match socket.next().await? {
                Ok(Message::Text(text)) => {
                    let response = serde_json::from_str::<HelloResponse>(&text).ok()?;
                    if response.id == HELLO_REQUEST_ID {
                        return response.ok.then_some(response.result).flatten();
                    }
                }
                Ok(Message::Ping(bytes)) => {
                    socket.send(Message::Pong(bytes)).await.ok()?;
                }
                Ok(Message::Close(_)) | Err(_) => return None,
                Ok(_) => {}
            }
        }
    })
    .await
    .ok()
    .flatten()
}

fn log_daemon_version(daemon: Option<&DaemonVersion>) {
    if let Some(daemon) = daemon {
        eprintln!(
            "workman desktop: connected to daemon v{} (build {}, control protocol {})",
            daemon.version, daemon.build_id, daemon.control_protocol_version
        );
    } else {
        eprintln!("workman desktop: connected to a legacy daemon without a version handshake");
    }
}

async fn stop_discovered_daemon(discovery: &Discovery) -> Result<(), String> {
    if discovery.pid <= 1 || discovery.pid == std::process::id() {
        return Err("refusing to signal an invalid daemon process".to_owned());
    }
    let current = Discovery::read(default_data_dir())
        .map_err(|error| format!("could not verify daemon discovery: {error}"))?;
    if current.pid != discovery.pid || current.token != discovery.token {
        return Err("daemon discovery changed before restart; reconnect and try again".to_owned());
    }

    let status = Command::new("kill")
        .arg("-TERM")
        .arg(discovery.pid.to_string())
        .status()
        .map_err(|error| format!("could not signal daemon {}: {error}", discovery.pid))?;
    if !status.success() {
        return Err(format!(
            "could not gracefully stop daemon {}: {status}",
            discovery.pid
        ));
    }

    let deadline = Instant::now() + Duration::from_secs(5);
    while probe(discovery).await {
        if Instant::now() >= deadline {
            return Err(format!(
                "daemon {} did not stop within 5 seconds",
                discovery.pid
            ));
        }
        sleep(Duration::from_millis(50)).await;
    }
    Ok(())
}

async fn connect_daemon() -> Result<
    (
        Discovery,
        WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
    ),
    String,
> {
    let executable = daemon_executable().map_err(|error| error.to_string())?;
    let discovery = discover_or_spawn(default_data_dir(), executable, Duration::from_secs(5))
        .await
        .map_err(|error| error.to_string())?;
    let mut request = format!("ws://127.0.0.1:{}/ws", discovery.port)
        .into_client_request()
        .map_err(|error| error.to_string())?;
    request.headers_mut().insert(
        "Authorization",
        format!("Bearer {}", discovery.token)
            .parse()
            .map_err(|error| format!("invalid daemon token: {error}"))?,
    );
    let (socket, _) = timeout(BRIDGE_CONNECT_TIMEOUT, connect_async(request))
        .await
        .map_err(|_| "timed out connecting to the daemon".to_owned())?
        .map_err(|error| error.to_string())?;
    Ok((discovery, socket))
}

fn publish_status(app: &tauri::AppHandle, state: &BridgeState, status: ConnectionStatus) {
    *lock_status(&state.status) = status.clone();
    let _ = app.emit(STATUS_EVENT, status);
}

fn parse_terminal_frame(bytes: &[u8]) -> Option<TerminalFrame> {
    if bytes.len() < TERMINAL_FRAME_HEADER_LEN || &bytes[..4] != TERMINAL_FRAME_MAGIC {
        return None;
    }
    let process_id = i64::from_be_bytes(bytes[4..12].try_into().ok()?);
    let start_offset = u64::from_be_bytes(bytes[12..20].try_into().ok()?);
    let flags = bytes[20];
    Some(TerminalFrame {
        process_id,
        start_offset,
        gap: flags & 1 == 1,
        kitty_keyboard_flags: (flags >> 1) & 1,
        modify_other_keys: (flags >> 2) & 3,
        data: bytes[TERMINAL_FRAME_HEADER_LEN..].to_vec(),
    })
}

fn encode_terminal_input(input: &TerminalInput) -> Vec<u8> {
    let mut frame = Vec::with_capacity(TERMINAL_INPUT_HEADER_LEN + input.data.len());
    frame.extend_from_slice(TERMINAL_INPUT_MAGIC);
    frame.extend_from_slice(&input.process_id.to_be_bytes());
    frame.extend_from_slice(&input.data);
    frame
}

fn lock_status(status: &Mutex<ConnectionStatus>) -> std::sync::MutexGuard<'_, ConnectionStatus> {
    status
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn canonical_shell_path(path: &str) -> Result<PathBuf, String> {
    if path.trim().is_empty() {
        return Err("path cannot be empty".to_owned());
    }
    std::fs::canonicalize(path)
        .map_err(|error| format!("could not open workspace path {path:?}: {error}"))
}

fn validated_browser_url(url: &str) -> Result<&str, String> {
    if url.trim() != url || url.chars().any(char::is_control) {
        return Err(
            "browser URL cannot contain surrounding whitespace or control characters".to_owned(),
        );
    }
    let parsed = tauri::Url::parse(url).map_err(|_| "browser URL is invalid".to_owned())?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("browser URL must use http or https".to_owned());
    }
    Ok(url)
}

fn open_in_browser(url: &str) -> Result<(), String> {
    #[cfg(debug_assertions)]
    if let Some(capture_path) = env::var_os("WORKMAN_BROWSER_OPEN_CAPTURE") {
        append_browser_open_capture(Path::new(&capture_path), url)?;
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    return spawn_detached(Command::new("/usr/bin/open").arg(url), "default browser");

    #[cfg(target_os = "linux")]
    return spawn_detached(Command::new("xdg-open").arg(url), "default browser");

    #[cfg(target_os = "windows")]
    return spawn_detached(Command::new("explorer").arg(url), "default browser");

    #[allow(unreachable_code)]
    Err("opening a browser is not supported on this platform".to_owned())
}

#[cfg(debug_assertions)]
fn append_browser_open_capture(path: &Path, url: &str) -> Result<(), String> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| format!("browser-open capture is unavailable: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("browser-open capture must be a pre-existing regular file".to_owned());
    }
    let canonical = std::fs::canonicalize(path)
        .map_err(|error| format!("browser-open capture path is invalid: {error}"))?;
    let temp_root = std::fs::canonicalize("/tmp")
        .map_err(|error| format!("could not resolve the isolated QA root: {error}"))?;
    let parent = canonical
        .parent()
        .ok_or_else(|| "browser-open capture has no parent directory".to_owned())?;
    let root_name = parent
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "browser-open capture has an invalid QA root".to_owned())?;
    let Some((todo_id, nonce)) = root_name
        .strip_prefix("workman-todo")
        .and_then(|suffix| suffix.split_once("-qa."))
    else {
        return Err("browser-open capture must be inside a per-todo QA root".to_owned());
    };
    if parent.parent() != Some(temp_root.as_path())
        || todo_id.is_empty()
        || !todo_id.bytes().all(|byte| byte.is_ascii_digit())
        || nonce.is_empty()
        || canonical.file_name().and_then(|name| name.to_str()) != Some("browser-open.log")
    {
        return Err("browser-open capture must be inside a per-todo QA root".to_owned());
    }

    let mut capture = OpenOptions::new()
        .append(true)
        .open(&canonical)
        .map_err(|error| format!("could not open browser-open capture: {error}"))?;
    writeln!(capture, "{url}")
        .map_err(|error| format!("could not record browser-open dispatch: {error}"))
}

fn open_shell_target(path: &Path, target: ShellOpenTarget) -> Result<(), String> {
    match target {
        ShellOpenTarget::Editor => open_in_editor(path),
        ShellOpenTarget::Finder => open_in_file_manager(path, false),
        ShellOpenTarget::Reveal => open_in_file_manager(path, true),
    }
}

fn open_in_editor(path: &Path) -> Result<(), String> {
    if let Some(editor) = env::var_os("WORKMAN_EDITOR")
        .or_else(|| env::var_os("VISUAL"))
        .or_else(|| env::var_os("EDITOR"))
        .filter(|editor| !editor.is_empty())
    {
        return spawn_detached(Command::new(editor).arg(path), "editor");
    }

    for candidate in ["code", "cursor", "zed"] {
        match spawn_detached(Command::new(candidate).arg(path), candidate) {
            Ok(()) => return Ok(()),
            Err(error) if error.contains("No such file") || error.contains("not found") => {}
            Err(error) => return Err(error),
        }
    }

    #[cfg(target_os = "macos")]
    return spawn_detached(
        Command::new("open")
            .args(["-a", "Visual Studio Code"])
            .arg(path),
        "Visual Studio Code",
    );

    #[cfg(target_os = "linux")]
    return spawn_detached(Command::new("xdg-open").arg(path), "default desktop editor");

    #[cfg(target_os = "windows")]
    return spawn_detached(Command::new("explorer").arg(path), "Explorer");

    #[allow(unreachable_code)]
    Err("opening an editor is not supported on this platform".to_owned())
}

fn standard_application_roots() -> Vec<PathBuf> {
    let mut roots = vec![PathBuf::from("/Applications")];
    if let Some(home) = env::var_os("HOME").filter(|value| !value.is_empty()) {
        roots.push(PathBuf::from(home).join("Applications"));
    }
    roots
}

fn detect_editors_in(roots: &[PathBuf]) -> Vec<DetectedEditor> {
    EDITOR_CANDIDATES
        .iter()
        .filter_map(|candidate| {
            roots.iter().find_map(|root| {
                candidate.bundle_names.iter().find_map(|bundle_name| {
                    let bundle_path = root.join(bundle_name);
                    bundle_path.is_dir().then(|| DetectedEditor {
                        id: candidate.id,
                        label: candidate.label,
                        bundle_path: bundle_path.to_string_lossy().into_owned(),
                    })
                })
            })
        })
        .collect()
}

fn open_in_detected_editor(path: &Path, id: &str) -> Result<(), String> {
    let editor = shell_detect_editors()
        .into_iter()
        .find(|editor| editor.id == id)
        .ok_or_else(|| format!("configured editor {id:?} is not installed"))?;

    #[cfg(not(target_os = "macos"))]
    let _ = path;

    #[cfg(target_os = "macos")]
    return spawn_detached(
        Command::new("open")
            .args(["-a", editor.bundle_path.as_str()])
            .arg(path),
        editor.label,
    );

    #[allow(unreachable_code)]
    Err(format!(
        "opening {} is not supported on this platform",
        editor.label
    ))
}

fn open_with_template(path: &Path, template: &str) -> Result<(), String> {
    let mut arguments = parse_command_template(template, path)?;
    let executable = arguments.remove(0);
    spawn_detached(Command::new(&executable).args(arguments), &executable)
}

/// Parse a small argv syntax without ever involving a command shell.
fn parse_command_template(template: &str, path: &Path) -> Result<Vec<String>, String> {
    if template.len() > 4096 {
        return Err("custom command template is too long".to_owned());
    }
    if template.contains('\0') {
        return Err("custom command template may not contain NUL bytes".to_owned());
    }

    let mut arguments = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut escaping = false;
    let mut token_started = false;

    for character in template.chars() {
        if escaping {
            current.push(character);
            token_started = true;
            escaping = false;
            continue;
        }
        if character == '\\' {
            escaping = true;
            token_started = true;
            continue;
        }
        if let Some(active_quote) = quote {
            if character == active_quote {
                quote = None;
            } else {
                current.push(character);
            }
            token_started = true;
            continue;
        }
        if character == '\'' || character == '"' {
            quote = Some(character);
            token_started = true;
        } else if character.is_whitespace() {
            if token_started {
                arguments.push(std::mem::take(&mut current));
                token_started = false;
            }
        } else {
            current.push(character);
            token_started = true;
        }
    }

    if escaping {
        return Err("custom command template ends with an incomplete escape".to_owned());
    }
    if quote.is_some() {
        return Err("custom command template has an unterminated quote".to_owned());
    }
    if token_started {
        arguments.push(current);
    }
    if arguments.is_empty() || arguments[0].is_empty() {
        return Err("custom command template needs an executable".to_owned());
    }
    if arguments.len() > 64 {
        return Err("custom command template has too many arguments".to_owned());
    }
    if arguments[0].contains("{path}") {
        return Err("{path} cannot be used as the command executable".to_owned());
    }
    if !arguments.iter().any(|argument| argument.contains("{path}")) {
        return Err("custom command template must include {path}".to_owned());
    }

    let path = path.to_string_lossy();
    Ok(arguments
        .into_iter()
        .map(|argument| argument.replace("{path}", &path))
        .collect())
}

fn open_in_file_manager(path: &Path, reveal: bool) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let mut command = Command::new("open");
        if reveal || path.is_file() {
            command.arg("-R");
        }
        return spawn_detached(command.arg(path), "Finder");
    }

    #[cfg(target_os = "linux")]
    {
        let target = if reveal && path.is_file() {
            path.parent().unwrap_or(path)
        } else {
            path
        };
        return spawn_detached(Command::new("xdg-open").arg(target), "file manager");
    }

    #[cfg(target_os = "windows")]
    {
        let mut command = Command::new("explorer");
        if reveal || path.is_file() {
            command.arg(format!("/select,{}", path.display()));
        } else {
            command.arg(path);
        }
        return spawn_detached(&mut command, "Explorer");
    }

    #[allow(unreachable_code)]
    Err("opening a file manager is not supported on this platform".to_owned())
}

fn spawn_detached(command: &mut Command, label: &str) -> Result<(), String> {
    let environment = UserEnvironmentResolver::new(user_config_path())
        .resolve()
        .command_environment();
    command
        .env_clear()
        .envs(environment)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("could not open {label}: {error}"))
}

fn daemon_executable() -> io::Result<PathBuf> {
    let current = env::current_exe()?;
    let sibling = current.with_file_name(format!("workmand{}", env::consts::EXE_SUFFIX));
    Ok(daemon_executable_from(
        &current,
        env::var_os("WORKMAN_DAEMON_BIN").map(PathBuf::from),
        sibling.is_file(),
    ))
}

fn daemon_executable_from(
    current: &Path,
    override_path: Option<PathBuf>,
    sibling_available: bool,
) -> PathBuf {
    override_path.unwrap_or_else(|| {
        if sibling_available {
            current.with_file_name(format!("workmand{}", env::consts::EXE_SUFFIX))
        } else {
            current.to_path_buf()
        }
    })
}

#[cfg(unix)]
async fn embedded_shutdown_signal() {
    let mut terminate =
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).ok();
    if let Some(terminate) = terminate.as_mut() {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = terminate.recv() => {}
        }
    } else {
        let _ = tokio::signal::ctrl_c().await;
    }
}

#[cfg(windows)]
async fn embedded_shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "macos")]
    #[test]
    fn relaunch_requires_an_application_bundle_executable() {
        let root = tempfile::tempdir().unwrap();
        let bundle = root.path().join("Workman.app");
        let executable = bundle.join("Contents/MacOS/workman");
        std::fs::create_dir_all(executable.parent().unwrap()).unwrap();
        std::fs::write(&executable, b"fixture").unwrap();
        std::fs::write(bundle.join("Contents/Info.plist"), b"fixture").unwrap();
        assert!(relaunch_supported_from_executable(&executable));
        assert!(!relaunch_supported_from_executable(Path::new(
            "/tmp/target/debug/workman"
        )));
    }

    #[test]
    fn keep_awake_command_prevents_idle_sleep_until_desktop_exits() {
        let command = keep_awake_command(42_424);
        assert_eq!(command.get_program(), "/usr/bin/caffeinate");
        assert_eq!(
            command
                .get_args()
                .map(|argument| argument.to_string_lossy().into_owned())
                .collect::<Vec<_>>(),
            ["-i", "-w", "42424"]
        );
    }

    #[test]
    fn keep_awake_state_starts_inactive_without_a_warning() {
        let inner = KeepAwakeInner::default();
        let now = Instant::now();
        assert_eq!(
            keep_awake_status_from(&inner, now),
            KeepAwakeStatus {
                supported: cfg!(target_os = "macos"),
                armed: false,
                active: false,
                arm_source: None,
                assertion_pid: None,
                warning: None,
                notice: None,
                respawn_count: 0,
                last_loss_reason: None,
                retry_in_ms: None,
                auto_enabled: false,
                auto_should_hold: false,
                auto_suppressed_until_activity_edge: false,
                auto_active_agent_ids: vec![],
                auto_snapshot_stale: false,
                auto_snapshot_max_age_ms: duration_millis(keep_awake_max_snapshot_age()),
                auto_preference_warning: None,
            }
        );
    }

    #[test]
    fn auto_should_hold_uses_current_agent_state_and_live_unpaused_waits() {
        let message = json!({
            "event": "process.statuses",
            "processes": [
                {
                    "id": 1,
                    "kind": "agent",
                    "status": "running",
                    "agent_state": { "state": "working", "last_output_at": 1 }
                },
                {
                    "id": 2,
                    "kind": "agent",
                    "status": "running",
                    "agent_state": {
                        "state": "waiting",
                        "last_output_at": 1,
                        "waiting_on": [{
                            "max_wait_ms": 30_000,
                            "remaining_ms": 20_000,
                            "paused": false
                        }]
                    }
                },
                {
                    "id": 3,
                    "kind": "agent",
                    "status": "running",
                    "agent_state": {
                        "state": "waiting",
                        "last_output_at": 1,
                        "waiting_on": [{
                            "max_wait_ms": 30_000,
                            "remaining_ms": 20_000,
                            "paused": true
                        }]
                    }
                },
                {
                    "id": 4,
                    "kind": "agent",
                    "status": "running",
                    "agent_state": { "state": "idle", "last_output_at": 1 }
                }
            ]
        })
        .to_string();

        assert_eq!(
            auto_keep_awake_active_agent_ids_from_message(&message),
            Some(vec![1, 2])
        );
    }

    #[test]
    fn auto_arms_from_startup_snapshot_without_an_activity_transition() {
        let mut inner = KeepAwakeInner {
            daemon_connected: true,
            ..KeepAwakeInner::default()
        };
        configure_auto_keep_awake_state(&mut inner, true, false, vec![]);
        assert!(!inner.armed);

        observe_auto_keep_awake_snapshot(&mut inner, vec![7], Instant::now());

        assert!(inner.armed);
        assert!(inner.auto_hold_requested);
        assert_eq!(inner.arm_source, Some(KeepAwakeArmSource::Auto));
    }

    #[test]
    fn enabling_auto_arms_from_an_already_active_snapshot() {
        let mut inner = KeepAwakeInner {
            daemon_connected: true,
            ..KeepAwakeInner::default()
        };
        observe_auto_keep_awake_snapshot(&mut inner, vec![11], Instant::now());
        assert!(!inner.armed);

        configure_auto_keep_awake_state(&mut inner, true, false, vec![]);

        assert!(inner.armed);
        assert_eq!(inner.auto_active_agent_ids, vec![11]);
        assert_eq!(inner.arm_source, Some(KeepAwakeArmSource::Auto));
    }

    #[test]
    fn enabling_auto_does_not_arm_from_an_expired_disabled_snapshot() {
        let now = Instant::now();
        let mut inner = KeepAwakeInner {
            daemon_connected: true,
            ..KeepAwakeInner::default()
        };
        observe_auto_keep_awake_snapshot(&mut inner, vec![12], now);
        configure_auto_keep_awake_state(&mut inner, true, false, vec![]);
        evaluate_auto_keep_awake_tick(
            &mut inner,
            now + Duration::from_secs(11),
            Duration::from_secs(1),
            Duration::from_secs(10),
        );
        reconcile_keep_awake_intent(&mut inner);

        assert!(inner.auto_snapshot_stale);
        assert!(!inner.armed);
    }

    #[test]
    fn persisted_suppression_rebases_then_accepts_only_a_fresh_activity_edge() {
        let now = Instant::now();
        let mut inner = KeepAwakeInner {
            daemon_connected: true,
            ..KeepAwakeInner::default()
        };
        configure_auto_keep_awake_state(&mut inner, true, true, vec![19]);
        observe_auto_keep_awake_snapshot(&mut inner, vec![19], now);
        assert!(inner.auto_suppressed_until_activity_edge);
        assert!(!inner.armed);

        observe_auto_keep_awake_snapshot(&mut inner, vec![19, 23], now + Duration::from_secs(1));

        assert!(!inner.auto_suppressed_until_activity_edge);
        assert!(inner.armed);
    }

    #[test]
    fn native_suppression_edge_detection_does_not_depend_on_a_webview_clock() {
        let now = Instant::now();
        let mut inner = KeepAwakeInner {
            daemon_connected: true,
            ..KeepAwakeInner::default()
        };
        configure_auto_keep_awake_state(&mut inner, true, true, vec![13]);
        observe_auto_keep_awake_snapshot(&mut inner, vec![13], now);
        assert!(inner.auto_suppressed_until_activity_edge);

        // This native observation path has no dependency on a renderer timer. A new active ID
        // therefore clears persisted suppression even while the WebView is hidden or throttled.
        observe_auto_keep_awake_snapshot(&mut inner, vec![13, 17], now + Duration::from_secs(3600));

        assert!(!inner.auto_suppressed_until_activity_edge);
        assert!(inner.armed);
        assert_eq!(inner.arm_source, Some(KeepAwakeArmSource::Auto));
    }

    #[test]
    fn auto_idle_release_keeps_the_settle_window() {
        let now = Instant::now();
        let mut inner = KeepAwakeInner {
            daemon_connected: true,
            auto_enabled: true,
            ..KeepAwakeInner::default()
        };
        observe_auto_keep_awake_snapshot(&mut inner, vec![17], now);
        observe_auto_keep_awake_snapshot(&mut inner, vec![], now);

        evaluate_auto_keep_awake_tick(
            &mut inner,
            now + Duration::from_secs(1),
            Duration::from_secs(2),
            Duration::from_secs(60),
        );
        reconcile_keep_awake_intent(&mut inner);
        assert!(inner.armed);
        evaluate_auto_keep_awake_tick(
            &mut inner,
            now + Duration::from_secs(2),
            Duration::from_secs(2),
            Duration::from_secs(60),
        );
        reconcile_keep_awake_intent(&mut inner);

        assert!(!inner.auto_hold_requested);
        assert!(!inner.armed);
    }

    #[test]
    fn stale_daemon_snapshot_releases_auto_after_a_deliberate_ceiling() {
        let now = Instant::now();
        let max_snapshot_age = Duration::from_secs(10 * 60);
        let mut inner = KeepAwakeInner {
            daemon_connected: true,
            auto_enabled: true,
            ..KeepAwakeInner::default()
        };
        observe_auto_keep_awake_snapshot(&mut inner, vec![29], now);
        inner.daemon_connected = false;

        evaluate_auto_keep_awake_tick(
            &mut inner,
            now + max_snapshot_age - Duration::from_secs(1),
            Duration::from_secs(60),
            max_snapshot_age,
        );
        reconcile_keep_awake_intent(&mut inner);
        assert!(inner.armed);
        assert!(!inner.auto_snapshot_stale);

        evaluate_auto_keep_awake_tick(
            &mut inner,
            now + max_snapshot_age,
            Duration::from_secs(60),
            max_snapshot_age,
        );
        reconcile_keep_awake_intent(&mut inner);
        assert!(!inner.armed);
        assert!(!inner.auto_hold_requested);
        assert!(inner.auto_active_agent_ids.is_empty());
        assert!(inner.auto_snapshot_stale);
    }

    #[test]
    fn fresh_snapshot_recovers_after_stale_daemon_release() {
        let now = Instant::now();
        let mut inner = KeepAwakeInner {
            daemon_connected: true,
            auto_enabled: true,
            ..KeepAwakeInner::default()
        };
        observe_auto_keep_awake_snapshot(&mut inner, vec![31], now);
        evaluate_auto_keep_awake_tick(
            &mut inner,
            now + Duration::from_secs(5),
            Duration::from_secs(60),
            Duration::from_secs(5),
        );
        reconcile_keep_awake_intent(&mut inner);
        assert!(inner.auto_snapshot_stale);
        assert!(!inner.armed);

        observe_auto_keep_awake_snapshot(&mut inner, vec![31], now + Duration::from_secs(6));

        assert!(!inner.auto_snapshot_stale);
        assert!(inner.armed);
    }

    #[test]
    fn starting_agent_without_output_gets_a_keep_awake_grace_hold() {
        let message = json!({
            "event": "process.statuses",
            "processes": [{
                "id": 41,
                "kind": "agent",
                "status": "starting",
                "agent_state": { "state": "idle", "last_output_at": null }
            }]
        })
        .to_string();

        assert_eq!(
            auto_keep_awake_active_agent_ids_from_message(&message),
            Some(vec![41])
        );
    }

    #[test]
    fn auto_snapshot_filters_non_agents_inactive_agents_and_non_live_waits() {
        let message = json!({
            "event": "process.statuses",
            "processes": [
                {
                    "id": 1,
                    "kind": "shell",
                    "status": "running",
                    "agent_state": { "state": "working" }
                },
                {
                    "id": 2,
                    "kind": "agent",
                    "status": "stopped",
                    "agent_state": { "state": "working" }
                },
                {
                    "id": 3,
                    "kind": "agent",
                    "status": "running",
                    "agent_state": { "state": "idle", "last_output_at": 1 }
                },
                {
                    "id": 4,
                    "kind": "agent",
                    "status": "running",
                    "agent_state": {
                        "state": "waiting",
                        "waiting_on": [{ "max_wait_ms": 10, "remaining_ms": 10, "paused": true }]
                    }
                },
                {
                    "id": 5,
                    "kind": "agent",
                    "status": "running",
                    "agent_state": {
                        "state": "waiting",
                        "waiting_on": [{ "max_wait_ms": 10, "remaining_ms": 0, "paused": false }]
                    }
                },
                {
                    "id": 6,
                    "kind": "agent",
                    "status": "starting",
                    "agent_state": { "state": "idle", "last_output_at": 1 }
                }
            ]
        })
        .to_string();

        assert_eq!(
            auto_keep_awake_active_agent_ids_from_message(&message),
            Some(vec![])
        );
    }

    #[test]
    fn malformed_or_unrelated_daemon_frames_do_not_change_auto_state() {
        assert_eq!(auto_keep_awake_active_agent_ids_from_message("{"), None);
        assert_eq!(
            auto_keep_awake_active_agent_ids_from_message(
                &json!({ "event": "daemon.hello", "processes": [] }).to_string()
            ),
            None
        );
        assert_eq!(
            auto_keep_awake_active_agent_ids_from_message(
                &json!({
                    "event": "process.statuses",
                    "processes": [{ "id": "not-an-id" }]
                })
                .to_string()
            ),
            None
        );
    }

    #[test]
    fn manual_request_takes_precedence_over_auto_and_hands_back_cleanly() {
        let mut inner = KeepAwakeInner {
            daemon_connected: true,
            auto_enabled: true,
            manual_requested: true,
            ..KeepAwakeInner::default()
        };
        observe_auto_keep_awake_snapshot(&mut inner, vec![43], Instant::now());
        assert_eq!(inner.arm_source, Some(KeepAwakeArmSource::Manual));

        inner.manual_requested = false;
        reconcile_keep_awake_intent(&mut inner);

        assert!(inner.armed);
        assert_eq!(inner.arm_source, Some(KeepAwakeArmSource::Auto));
    }

    #[test]
    fn native_auto_preference_survives_state_recreation() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join(KEEP_AWAKE_PREFERENCE_FILE);
        let state = KeepAwakeState::persistent(path.clone());
        let status = state.configure_auto(true, true, vec![47]);
        assert!(status.auto_enabled);
        assert!(status.auto_suppressed_until_activity_edge);
        drop(state);

        let restored = KeepAwakeState::persistent(path);
        let inner = restored
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert!(inner.auto_enabled);
        assert!(inner.auto_suppressed_until_activity_edge);
        assert_eq!(inner.auto_active_agent_ids, vec![47]);
        assert!(!inner.auto_observation_continuous);
    }

    #[test]
    fn only_an_explicit_user_stop_suppresses_auto_mode() {
        let state = KeepAwakeState::default();
        state.configure_auto(true, false, vec![]);

        state.stop(false).unwrap();
        {
            let inner = state
                .inner
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            assert!(!inner.auto_suppressed_until_activity_edge);
        }

        state.stop(true).unwrap();
        let inner = state
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert!(inner.auto_suppressed_until_activity_edge);
    }

    #[test]
    fn malformed_native_preference_falls_back_and_surfaces_a_warning() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join(KEEP_AWAKE_PREFERENCE_FILE);
        std::fs::write(&path, b"not json").unwrap();

        let state = KeepAwakeState::persistent(path);
        let status = {
            let inner = state
                .inner
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            keep_awake_status_for_platform(&inner, Instant::now())
        };

        assert!(!status.auto_enabled);
        assert!(
            status
                .auto_preference_warning
                .as_deref()
                .is_some_and(|warning| warning.contains("Could not load"))
        );
    }

    #[test]
    fn watchdog_emits_status_only_when_the_payload_changes() {
        let state = KeepAwakeState::default();
        let status = {
            let inner = state
                .inner
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            keep_awake_status_for_platform(&inner, Instant::now())
        };
        assert_eq!(
            state.emit_status_if_changed(status.clone()),
            Some(status.clone())
        );
        assert_eq!(state.emit_status_if_changed(status), None);

        let changed = {
            let mut inner = state
                .inner
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            inner.auto_enabled = true;
            keep_awake_status_for_platform(&inner, Instant::now())
        };
        assert_eq!(state.emit_status_if_changed(changed.clone()), Some(changed));
    }

    #[test]
    fn native_status_subscription_is_periodically_reasserted() {
        let state = KeepAwakeState::default();
        {
            let mut inner = state
                .inner
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            inner.auto_enabled = true;
            inner.daemon_connected = true;
        }
        let now = Instant::now();
        assert!(state.status_subscription_due(now));
        assert!(!state.status_subscription_due(
            now + KEEP_AWAKE_SUBSCRIPTION_REASSERT_INTERVAL - Duration::from_millis(1)
        ));
        assert!(state.status_subscription_due(now + KEEP_AWAKE_SUBSCRIPTION_REASSERT_INTERVAL));
    }

    #[test]
    fn disarmed_keep_awake_never_spawns() {
        let attempts = std::cell::Cell::new(0);
        let mut inner = KeepAwakeInner::default();
        sync_keep_awake_child_with(&mut inner, Instant::now(), || {
            attempts.set(attempts.get() + 1);
            test_keep_awake_child()
        });
        assert_eq!(attempts.get(), 0);
        assert!(inner.child.is_none());
    }

    #[test]
    fn watchdog_repairs_a_lost_assertion_in_the_same_tick() {
        let mut inner = KeepAwakeInner::default();
        begin_keep_awake_session(&mut inner);
        let now = Instant::now();
        sync_keep_awake_child_with(&mut inner, now, test_keep_awake_child);
        let first_pid = inner.child.as_ref().map(Child::id).unwrap();

        let child = inner.child.as_mut().unwrap();
        child.kill().unwrap();
        std::thread::sleep(Duration::from_millis(20));

        sync_keep_awake_child_with(
            &mut inner,
            now + Duration::from_millis(500),
            test_keep_awake_child,
        );
        let status = keep_awake_status_from(&inner, now + Duration::from_millis(500));
        let replacement_pid = status.assertion_pid.unwrap();
        assert!(status.armed);
        assert!(status.active);
        assert_ne!(status.assertion_pid, Some(first_pid));
        assert_eq!(status.respawn_count, 1);
        assert!(status.warning.is_none());
        assert!(
            status
                .notice
                .as_deref()
                .is_some_and(|warning| warning.contains("assertion restored"))
        );
        assert!(
            status
                .last_loss_reason
                .as_deref()
                .is_some_and(|reason| reason.contains("exited unexpectedly"))
        );

        stop_keep_awake_child(&mut inner).unwrap();
        assert!(inner.child.is_none());
        #[cfg(unix)]
        assert!(!test_process_exists(replacement_pid));
    }

    #[test]
    fn spawn_failure_keeps_intent_and_rate_limits_retries() {
        let attempts = std::cell::Cell::new(0);
        let mut inner = KeepAwakeInner::default();
        begin_keep_awake_session(&mut inner);
        let now = Instant::now();
        sync_keep_awake_child_with(&mut inner, now, || {
            attempts.set(attempts.get() + 1);
            Err(std::io::Error::new(
                std::io::ErrorKind::WouldBlock,
                "temporary process limit",
            ))
        });
        let failed = keep_awake_status_from(&inner, now);
        assert!(failed.armed);
        assert!(!failed.active);
        assert_eq!(attempts.get(), 1);
        assert!(
            failed
                .warning
                .as_deref()
                .is_some_and(|warning| warning.contains("Retrying"))
        );

        sync_keep_awake_child_with(&mut inner, now + Duration::from_millis(500), || {
            attempts.set(attempts.get() + 1);
            test_keep_awake_child()
        });
        assert_eq!(attempts.get(), 1);
        assert!(inner.child.is_none());

        sync_keep_awake_child_with(
            &mut inner,
            now + Duration::from_secs(1),
            test_keep_awake_child,
        );
        let recovered = keep_awake_status_from(&inner, now + Duration::from_secs(1));
        assert!(recovered.active);
        assert!(recovered.warning.is_none());
        assert!(
            recovered
                .notice
                .as_deref()
                .is_some_and(|notice| notice.contains("after 1 failed attempt"))
        );
        stop_keep_awake_child(&mut inner).unwrap();
    }

    #[test]
    fn auto_rearm_preserves_spawn_backoff() {
        let attempts = std::cell::Cell::new(0);
        let mut inner = KeepAwakeInner {
            daemon_connected: true,
            auto_enabled: true,
            ..KeepAwakeInner::default()
        };
        let now = Instant::now();
        observe_auto_keep_awake_snapshot(&mut inner, vec![5], now);
        sync_keep_awake_child_with(&mut inner, now, || {
            attempts.set(attempts.get() + 1);
            Err(std::io::Error::new(
                std::io::ErrorKind::WouldBlock,
                "temporary process limit",
            ))
        });
        assert_eq!(attempts.get(), 1);

        inner.auto_hold_requested = false;
        reconcile_keep_awake_intent(&mut inner);
        observe_auto_keep_awake_snapshot(&mut inner, vec![5], now + Duration::from_millis(250));
        sync_keep_awake_child_with(&mut inner, now + Duration::from_millis(500), || {
            attempts.set(attempts.get() + 1);
            test_keep_awake_child()
        });

        assert_eq!(attempts.get(), 1);
        assert!(inner.child.is_none());
        assert!(inner.next_spawn_attempt_at.is_some());
    }

    #[cfg(unix)]
    #[test]
    fn stop_targets_only_the_state_owned_child() {
        let owned = test_keep_awake_child().unwrap();
        let owned_pid = owned.id();
        let mut unrelated = test_keep_awake_child().unwrap();
        let unrelated_pid = unrelated.id();
        let mut inner = KeepAwakeInner {
            armed: true,
            child: Some(owned),
            ..KeepAwakeInner::default()
        };

        stop_keep_awake_child(&mut inner).unwrap();
        assert!(inner.child.is_none());
        assert!(!test_process_exists(owned_pid));
        assert!(test_process_exists(unrelated_pid));

        unrelated.kill().unwrap();
        unrelated.wait().unwrap();
    }

    fn test_keep_awake_child() -> std::io::Result<Child> {
        #[cfg(unix)]
        let mut command = {
            let mut command = Command::new("/bin/sleep");
            command.arg("30");
            command
        };
        // Windows has no sleep binary; a quiet ping holds a child alive the
        // same way and dies cleanly when killed.
        #[cfg(windows)]
        let mut command = {
            let mut command = Command::new("ping");
            command.args(["-n", "30", "127.0.0.1"]);
            command
        };
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
    }

    #[cfg(unix)]
    fn test_process_exists(pid: u32) -> bool {
        Command::new("/bin/kill")
            .args(["-0", &pid.to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }

    #[test]
    fn native_menu_ids_emit_only_frontend_actions() {
        assert_eq!(
            native_menu_action(MENU_ABOUT),
            Some(NativeMenuAction::About)
        );
        assert_eq!(
            native_menu_action(MENU_SETTINGS),
            Some(NativeMenuAction::Settings)
        );
        assert_eq!(
            native_menu_action(MENU_CHECK_UPDATES),
            Some(NativeMenuAction::CheckUpdates)
        );
        assert_eq!(
            native_menu_action(MENU_PREVIOUS_VIEW),
            Some(NativeMenuAction::PreviousView)
        );
        assert_eq!(
            native_menu_action(MENU_TOGGLE_PROJECT_RAIL),
            Some(NativeMenuAction::ToggleProjectRail)
        );
        assert_eq!(
            native_menu_action(MENU_TOGGLE_SECTION_RAIL),
            Some(NativeMenuAction::ToggleSectionRail)
        );
        assert_eq!(native_menu_action("predefined-quit"), None);
        assert_eq!(
            serde_json::to_string(&NativeMenuAction::CheckUpdates).unwrap(),
            "\"check_updates\""
        );
        assert_eq!(
            serde_json::to_string(&NativeMenuAction::PreviousView).unwrap(),
            "\"previous_view\""
        );
    }

    #[test]
    fn daemon_binary_defaults_to_desktop_binary_sibling() {
        let desktop = Path::new("/tmp/workman-target/debug/workman-desktop");
        assert_eq!(
            daemon_executable_from(desktop, None, true),
            desktop.with_file_name(format!("workmand{}", env::consts::EXE_SUFFIX))
        );
    }

    #[test]
    fn desktop_binary_is_the_headless_fallback() {
        let desktop = Path::new("/tmp/workman-target/debug/workman-desktop");
        assert_eq!(daemon_executable_from(desktop, None, false), desktop);
    }

    #[test]
    fn daemon_binary_override_wins() {
        let override_path = PathBuf::from("/opt/workman/bin/workmand");
        assert_eq!(
            daemon_executable_from(
                Path::new("/tmp/workman-desktop"),
                Some(override_path.clone()),
                false
            ),
            override_path
        );
    }

    #[test]
    fn embedded_daemon_mode_reads_data_dir_argument() {
        let args = [
            OsString::from("workman-desktop"),
            OsString::from("--data-dir"),
            OsString::from("/tmp/workman-data"),
        ];
        assert_eq!(
            embedded_daemon_data_dir(args),
            Some(PathBuf::from("/tmp/workman-data"))
        );
    }

    #[test]
    fn launch_environment_arguments_restore_wrk_overrides() {
        let data_dir = OsString::from("/tmp/workman-launch-data");
        let config = OsString::from("/tmp/workman-launch-config.yml");
        let daemon = OsString::from("/tmp/workman-launch-workmand");
        let environment = launch_environment([
            OsString::from("workman-desktop"),
            OsString::from("--workman-data-dir"),
            data_dir.clone(),
            OsString::from("--workman-config"),
            config.clone(),
            OsString::from("--workman-daemon-bin"),
            daemon.clone(),
        ]);
        assert_eq!(environment, (Some(data_dir), Some(config), Some(daemon)));
    }

    // The per-todo visual-QA contract pins bundles under `/tmp`, which only
    // exists on Unix hosts; the bundle flow itself is macOS-only.
    #[cfg(unix)]
    #[test]
    fn native_visual_qa_bundle_requires_per_todo_tmp_paths() {
        let identifier = "com.workman.todo307";
        let root = tempfile::Builder::new()
            .prefix("workman-todo307-qa.")
            .tempdir_in("/tmp")
            .unwrap();
        let data_path = root.path().join("data");
        let config_path = root.path().join("config.yml");
        std::fs::create_dir(&data_path).unwrap();
        std::fs::write(&config_path, b"agent_tools: []\n").unwrap();
        let data_dir = data_path.into_os_string();
        let config = config_path.into_os_string();
        let daemon = OsString::from("/tmp/workman-build/workmand");
        assert_eq!(
            validate_native_visual_qa_environment(
                identifier,
                Some(&data_dir),
                Some(&config),
                Some(&daemon)
            ),
            Ok(())
        );

        let fix_identifier = "com.workman.fix28";
        let fix_root = tempfile::Builder::new()
            .prefix("qa-fix28.")
            .tempdir_in("/tmp")
            .unwrap();
        let fix_data_path = fix_root.path().join("data");
        let fix_config_path = fix_root.path().join("config.yml");
        std::fs::create_dir(&fix_data_path).unwrap();
        std::fs::write(&fix_config_path, b"agent_tools: []\n").unwrap();
        let fix_data = fix_data_path.into_os_string();
        let fix_config = fix_config_path.into_os_string();
        assert_eq!(
            validate_native_visual_qa_environment(
                fix_identifier,
                Some(&fix_data),
                Some(&fix_config),
                Some(&daemon)
            ),
            Ok(())
        );

        for (data, config, expected) in [
            (
                None,
                Some(config.as_os_str()),
                "WORKMAN_DATA_DIR is missing",
            ),
            (
                Some(data_dir.as_os_str()),
                None,
                "WORKMAN_CONFIG is missing",
            ),
            (
                Some(std::ffi::OsStr::new("/Applications")),
                Some(config.as_os_str()),
                "must resolve to an isolated path under /tmp",
            ),
            (
                Some(std::ffi::OsStr::new("/tmp/other-qa/data")),
                Some(config.as_os_str()),
                "must already exist and resolve safely",
            ),
        ] {
            let error = validate_native_visual_qa_environment(identifier, data, config, None)
                .expect_err("unsafe QA environment must fail closed");
            assert!(error.contains(expected), "{error}");
        }
    }

    #[test]
    fn native_visual_qa_guard_does_not_restrict_stable_or_dev_bundles() {
        for identifier in ["com.workman.desktop", "com.workman.dev"] {
            assert_eq!(
                validate_native_visual_qa_environment(identifier, None, None, None),
                Ok(())
            );
        }
        let error = validate_native_visual_qa_environment(
            "com.workman.todo-not-a-number",
            None,
            None,
            None,
        )
        .expect_err("malformed QA identities must fail closed");
        assert!(error.contains("must be todoNNN or fixNNN"));

        let error =
            validate_native_visual_qa_environment("com.workman.fix-not-a-number", None, None, None)
                .expect_err("malformed fix identities must fail closed");
        assert!(error.contains("must be todoNNN or fixNNN"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn native_visual_qa_guard_reads_the_bundle_identifier_from_info_plist() {
        let bundle = tempfile::tempdir().unwrap();
        let contents = bundle.path().join("Workman Todo 307.app/Contents");
        let executable = contents.join("MacOS/workman-desktop");
        std::fs::create_dir_all(executable.parent().unwrap()).unwrap();
        std::fs::write(&executable, b"").unwrap();
        std::fs::write(
            contents.join("Info.plist"),
            br#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>com.workman.todo307</string>
</dict></plist>"#,
        )
        .unwrap();
        assert_eq!(
            macos_bundle_identifier_from_executable(&executable).unwrap(),
            Some("com.workman.todo307".to_owned())
        );
    }

    #[test]
    fn terminal_binary_frame_is_decoded_without_touching_raw_payload() {
        let mut bytes = Vec::from(*TERMINAL_FRAME_MAGIC);
        bytes.extend_from_slice(&42_i64.to_be_bytes());
        bytes.extend_from_slice(&8192_u64.to_be_bytes());
        bytes.push(1 | (1 << 1) | (2 << 2));
        bytes.extend_from_slice(b"\x1b[31mraw\x00bytes");

        let frame = parse_terminal_frame(&bytes).unwrap();
        assert_eq!(frame.process_id, 42);
        assert_eq!(frame.start_offset, 8192);
        assert!(frame.gap);
        assert_eq!(frame.kitty_keyboard_flags, 1);
        assert_eq!(frame.modify_other_keys, 2);
        assert_eq!(frame.data, b"\x1b[31mraw\x00bytes");
        assert!(parse_terminal_frame(b"not-a-terminal-frame").is_none());
    }

    #[test]
    fn terminal_input_is_encoded_as_a_response_free_binary_frame() {
        let input = TerminalInput {
            process_id: 42,
            data: b"raw\x00input".to_vec(),
        };
        let frame = encode_terminal_input(&input);

        assert_eq!(&frame[..4], TERMINAL_INPUT_MAGIC);
        assert_eq!(i64::from_be_bytes(frame[4..12].try_into().unwrap()), 42);
        assert_eq!(&frame[TERMINAL_INPUT_HEADER_LEN..], b"raw\x00input");
    }

    #[test]
    fn connection_status_marks_missing_and_older_builds_incompatible() {
        assert!(!ConnectionStatus::connected(1, None).version_compatible);
        let mut older = DaemonVersion::current();
        older.build_id = "older".to_owned();
        assert!(!ConnectionStatus::connected(1, Some(&older)).version_compatible);
        assert!(ConnectionStatus::connected(1, Some(&DaemonVersion::current())).version_compatible);
    }

    #[test]
    fn shell_paths_must_exist_and_are_canonicalized() {
        let root = tempfile::tempdir().unwrap();
        let nested = root.path().join("nested");
        std::fs::create_dir(&nested).unwrap();
        assert_eq!(
            canonical_shell_path(nested.to_str().unwrap()).unwrap(),
            std::fs::canonicalize(&nested).unwrap()
        );
        assert!(canonical_shell_path(root.path().join("missing").to_str().unwrap()).is_err());
        assert!(canonical_shell_path(" ").is_err());
    }

    #[test]
    fn browser_urls_accept_http_and_https_only() {
        assert_eq!(
            validated_browser_url("https://github.com/adrenallen/workman/pull/1").unwrap(),
            "https://github.com/adrenallen/workman/pull/1"
        );
        assert_eq!(
            validated_browser_url("http://127.0.0.1:4173/pr/1").unwrap(),
            "http://127.0.0.1:4173/pr/1"
        );
        assert_eq!(
            validated_browser_url("HTTPS://example.com/docs").unwrap(),
            "HTTPS://example.com/docs"
        );
        assert!(validated_browser_url("file:///tmp/report.html").is_err());
        assert!(validated_browser_url("javascript:alert(1)").is_err());
        assert!(validated_browser_url(" https://github.com/example ").is_err());
        assert!(validated_browser_url("https://github.com/example\n").is_err());
    }

    #[test]
    fn debug_browser_capture_is_scoped_to_precreated_per_todo_files() {
        let qa_root = tempfile::Builder::new()
            .prefix("workman-todo436-qa.")
            .tempdir_in("/tmp")
            .unwrap();
        let capture = qa_root.path().join("browser-open.log");
        std::fs::write(&capture, "").unwrap();

        append_browser_open_capture(&capture, "https://example.test/todo-436").unwrap();
        assert_eq!(
            std::fs::read_to_string(&capture).unwrap(),
            "https://example.test/todo-436\n"
        );

        let outside = tempfile::NamedTempFile::new_in("/tmp").unwrap();
        assert!(append_browser_open_capture(outside.path(), "https://example.test").is_err());
        let wrong_name = qa_root.path().join("not-the-capture.log");
        std::fs::write(&wrong_name, "").unwrap();
        assert!(append_browser_open_capture(&wrong_name, "https://example.test").is_err());
    }

    #[test]
    fn editor_detection_prefers_vscode_and_accepts_intellij_community() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("Cursor.app")).unwrap();
        std::fs::create_dir(root.path().join("Visual Studio Code.app")).unwrap();
        std::fs::create_dir(root.path().join("IntelliJ IDEA CE.app")).unwrap();

        let editors = detect_editors_in(&[root.path().to_owned()]);
        assert_eq!(
            editors.iter().map(|editor| editor.id).collect::<Vec<_>>(),
            ["vscode", "cursor", "intellij"]
        );
        assert!(editors[2].bundle_path.ends_with("IntelliJ IDEA CE.app"));
    }

    #[test]
    fn argv_templates_preserve_path_as_data_without_a_shell() {
        let workspace = Path::new("/tmp/work tree/$(touch nope); still-data");
        assert_eq!(
            parse_command_template(
                r#"tool --reuse-window "two words" --folder={path}"#,
                workspace
            )
            .unwrap(),
            [
                "tool",
                "--reuse-window",
                "two words",
                "--folder=/tmp/work tree/$(touch nope); still-data"
            ]
        );
    }

    #[test]
    fn argv_templates_reject_ambiguous_or_incomplete_commands() {
        let workspace = Path::new("/tmp/workspace");
        assert!(parse_command_template("tool --flag", workspace).is_err());
        assert!(parse_command_template("\"tool {path}", workspace).is_err());
        assert!(parse_command_template("{path} --flag", workspace).is_err());
        assert!(parse_command_template("", workspace).is_err());
    }
}
