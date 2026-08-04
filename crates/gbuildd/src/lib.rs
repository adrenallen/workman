//! Loopback-only HTTP and WebSocket control server for `gbuild`.

use std::{
    env,
    future::Future,
    io,
    net::{Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use axum::{
    Json, Router,
    extract::{
        Request, State, WebSocketUpgrade,
        ws::{CloseFrame, Message, WebSocket},
    },
    http::{HeaderMap, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    process::Command,
    sync::watch,
    time::{Instant, sleep, timeout},
};
use uuid::Uuid;

/// The name of the secure daemon discovery file in the gbuild data directory.
pub const DISCOVERY_FILE: &str = "daemon.json";

/// Runtime configuration for the local daemon server.
#[derive(Clone, Debug)]
pub struct DaemonConfig {
    /// Directory that holds the daemon port and token discovery file.
    pub data_dir: PathBuf,
    /// Requested loopback port. Zero asks the operating system for a free port.
    pub port: u16,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            data_dir: default_data_dir(),
            port: 0,
        }
    }
}

/// Contents of the daemon discovery file shared with local clients.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Discovery {
    pub port: u16,
    pub token: String,
    pub pid: u32,
}

impl Discovery {
    pub fn endpoint(&self) -> SocketAddr {
        SocketAddr::from((Ipv4Addr::LOCALHOST, self.port))
    }

    /// Read and validate a discovery file from `data_dir`.
    pub fn read(data_dir: impl AsRef<Path>) -> io::Result<Self> {
        let path = discovery_path(data_dir);
        ensure_private_file(&path)?;
        let bytes = std::fs::read(path)?;
        let discovery: Self = serde_json::from_slice(&bytes).map_err(invalid_data)?;
        if discovery.port == 0 || discovery.token.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "daemon discovery file has an invalid port or token",
            ));
        }
        Ok(discovery)
    }
}

/// A bound server. Constructing this value writes its secure discovery file.
pub struct DaemonServer {
    listener: TcpListener,
    discovery: Discovery,
    discovery_guard: DiscoveryGuard,
}

impl DaemonServer {
    /// Bind only IPv4 loopback and publish the selected port and a fresh bearer token.
    pub async fn bind(config: DaemonConfig) -> io::Result<Self> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, config.port)).await?;
        let port = listener.local_addr()?.port();
        let discovery = Discovery {
            port,
            token: new_token(),
            pid: std::process::id(),
        };
        let discovery_guard = DiscoveryGuard::publish(&config.data_dir, &discovery)?;

        Ok(Self {
            listener,
            discovery,
            discovery_guard,
        })
    }

    pub fn discovery(&self) -> &Discovery {
        &self.discovery
    }

    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.listener.local_addr()
    }

    /// Serve until `shutdown` resolves, then close upgraded sockets and remove discovery.
    pub async fn serve_until<F>(self, shutdown: F) -> io::Result<()>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let state = AppState {
            token: self.discovery.token.clone(),
            port: self.discovery.port,
            shutdown: shutdown_rx,
        };
        let app = router(state);
        let listener = self.listener;

        let shutdown_server = async move {
            shutdown.await;
            let _ = shutdown_tx.send(true);
        };

        // Keep the guard alive until all HTTP connections and WebSockets have drained.
        let _discovery_guard = self.discovery_guard;
        axum::serve(listener, app)
            .with_graceful_shutdown(shutdown_server)
            .await
    }
}

#[derive(Clone)]
struct AppState {
    token: String,
    port: u16,
    shutdown: watch::Receiver<bool>,
}

fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/ws", get(ws_upgrade))
        .fallback(|| async { StatusCode::NOT_FOUND })
        .layer(middleware::from_fn_with_state(
            state.clone(),
            authorize_local_request,
        ))
        .with_state(state)
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({ "status": "ok" }))
}

async fn ws_upgrade(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| control_session(socket, state.shutdown))
}

