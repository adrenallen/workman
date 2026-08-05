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
use gbuildd::{
    BUILD_ID, BUILD_VERSION, CONTROL_PROTOCOL_VERSION, DaemonConfig, DaemonServer, DaemonVersion,
    Discovery, default_data_dir, discover_or_spawn, probe,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tauri::{Emitter, State};
use tokio::{
    sync::{mpsc, oneshot},
    time::{sleep, timeout},
};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
    tungstenite::{Message, client::IntoClientRequest},
};

const STATUS_EVENT: &str = "daemon://status";
const MESSAGE_EVENT: &str = "daemon://message";
const TERMINAL_FRAME_MAGIC: &[u8; 4] = b"GBT1";
const TERMINAL_FRAME_HEADER_LEN: usize = 21;
const HELLO_REQUEST_ID: &str = "__gbuild_desktop_hello__";
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

#[derive(Clone, Serialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
enum DaemonFrame {
    Text(String),
    Binary(Vec<u8>),
    Terminal(TerminalFrame),
}

#[derive(Clone, Serialize)]
struct TerminalFrame {
    process_id: i64,
    start_offset: u64,
    gap: bool,
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
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            daemon_send,
            daemon_restart,
            daemon_status,
            shell_open_path
        ])
        .setup(move |app| {
            tauri::async_runtime::spawn(run_bridge(app.handle().clone(), task_state, receiver));
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("failed to run gbuild desktop");
}

/// Detect the private daemon-process mode used when no standalone `gbuildd` sits beside the app.
pub fn embedded_daemon_data_dir(args: impl IntoIterator<Item = OsString>) -> Option<PathBuf> {
    let mut args = args.into_iter().skip(1);
    while let Some(argument) = args.next() {
        if argument == "--data-dir" {
            return args.next().map(PathBuf::from);
        }
    }
    None
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
            "gbuild desktop: connected to daemon v{} (build {}, control protocol {})",
            daemon.version, daemon.build_id, daemon.control_protocol_version
        );
    } else {
        eprintln!("gbuild desktop: connected to a legacy daemon without a version handshake");
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
    Some(TerminalFrame {
        process_id,
        start_offset,
        gap: bytes[20] & 1 == 1,
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

fn open_shell_target(path: &Path, target: ShellOpenTarget) -> Result<(), String> {
    match target {
        ShellOpenTarget::Editor => open_in_editor(path),
        ShellOpenTarget::Finder => open_in_file_manager(path, false),
        ShellOpenTarget::Reveal => open_in_file_manager(path, true),
    }
}

fn open_in_editor(path: &Path) -> Result<(), String> {
    if let Some(editor) = env::var_os("GBUILD_EDITOR")
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
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("could not open {label}: {error}"))
}

fn daemon_executable() -> io::Result<PathBuf> {
    let current = env::current_exe()?;
    let sibling = current.with_file_name(format!("gbuildd{}", env::consts::EXE_SUFFIX));
    Ok(daemon_executable_from(
        &current,
        env::var_os("GBUILD_DAEMON_BIN").map(PathBuf::from),
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
            current.with_file_name(format!("gbuildd{}", env::consts::EXE_SUFFIX))
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
    fn daemon_binary_defaults_to_desktop_binary_sibling() {
        let desktop = Path::new("/tmp/gbuild-target/debug/gbuild-desktop");
        assert_eq!(
            daemon_executable_from(desktop, None, true),
            Path::new("/tmp/gbuild-target/debug/gbuildd")
        );
    }

    #[test]
    fn desktop_binary_is_the_headless_fallback() {
        let desktop = Path::new("/tmp/gbuild-target/debug/gbuild-desktop");
        assert_eq!(daemon_executable_from(desktop, None, false), desktop);
    }

    #[test]
    fn daemon_binary_override_wins() {
        let override_path = PathBuf::from("/opt/gbuild/bin/gbuildd");
        assert_eq!(
            daemon_executable_from(
                Path::new("/tmp/gbuild-desktop"),
                Some(override_path.clone()),
                false
            ),
            override_path
        );
    }

    #[test]
    fn embedded_daemon_mode_reads_data_dir_argument() {
        let args = [
            OsString::from("gbuild-desktop"),
            OsString::from("--data-dir"),
            OsString::from("/tmp/gbuild-data"),
        ];
        assert_eq!(
            embedded_daemon_data_dir(args),
            Some(PathBuf::from("/tmp/gbuild-data"))
        );
    }

    #[test]
    fn terminal_binary_frame_is_decoded_without_touching_raw_payload() {
        let mut bytes = Vec::from(*TERMINAL_FRAME_MAGIC);
        bytes.extend_from_slice(&42_i64.to_be_bytes());
        bytes.extend_from_slice(&8192_u64.to_be_bytes());
        bytes.push(1);
        bytes.extend_from_slice(b"\x1b[31mraw\x00bytes");

        let frame = parse_terminal_frame(&bytes).unwrap();
        assert_eq!(frame.process_id, 42);
        assert_eq!(frame.start_offset, 8192);
        assert!(frame.gap);
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
}
