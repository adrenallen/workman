use std::{
    env,
    error::Error,
    ffi::OsString,
    io,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use futures_util::{SinkExt, StreamExt};
use gbuildd::{DaemonConfig, DaemonServer, Discovery, default_data_dir, discover_or_spawn};
use serde::Serialize;
use tauri::{Emitter, State};
use tokio::sync::mpsc;
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
    tungstenite::{Message, client::IntoClientRequest},
};

const STATUS_EVENT: &str = "daemon://status";
const MESSAGE_EVENT: &str = "daemon://message";

#[derive(Clone)]
struct BridgeState {
    sender: mpsc::Sender<String>,
    status: Arc<Mutex<ConnectionStatus>>,
}

#[derive(Clone, Serialize)]
struct ConnectionStatus {
    status: &'static str,
    message: Option<String>,
    port: Option<u16>,
}

impl ConnectionStatus {
    fn connecting() -> Self {
        Self {
            status: "connecting",
            message: None,
            port: None,
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
enum DaemonFrame {
    Text(String),
    Binary(Vec<u8>),
}

#[tauri::command]
fn daemon_send(message: String, state: State<'_, BridgeState>) -> Result<(), String> {
    if message.len() > 1024 * 1024 {
        return Err("control message exceeds the 1 MiB limit".to_owned());
    }
    state
        .sender
        .try_send(message)
        .map_err(|error| format!("daemon bridge is not accepting messages: {error}"))
}

#[tauri::command]
fn daemon_status(state: State<'_, BridgeState>) -> ConnectionStatus {
    lock_status(&state.status).clone()
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
        .invoke_handler(tauri::generate_handler![daemon_send, daemon_status])
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
    mut receiver: mpsc::Receiver<String>,
) {
    let mut reconnect_delay = Duration::from_millis(250);
    loop {
        publish_status(&app, &state, ConnectionStatus::connecting());
        match connect_daemon().await {
            Ok((discovery, mut socket)) => {
                reconnect_delay = Duration::from_millis(250);
                publish_status(
                    &app,
                    &state,
                    ConnectionStatus {
                        status: "connected",
                        message: None,
                        port: Some(discovery.port),
                    },
                );

                loop {
                    tokio::select! {
                        outgoing = receiver.recv() => {
                            let Some(outgoing) = outgoing else { return };
                            if socket.send(Message::Text(outgoing.into())).await.is_err() {
                                break;
                            }
                        }
                        incoming = socket.next() => {
                            let Some(incoming) = incoming else { break };
                            match incoming {
                                Ok(Message::Text(text)) => {
                                    let _ = app.emit(MESSAGE_EVENT, DaemonFrame::Text(text.to_string()));
                                }
                                Ok(Message::Binary(bytes)) => {
                                    let _ = app.emit(MESSAGE_EVENT, DaemonFrame::Binary(bytes.to_vec()));
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
                    ConnectionStatus {
                        status: "disconnected",
                        message: Some("Daemon connection closed; retrying".to_owned()),
                        port: None,
                    },
                );
            }
            Err(error) => {
                publish_status(
                    &app,
                    &state,
                    ConnectionStatus {
                        status: "disconnected",
                        message: Some(error),
                        port: None,
                    },
                );
            }
        }
        tokio::time::sleep(reconnect_delay).await;
        reconnect_delay = (reconnect_delay * 2).min(Duration::from_secs(3));
    }
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

fn lock_status(status: &Mutex<ConnectionStatus>) -> std::sync::MutexGuard<'_, ConnectionStatus> {
    status
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
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
}