async fn control_session(mut socket: WebSocket, mut shutdown: watch::Receiver<bool>) {
    loop {
        tokio::select! {
            changed = shutdown.changed() => {
                if changed.is_ok() && *shutdown.borrow() {
                    let _ = socket.send(Message::Close(Some(CloseFrame {
                        code: 1001,
                        reason: "daemon shutting down".into(),
                    }))).await;
                }
                break;
            }
            message = socket.recv() => {
                let Some(message) = message else { break };
                let Ok(message) = message else { break };
                let reply = match message {
                    Message::Text(text) => {
                        if serde_json::from_str::<serde_json::Value>(&text).is_ok() {
                            Message::Text(text)
                        } else {
                            Message::Text(json!({
                                "type": "error",
                                "error": "text frames must contain valid JSON"
                            }).to_string().into())
                        }
                    }
                    Message::Binary(bytes) => Message::Binary(bytes),
                    Message::Ping(bytes) => Message::Pong(bytes),
                    Message::Pong(_) => continue,
                    Message::Close(frame) => {
                        let _ = socket.send(Message::Close(frame)).await;
                        break;
                    }
                };

                if socket.send(reply).await.is_err() {
                    break;
                }
            }
        }
    }
}

async fn authorize_local_request(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    if !valid_bearer(request.headers(), &state.token) {
        return (StatusCode::UNAUTHORIZED, "missing or invalid bearer token").into_response();
    }

    if !valid_host_and_origin(request.headers(), state.port) {
        return (StatusCode::FORBIDDEN, "invalid Host or Origin header").into_response();
    }

    next.run(request).await
}

fn valid_bearer(headers: &HeaderMap, token: &str) -> bool {
    let Some(value) = headers.get(header::AUTHORIZATION) else {
        return false;
    };
    let Ok(value) = value.to_str() else {
        return false;
    };
    let Some(candidate) = value.strip_prefix("Bearer ") else {
        return false;
    };
    constant_time_eq(candidate.as_bytes(), token.as_bytes())
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0_u8, |difference, (left, right)| {
            difference | (left ^ right)
        })
        == 0
}

fn valid_host_and_origin(headers: &HeaderMap, port: u16) -> bool {
    let Some(host) = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
    else {
        return false;
    };
    let Some(host) = parse_loopback_authority(host, port) else {
        return false;
    };

    let Some(origin) = headers.get(header::ORIGIN) else {
        // Native clients do not normally send Origin. Browser clients do and are checked below.
        return true;
    };
    let Ok(origin) = origin.to_str() else {
        return false;
    };
    let Ok(origin) = origin.parse::<axum::http::Uri>() else {
        return false;
    };
    if !matches!(origin.scheme_str(), Some("http" | "https")) {
        return false;
    }
    let Some(authority) = origin.authority() else {
        return false;
    };
    let Some(origin_host) = parse_loopback_authority(authority.as_str(), port) else {
        return false;
    };

    origin_host == host
}

fn parse_loopback_authority(authority: &str, expected_port: u16) -> Option<String> {
    let authority = authority.parse::<axum::http::uri::Authority>().ok()?;
    if authority.port_u16()? != expected_port {
        return None;
    }
    let host = authority.host().to_ascii_lowercase();
    matches!(host.as_str(), "127.0.0.1" | "localhost").then_some(host)
}

