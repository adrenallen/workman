use std::{
    env,
    error::Error,
    ffi::OsString,
    io,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tauri::{
    Emitter, State,
    menu::{Menu, MenuBuilder, MenuItemBuilder, SubmenuBuilder, WINDOW_SUBMENU_ID},
};
use tokio::{
    sync::{mpsc, oneshot},
    time::{sleep, timeout},
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

mod native_notifications;

const STATUS_EVENT: &str = "daemon://status";
const MESSAGE_EVENT: &str = "daemon://message";
const NATIVE_MENU_EVENT: &str = "menu://action";
const MENU_ABOUT: &str = "workman.about";
const MENU_SETTINGS: &str = "workman.settings";
const MENU_CHECK_UPDATES: &str = "workman.check_updates";
const MENU_TOGGLE_PROJECT_RAIL: &str = "view.toggle_project_rail";
const MENU_TOGGLE_SECTION_RAIL: &str = "view.toggle_section_rail";
const TERMINAL_FRAME_MAGIC: &[u8; 4] = b"WRK1";
const TERMINAL_FRAME_HEADER_LEN: usize = 21;
const HELLO_REQUEST_ID: &str = "__workman_desktop_hello__";
const HELLO_TIMEOUT: Duration = Duration::from_millis(750);
const RESTART_TIMEOUT: Duration = Duration::from_secs(6);

#[derive(Clone)]
struct BridgeState {
    sender: mpsc::Sender<BridgeCommand>,
    status: Arc<Mutex<ConnectionStatus>>,
}

enum BridgeCommand {
    Send(String),
    Restart(oneshot::Sender<Result<(), String>>),
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
    ToggleProjectRail,
    ToggleSectionRail,
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
fn daemon_status(state: State<'_, BridgeState>) -> ConnectionStatus {
    lock_status(&state.status).clone()
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
    let state = BridgeState {
        sender,
        status: Arc::new(Mutex::new(ConnectionStatus::connecting())),
    };
    let task_state = state.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .manage(state)
        .menu(build_native_menu)
        .on_menu_event(|app, event| {
            if let Some(action) = native_menu_action(event.id().as_ref()) {
                let _ = app.emit(NATIVE_MENU_EVENT, action);
            }
        })
        .invoke_handler(tauri::generate_handler![
            daemon_send,
            daemon_restart,
            daemon_status,
            shell_open_path,
            shell_open_url,
            shell_detect_editors,
            shell_open_with,
            native_notifications::native_notification_permission_state,
            native_notifications::native_notification_request_permission,
            native_notifications::native_notification_show
        ])
        .setup(move |app| {
            tauri::async_runtime::spawn(run_bridge(app.handle().clone(), task_state, receiver));
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("failed to run workman desktop");
}

fn build_native_menu(app: &tauri::AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let about = MenuItemBuilder::with_id(MENU_ABOUT, "About Workman").build(app)?;
    let settings = MenuItemBuilder::with_id(MENU_SETTINGS, "Settings…")
        .accelerator("CmdOrCtrl+,")
        .build(app)?;
    let check_updates =
        MenuItemBuilder::with_id(MENU_CHECK_UPDATES, "Check for Updates…").build(app)?;
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
        MENU_TOGGLE_PROJECT_RAIL => Some(NativeMenuAction::ToggleProjectRail),
        MENU_TOGGLE_SECTION_RAIL => Some(NativeMenuAction::ToggleSectionRail),
        _ => None,
    }
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

const NATIVE_VISUAL_QA_BUNDLE_PREFIX: &str = "com.workman.todo";

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

fn native_visual_qa_todo_id(identifier: &str) -> Option<&str> {
    let todo_id = identifier.strip_prefix(NATIVE_VISUAL_QA_BUNDLE_PREFIX)?;
    (!todo_id.is_empty() && todo_id.bytes().all(|byte| byte.is_ascii_digit())).then_some(todo_id)
}

fn validate_native_visual_qa_environment(
    identifier: &str,
    data_dir: Option<&std::ffi::OsStr>,
    config: Option<&std::ffi::OsStr>,
    daemon_bin: Option<&std::ffi::OsStr>,
) -> Result<(), String> {
    if !identifier.starts_with(NATIVE_VISUAL_QA_BUNDLE_PREFIX) {
        return Ok(());
    }
    let todo_id = native_visual_qa_todo_id(identifier).ok_or_else(|| {
        native_visual_qa_error(
            identifier,
            "bundle identifier must end with a numeric todo ID",
        )
    })?;
    let token = format!("workman-todo{todo_id}");
    let data_dir =
        required_native_visual_qa_path(identifier, "WORKMAN_DATA_DIR", data_dir, &token)?;
    let config = required_native_visual_qa_path(identifier, "WORKMAN_CONFIG", config, &token)?;
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

fn required_native_visual_qa_path<'a>(
    identifier: &str,
    name: &str,
    value: Option<&'a std::ffi::OsStr>,
    token: &str,
) -> Result<PathBuf, String> {
    let value =
        value.ok_or_else(|| native_visual_qa_error(identifier, &format!("{name} is missing")))?;
    let path = Path::new(value);
    if !path.is_absolute() {
        return Err(native_visual_qa_error(
            identifier,
            &format!(
                "{name} must be an absolute per-todo path under /tmp containing {token}; got {}",
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
                "{name} must resolve to a per-todo path under /tmp containing {token}; got {}",
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
    mut receiver: mpsc::Receiver<BridgeCommand>,
) {
    let mut reconnect_delay = Duration::from_millis(250);
    loop {
        publish_status(&app, &state, ConnectionStatus::connecting());
        match connect_daemon().await {
            Ok((discovery, mut socket)) => {
                reconnect_delay = Duration::from_millis(250);
                let daemon_version = negotiate_daemon_version(&mut socket).await;
                log_daemon_version(daemon_version.as_ref());
                publish_status(
                    &app,
                    &state,
                    ConnectionStatus::connected(discovery.port, daemon_version.as_ref()),
                );

                loop {
                    tokio::select! {
                        outgoing = receiver.recv() => {
                            let Some(outgoing) = outgoing else { return };
                            match outgoing {
                                BridgeCommand::Send(message) => {
                                    if socket.send(Message::Text(message.into())).await.is_err() {
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
                            }
                        }
                        incoming = socket.next() => {
                            let Some(incoming) = incoming else { break };
                            match incoming {
                                Ok(Message::Text(text)) => {
                                    let _ = app.emit(MESSAGE_EVENT, DaemonFrame::Text(text.to_string()));
                                }
                                Ok(Message::Binary(bytes)) => {
                                    let frame = parse_terminal_frame(&bytes)
                                        .map(DaemonFrame::Terminal)
                                        .unwrap_or_else(|| DaemonFrame::Binary(bytes.to_vec()));
                                    let _ = app.emit(MESSAGE_EVENT, frame);
                                }
                                Ok(Message::Close(_)) | Err(_) => break,
                                Ok(Message::Ping(_) | Message::Pong(_) | Message::Frame(_)) => {}
                            }
                        }
                    }
                }
                publish_status(
                    &app,
                    &state,
                    ConnectionStatus::disconnected("Daemon connection closed; retrying"),
                );
            }
            Err(error) => {
                publish_status(&app, &state, ConnectionStatus::disconnected(error));
            }
        }
        tokio::time::sleep(reconnect_delay).await;
        reconnect_delay = (reconnect_delay * 2).min(Duration::from_secs(3));
    }
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
    let (socket, _) = connect_async(request)
        .await
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
    if !url.starts_with("https://") && !url.starts_with("http://") {
        return Err("browser URL must use http or https".to_owned());
    }
    Ok(url)
}

fn open_in_browser(url: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    return spawn_detached(Command::new("open").arg(url), "default browser");

    #[cfg(target_os = "linux")]
    return spawn_detached(Command::new("xdg-open").arg(url), "default browser");

    #[cfg(target_os = "windows")]
    return spawn_detached(Command::new("explorer").arg(url), "default browser");

    #[allow(unreachable_code)]
    Err("opening a browser is not supported on this platform".to_owned())
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

#[cfg(test)]
mod tests {
    use super::*;

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
    }

    #[test]
    fn daemon_binary_defaults_to_desktop_binary_sibling() {
        let desktop = Path::new("/tmp/workman-target/debug/workman-desktop");
        assert_eq!(
            daemon_executable_from(desktop, None, true),
            Path::new("/tmp/workman-target/debug/workmand")
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
                Some(std::ffi::OsStr::new(
                    "/Users/g/Library/Application Support/workman",
                )),
                Some(config.as_os_str()),
                "must resolve to a per-todo path under /tmp",
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
        assert!(error.contains("must end with a numeric todo ID"));
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
        assert!(validated_browser_url("file:///tmp/report.html").is_err());
        assert!(validated_browser_url("javascript:alert(1)").is_err());
        assert!(validated_browser_url(" https://github.com/example ").is_err());
        assert!(validated_browser_url("https://github.com/example\n").is_err());
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