fn new_token() -> String {
    // Two independent UUIDv4 values provide 244 random bits while remaining header-safe.
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

pub fn discovery_path(data_dir: impl AsRef<Path>) -> PathBuf {
    data_dir.as_ref().join(DISCOVERY_FILE)
}

/// Resolve the platform data directory, with `GBUILD_DATA_DIR` as an explicit override.
pub fn default_data_dir() -> PathBuf {
    if let Some(path) = env::var_os("GBUILD_DATA_DIR") {
        return PathBuf::from(path);
    }

    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    if cfg!(target_os = "macos") {
        home.join("Library/Application Support/gbuild")
    } else if let Some(path) = env::var_os("XDG_DATA_HOME") {
        PathBuf::from(path).join("gbuild")
    } else {
        home.join(".local/share/gbuild")
    }
}

struct DiscoveryGuard {
    path: PathBuf,
    token: String,
}

impl DiscoveryGuard {
    fn publish(data_dir: &Path, discovery: &Discovery) -> io::Result<Self> {
        std::fs::create_dir_all(data_dir)?;
        let path = discovery_path(data_dir);
        let temporary = data_dir.join(format!(
            ".{DISCOVERY_FILE}.{}.{}.tmp",
            std::process::id(),
            Uuid::new_v4().simple()
        ));
        let bytes = serde_json::to_vec(discovery).map_err(invalid_data)?;

        write_private_file(&temporary, &bytes)?;
        if let Err(error) = std::fs::rename(&temporary, &path) {
            let _ = std::fs::remove_file(&temporary);
            return Err(error);
        }
        set_private_permissions(&path)?;

        Ok(Self {
            path,
            token: discovery.token.clone(),
        })
    }
}

impl Drop for DiscoveryGuard {
    fn drop(&mut self) {
        // Never remove a newer daemon's discovery file if it replaced ours.
        let ours = std::fs::read(&self.path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Discovery>(&bytes).ok())
            .is_some_and(|discovery| discovery.token == self.token);
        if ours {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

fn write_private_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::io::Write;

    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

fn set_private_permissions(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

fn ensure_private_file(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(path)?.permissions().mode();
        if mode & 0o077 != 0 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "daemon discovery file must not be accessible by group or others",
            ));
        }
    }
    Ok(())
}

fn invalid_data(error: impl std::error::Error + Send + Sync + 'static) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error)
}

/// Check that a discovered daemon answers its authenticated health endpoint.
pub async fn probe(discovery: &Discovery) -> bool {
    timeout(Duration::from_millis(500), probe_inner(discovery))
        .await
        .is_ok_and(|result| result.unwrap_or(false))
}

async fn probe_inner(discovery: &Discovery) -> io::Result<bool> {
    let mut stream = TcpStream::connect(discovery.endpoint()).await?;
    let request = format!(
        "GET /health HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nAuthorization: Bearer {}\r\nConnection: close\r\n\r\n",
        discovery.port, discovery.token
    );
    stream.write_all(request.as_bytes()).await?;

    let mut response = Vec::with_capacity(512);
    stream.take(4096).read_to_end(&mut response).await?;
    Ok(response.starts_with(b"HTTP/1.1 200") || response.starts_with(b"HTTP/1.0 200"))
}

/// Return a live daemon discovery record, spawning `daemon_executable` when necessary.
pub async fn discover_or_spawn(
    data_dir: impl AsRef<Path>,
    daemon_executable: impl AsRef<Path>,
    wait_timeout: Duration,
) -> io::Result<Discovery> {
    let data_dir = data_dir.as_ref();
    if let Ok(discovery) = Discovery::read(data_dir)
        && probe(&discovery).await
    {
        return Ok(discovery);
    }

    let mut child = Command::new(daemon_executable.as_ref())
        .arg("--data-dir")
        .arg(data_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let deadline = Instant::now() + wait_timeout;

    loop {
        if let Ok(discovery) = Discovery::read(data_dir)
            && probe(&discovery).await
        {
            return Ok(discovery);
        }
        if let Some(status) = child.try_wait()? {
            return Err(io::Error::other(format!(
                "gbuildd exited before becoming ready: {status}"
            )));
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "timed out waiting for gbuildd discovery",
            ));
        }
        sleep(Duration::from_millis(50)).await;
    }
}

#[cfg(test)]
mod tests {
    use std::{os::unix::fs::PermissionsExt, time::Duration};

    use futures_util::{SinkExt, StreamExt};
    use tempfile::TempDir;
    use tokio::sync::oneshot;
    use tokio_tungstenite::{
        connect_async,
        tungstenite::{Message, client::IntoClientRequest},
    };

    use super::*;

    struct TestServer {
        discovery: Discovery,
        data_dir: PathBuf,
        shutdown: Option<oneshot::Sender<()>>,
        task: tokio::task::JoinHandle<io::Result<()>>,
        _temp: TempDir,
    }

    impl TestServer {
        async fn start() -> Self {
            let temp = tempfile::tempdir().unwrap();
            let data_dir = temp.path().to_path_buf();
            let server = DaemonServer::bind(DaemonConfig {
                data_dir: data_dir.clone(),
                port: 0,
            })
            .await
            .unwrap();
            let discovery = server.discovery().clone();
            let (shutdown, receive_shutdown) = oneshot::channel();
            let task = tokio::spawn(server.serve_until(async move {
                let _ = receive_shutdown.await;
            }));

            Self {
                discovery,
                data_dir,
                shutdown: Some(shutdown),
                task,
                _temp: temp,
            }
        }

        fn request(&self) -> axum::http::Request<()> {
            let mut request = format!("ws://127.0.0.1:{}/ws", self.discovery.port)
                .into_client_request()
                .unwrap();
            request.headers_mut().insert(
                header::AUTHORIZATION,
                format!("Bearer {}", self.discovery.token).parse().unwrap(),
            );
            request
        }

        async fn stop(mut self) {
            self.shutdown.take().unwrap().send(()).unwrap();
            self.task.await.unwrap().unwrap();
        }
    }

    #[tokio::test]
    async fn authenticated_websocket_echoes_json_and_binary_frames() {
        let server = TestServer::start().await;
        let (mut socket, _) = connect_async(server.request()).await.unwrap();

        let json = r#"{"type":"echo","value":42}"#;
        socket.send(Message::Text(json.into())).await.unwrap();
        assert_eq!(
            socket.next().await.unwrap().unwrap(),
            Message::Text(json.into())
        );

        let binary = vec![0, 1, 2, 255];
        socket
            .send(Message::Binary(binary.clone().into()))
            .await
            .unwrap();
        assert_eq!(
            socket.next().await.unwrap().unwrap(),
            Message::Binary(binary.into())
        );

        socket.close(None).await.unwrap();
        server.stop().await;
    }

    #[tokio::test]
    async fn rejects_unauthenticated_and_cross_origin_websockets() {
        let server = TestServer::start().await;

        let mut unauthenticated = server.request();
        unauthenticated.headers_mut().remove(header::AUTHORIZATION);
        assert_http_error(unauthenticated, StatusCode::UNAUTHORIZED).await;

        let mut cross_origin = server.request();
        cross_origin
            .headers_mut()
            .insert(header::ORIGIN, "https://attacker.example".parse().unwrap());
        assert_http_error(cross_origin, StatusCode::FORBIDDEN).await;

        let mut rebound_host = server.request();
        rebound_host
            .headers_mut()
            .insert(header::HOST, "attacker.example".parse().unwrap());
        assert_http_error(rebound_host, StatusCode::FORBIDDEN).await;

        server.stop().await;
    }

    #[tokio::test]
    async fn discovery_is_private_probeable_and_removed_after_shutdown() {
        let server = TestServer::start().await;
        let path = discovery_path(&server.data_dir);
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        assert_eq!(Discovery::read(&server.data_dir).unwrap(), server.discovery);
        assert!(probe(&server.discovery).await);

        let data_dir = server.data_dir.clone();
        server.stop().await;
        assert!(!discovery_path(data_dir).exists());
    }

    #[tokio::test]
    async fn shutdown_closes_connected_websockets() {
        let mut server = TestServer::start().await;
        let (mut socket, _) = connect_async(server.request()).await.unwrap();

        server.shutdown.take().unwrap().send(()).unwrap();
        let message = tokio::time::timeout(Duration::from_secs(2), socket.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(matches!(message, Message::Close(_)));
        server.task.await.unwrap().unwrap();
    }

    async fn assert_http_error(request: axum::http::Request<()>, expected: StatusCode) {
        let error = connect_async(request).await.unwrap_err();
        let tokio_tungstenite::tungstenite::Error::Http(response) = error else {
            panic!("expected HTTP handshake error, got {error}");
        };
        assert_eq!(response.status(), expected);
    }
}
