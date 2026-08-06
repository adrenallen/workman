//! Loopback-only HTTP and WebSocket control server for `workman`.

use std::{
    env, fmt,
    future::Future,
    io,
    net::{Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
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
    sync::{Mutex, watch},
    time::{Instant, MissedTickBehavior, interval, sleep, timeout},
};
use uuid::Uuid;

pub mod config;
mod context_actions;
mod control;
mod coordination;
pub mod lifecycle;
mod mcp;
mod migration;
mod process_registry;
pub mod process_stats;
pub mod readiness;
pub mod runtime_doctor;
mod settings;
mod subprocesses;
mod timer_events;
mod timers;
mod updates;
mod user_config;
mod version;
mod worktree_integrations;
mod worktree_operations;
pub mod worktrees;

pub use config::{
    ConfigError, LEGACY_AWM_CONFIG_FILE, LEGACY_GBUILD_CONFIG_FILE, SyncReport, TrustFieldChange,
    TrustFields, TrustReview, WORKMAN_CONFIG_FILE, WorkmanConfig, YmlProcess, is_process_trusted,
    parse_workman_yml, project_config_path, sync_workman_yml, sync_workman_yml_file,
    trust_hash_for_process,
};
pub use lifecycle::{
    LifecycleOptions, auto_start_project, spawn_lifecycle_supervisor,
    spawn_lifecycle_supervisor_with_options,
};
pub use mcp::WORKMAN_MCP_TOKEN_HEADER;
pub use migration::migrate_legacy_data_dir;
pub use process_registry::{
    BulkFailure, BulkProcessResult, OUTPUT_DIRECTORY, ProcessRegistry, ProcessStatusView,
    RegistryError, RegistryResult, WORKMAN_OUTPUT_CAPACITY_ENV, output_spill_capacity_from_env,
};
pub use process_stats::{
    DescendantProcessStats, LiveStatsSnapshot, ProcessRuntimeStats, ProjectCounts,
    ProjectRuntimeStats, inspect_process_tree, inspect_process_tree_in,
};
pub use readiness::{
    BoundListener, DEFAULT_PORT_WAIT, DetectedListener, MAX_PORT_WAIT, PortDetector,
    ReadinessError, ReadinessService, ReadinessState, Service, ServiceProtocol, SystemPortDetector,
    WaitForBoundPortResult,
};
pub use settings::{
    DaemonSettingsInfo, McpClient, McpClientSetup, McpConnectionInfo, McpSetupField,
    McpSetupFormat, mcp_connection_info,
};
pub use updates::UpdateStatus;
pub use user_config::{
    AgentToolSyncReport, USER_CONFIG_FILE, UserAgentTool, UserConfig, UserConfigError,
    WORKMAN_CONFIG_ENV, parse_user_config, sync_user_agent_tools, sync_user_config_file,
    user_config_path,
};
pub use version::{BUILD_ID, BUILD_VERSION, CONTROL_PROTOCOL_VERSION, DaemonVersion};

pub type SharedProcessRegistry = Arc<Mutex<ProcessRegistry>>;

const TERMINAL_FRAME_MAGIC: &[u8; 4] = b"WRK1";
const TERMINAL_FRAME_HEADER_LEN: usize = 21;
const TERMINAL_STREAM_CHUNK_BYTES: usize = 64 * 1024;
const TERMINAL_STREAM_CHUNKS_PER_TICK: usize = 4;
const TERMINAL_STREAM_TICK: Duration = Duration::from_millis(16);
const PROCESS_STATUS_STREAM_TICK: Duration = Duration::from_millis(500);

/// The name of the secure daemon discovery file in the workman data directory.
pub const DISCOVERY_FILE: &str = "daemon.json";

/// The SQLite state file stored beside daemon discovery metadata.
pub const DATABASE_FILE: &str = "workman.sqlite3";

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
    registry: SharedProcessRegistry,
    data_dir: PathBuf,
    started_at: Instant,
    updates: updates::UpdateService,
}

impl DaemonServer {
    /// Bind only IPv4 loopback and publish the selected port and a fresh bearer token.
    pub async fn bind(config: DaemonConfig) -> io::Result<Self> {
        let started_at = Instant::now();
        migration::migrate_default_paths_if_needed(&config.data_dir)?;
        std::fs::create_dir_all(&config.data_dir)?;
        let store = workman_core::Store::open(database_path(&config.data_dir))
            .map_err(registry_io_error)?;
        let user_config_path = user_config_path();
        sync_user_config_file(&store, &user_config_path).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{}: {error}", user_config_path.display()),
            )
        })?;
        if let Err(error) = worktrees::reconcile_existing_projects(&store) {
            eprintln!("workman daemon: worktree metadata reconciliation skipped: {error}");
        }
        let registry = Arc::new(Mutex::new(
            ProcessRegistry::with_output_persistence(
                store,
                config.data_dir.join(OUTPUT_DIRECTORY),
                output_spill_capacity_from_env(),
            )
            .map_err(registry_io_error)?,
        ));
        // Build the HTTP update client before publishing discovery so readiness never advertises
        // a listener that is still loading platform TLS state.
        let updates = updates::UpdateService::new(&config.data_dir).map_err(io::Error::other)?;
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
            registry,
            data_dir: config.data_dir,
            started_at,
            updates,
        })
    }

    pub fn discovery(&self) -> &Discovery {
        &self.discovery
    }

    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.listener.local_addr()
    }

    /// Clone the process registry handle used by WebSocket control sessions.
    pub fn registry(&self) -> SharedProcessRegistry {
        self.registry.clone()
    }

    /// Serve until `shutdown` resolves, then close upgraded sockets and remove discovery.
    pub async fn serve_until<F>(self, shutdown: F) -> io::Result<()>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let lifecycle_task =
            spawn_lifecycle_supervisor(self.registry.clone(), shutdown_rx.clone())?;
        let timer_events = timer_events::TimerLifecycleHub::default();
        let worktree_operations = worktree_operations::WorktreeOperationHub::default();
        let timer_task = timers::spawn_timer_scheduler(
            self.registry.clone(),
            timer_events.clone(),
            shutdown_rx.clone(),
        );
        let live_stats = process_stats::LiveStatsHub::new();
        let live_stats_task = process_stats::spawn_live_stats_sampler(
            live_stats.clone(),
            self.registry.clone(),
            shutdown_rx.clone(),
        );
        let lifecycle_shutdown = shutdown_tx.clone();
        let runtime_settings = settings::DaemonRuntimeSettings::new(
            self.data_dir,
            self.discovery.clone(),
            self.started_at,
            self.updates,
        );
        let state = AppState {
            token: self.discovery.token.clone(),
            port: self.discovery.port,
            shutdown: shutdown_rx.clone(),
            shutdown_request: shutdown_tx.clone(),
            settings: runtime_settings,
            registry: self.registry,
            live_stats,
            timer_events,
            worktree_operations,
        };
        let app = router(state);
        let listener = self.listener;

        let mut requested_shutdown = shutdown_rx;
        let shutdown_server = async move {
            tokio::select! {
                _ = shutdown => {}
                _ = requested_shutdown.changed() => {}
            }
            let _ = shutdown_tx.send(true);
        };

        // Keep the guard alive until all HTTP connections and WebSockets have drained.
        let _discovery_guard = self.discovery_guard;
        let result = axum::serve(listener, app)
            .with_graceful_shutdown(shutdown_server)
            .await;
        let _ = lifecycle_shutdown.send(true);
        let _ = lifecycle_task.await;
        let _ = timer_task.await;
        let _ = live_stats_task.await;
        result
    }
}

#[derive(Clone)]
struct AppState {
    token: String,
    port: u16,
    shutdown: watch::Receiver<bool>,
    shutdown_request: watch::Sender<bool>,
    settings: settings::DaemonRuntimeSettings,
    registry: SharedProcessRegistry,
    live_stats: process_stats::LiveStatsHub,
    timer_events: timer_events::TimerLifecycleHub,
    worktree_operations: worktree_operations::WorktreeOperationHub,
}

fn router(state: AppState) -> Router {
    let mcp_url = format!("http://127.0.0.1:{}/mcp", state.port);
    let (mcp_service, mcp_sessions) =
        mcp::streamable_http_service(state.registry.clone(), mcp_url, state.timer_events.clone());
    Router::new()
        .route("/health", get(health))
        .route("/ws", get(ws_upgrade))
        .nest_service("/mcp", mcp_service)
        .fallback(|| async { StatusCode::NOT_FOUND })
        .layer(middleware::from_fn_with_state(
            mcp_sessions,
            mcp::require_known_session,
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            authorize_local_request,
        ))
        .with_state(state)
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({
        "status": "ok",
        "version": BUILD_VERSION,
        "build_id": BUILD_ID,
        "control_protocol_version": CONTROL_PROTOCOL_VERSION,
    }))
}

async fn ws_upgrade(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| {
        control_session(
            socket,
            state.shutdown,
            state.shutdown_request,
            state.settings,
            state.registry,
            state.live_stats,
            state.timer_events,
            state.worktree_operations,
        )
    })
}

async fn control_session(
    mut socket: WebSocket,
    mut shutdown: watch::Receiver<bool>,
    shutdown_request: watch::Sender<bool>,
    settings: settings::DaemonRuntimeSettings,
    registry: SharedProcessRegistry,
    live_stats: process_stats::LiveStatsHub,
    timer_events: timer_events::TimerLifecycleHub,
    worktree_operations: worktree_operations::WorktreeOperationHub,
) {
    let mcp_url = settings.info().mcp.endpoint;
    let _live_stats_client = live_stats.client_connected();
    let mut terminal = TerminalSubscription::default();
    let mut status_subscribed = false;
    let mut timer_event_cursor = timer_events.latest_sequence();
    let mut terminal_tick = interval(TERMINAL_STREAM_TICK);
    terminal_tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
    terminal_tick.tick().await;
    let mut status_tick = interval(PROCESS_STATUS_STREAM_TICK);
    status_tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
    status_tick.tick().await;

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
                            let response = match handle_session_control(
                                &text,
                                &registry,
                                &settings,
                                &shutdown_request,
                                &mut terminal,
                                &mut status_subscribed,
                                &worktree_operations,
                            ).await {
                                Some(response) => response,
                                None => control::handle_text(&text, &registry, &mcp_url).await,
                            };
                            Message::Text(response.into())
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
            _ = terminal_tick.tick(), if terminal.process_id.is_some() => {
                match terminal_output_frames(&registry, &mut terminal).await {
                    Ok(frames) => {
                        for frame in frames {
                            if socket.send(Message::Binary(frame.into())).await.is_err() {
                                return;
                            }
                        }
                    }
                    Err(error) => {
                        let process_id = terminal.process_id.take();
                        let event = json!({
                            "event": "terminal.error",
                            "process_id": process_id,
                            "error": { "code": error.code(), "message": error.to_string() }
                        });
                        if socket.send(Message::Text(event.to_string().into())).await.is_err() {
                            break;
                        }
                    }
                }
            }
            _ = status_tick.tick(), if status_subscribed => {
                let (statuses, timers) = {
                    let mut registry = registry.lock().await;
                    let statuses = registry.list_statuses(None);
                    let timers = timers::TimerService::new(&mut registry)
                        .list_active(timers::now_millis());
                    (statuses, timers)
                };
                if let (Ok(processes), Ok(timers)) = (statuses, timers) {
                    let stats = live_stats.snapshot().await;
                    let (latest_timer_event, lifecycle_events) =
                        timer_events.events_since(timer_event_cursor);
                    timer_event_cursor = latest_timer_event;
                    let worktree_operations = worktree_operations.snapshot();
                    let event = json!({
                        "event": "process.statuses",
                        "processes": processes,
                        "stats": stats,
                        "timers": timers,
                        "timer_events": lifecycle_events,
                        "worktree_operations": worktree_operations,
                    });
                    if socket.send(Message::Text(event.to_string().into())).await.is_err() {
                        break;
                    }
                }
            }
        }
    }
}

#[derive(Default)]
struct TerminalSubscription {
    process_id: Option<workman_core::ProcessId>,
    offset: u64,
}

async fn handle_session_control(
    text: &str,
    registry: &SharedProcessRegistry,
    settings: &settings::DaemonRuntimeSettings,
    shutdown_request: &watch::Sender<bool>,
    terminal: &mut TerminalSubscription,
    status_subscribed: &mut bool,
    worktree_operations: &worktree_operations::WorktreeOperationHub,
) -> Option<String> {
    let request: serde_json::Value = serde_json::from_str(text).ok()?;
    let method = request.get("method")?.as_str()?;
    if !matches!(
        method,
        "daemon.hello"
            | "daemon.info"
            | "daemon.restart"
            | "daemon.update_check"
            | "daemon.update_preferences"
            | "daemon.update_apply"
            | "terminal.attach"
            | "terminal.detach"
            | "process.status_subscribe"
            | "process.status_unsubscribe"
            | "worktree.create_async"
            | "worktree.fork_async"
            | "worktree.adopt_async"
    ) {
        return None;
    }

    let id = request.get("id").cloned().unwrap_or_default();
    if matches!(
        method,
        "worktree.create_async" | "worktree.fork_async" | "worktree.adopt_async"
    ) {
        let params = request.get("params").cloned().unwrap_or_default();
        return Some(
            match worktree_operations::start(
                method,
                params,
                registry.clone(),
                worktree_operations.clone(),
            )
            .await
            {
                Ok(result) => json!({ "id": id, "ok": true, "result": result }).to_string(),
                Err(error) => json!({
                    "id": id,
                    "ok": false,
                    "error": { "code": error.code, "message": error.message }
                })
                .to_string(),
            },
        );
    }
    if method == "daemon.hello" {
        return Some(
            json!({ "id": id, "ok": true, "result": DaemonVersion::current() }).to_string(),
        );
    }
    if method == "daemon.info" {
        return Some(json!({ "id": id, "ok": true, "result": settings.info() }).to_string());
    }
    if method == "daemon.restart" {
        let shutdown_request = shutdown_request.clone();
        tokio::spawn(async move {
            sleep(Duration::from_millis(100)).await;
            let _ = shutdown_request.send(true);
        });
        return Some(json!({ "id": id, "ok": true, "result": { "restarting": true } }).to_string());
    }
    if method == "daemon.update_check" {
        let force = request
            .get("params")
            .and_then(|params| params.get("force"))
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        return Some(match settings.updates().check(force).await {
            Ok(result) => json!({ "id": id, "ok": true, "result": result }).to_string(),
            Err(error) => update_error_reply(id, error),
        });
    }
    if method == "daemon.update_preferences" {
        let params = request.get("params").cloned().unwrap_or_default();
        let automatic_checks = match params.get("automatic_checks") {
            Some(value) => match value.as_bool() {
                Some(enabled) => Some(enabled),
                None => {
                    return Some(json!({
                        "id": id, "ok": false,
                        "error": { "code": "invalid_params", "message": "automatic_checks must be a boolean" }
                    }).to_string());
                }
            },
            None => None,
        };
        let channel = match params.get("channel") {
            Some(value) => {
                match serde_json::from_value::<workman_core::UpdateChannel>(value.clone()) {
                    Ok(channel) => Some(channel),
                    Err(_) => {
                        return Some(json!({
                        "id": id, "ok": false,
                        "error": { "code": "invalid_params", "message": "channel must be stable or latest" }
                    }).to_string());
                    }
                }
            }
            None => None,
        };
        if automatic_checks.is_none() && channel.is_none() {
            return Some(json!({
                "id": id, "ok": false,
                "error": { "code": "invalid_params", "message": "automatic_checks or channel is required" }
            }).to_string());
        }
        return Some(
            match settings
                .updates()
                .set_preferences(automatic_checks, channel)
            {
                Ok(result) => json!({ "id": id, "ok": true, "result": result }).to_string(),
                Err(error) => update_error_reply(id, error),
            },
        );
    }
    if method == "daemon.update_apply" {
        return Some(match settings.updates().install().await {
            Ok(result) => {
                let shutdown_request = shutdown_request.clone();
                tokio::spawn(async move {
                    sleep(Duration::from_millis(150)).await;
                    let _ = shutdown_request.send(true);
                });
                json!({ "id": id, "ok": true, "result": result }).to_string()
            }
            Err(error) => update_error_reply(id, error),
        });
    }
    if matches!(
        method,
        "process.status_subscribe" | "process.status_unsubscribe"
    ) {
        *status_subscribed = method == "process.status_subscribe";
        return Some(
            json!({
                "id": id,
                "ok": true,
                "result": { "subscribed": *status_subscribed }
            })
            .to_string(),
        );
    }
    if method == "terminal.detach" {
        terminal.process_id = None;
        terminal.offset = 0;
        return Some(
            json!({
                "id": id,
                "ok": true,
                "result": { "process_id": null }
            })
            .to_string(),
        );
    }

    let params = request.get("params").cloned().unwrap_or_default();
    let Some(process_id) = params.get("process_id").and_then(serde_json::Value::as_i64) else {
        return Some(
            json!({
                "id": id,
                "ok": false,
                "error": {
                    "code": "invalid_params",
                    "message": "terminal.attach requires an integer process_id"
                }
            })
            .to_string(),
        );
    };
    let offset = params
        .get("offset")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);

    match registry.lock().await.select(process_id) {
        Ok(process) => {
            terminal.process_id = Some(process_id);
            terminal.offset = offset;
            Some(
                json!({
                    "id": id,
                    "ok": true,
                    "result": {
                        "process_id": process_id,
                        "project_id": process.project_id,
                        "offset": offset
                    }
                })
                .to_string(),
            )
        }
        Err(error) => Some(
            json!({
                "id": id,
                "ok": false,
                "error": { "code": error.code(), "message": error.to_string() }
            })
            .to_string(),
        ),
    }
}

fn update_error_reply(id: serde_json::Value, error: workman_core::UpdateError) -> String {
    json!({
        "id": id,
        "ok": false,
        "error": { "code": "update_failed", "message": error.to_string() }
    })
    .to_string()
}

async fn terminal_output_frames(
    registry: &SharedProcessRegistry,
    terminal: &mut TerminalSubscription,
) -> RegistryResult<Vec<Vec<u8>>> {
    let Some(process_id) = terminal.process_id else {
        return Ok(Vec::new());
    };
    let mut frames = Vec::new();
    for _ in 0..TERMINAL_STREAM_CHUNKS_PER_TICK {
        let requested_offset = terminal.offset;
        let chunk = registry.lock().await.raw_output(
            process_id,
            Some(requested_offset),
            TERMINAL_STREAM_CHUNK_BYTES,
        )?;
        terminal.offset = chunk.end_offset;
        if chunk.data.is_empty() {
            break;
        }
        frames.push(encode_terminal_frame(
            process_id,
            chunk.start_offset,
            chunk.start_offset > requested_offset,
            &chunk.data,
        ));
        if chunk.end_offset >= chunk.total_bytes {
            break;
        }
    }
    Ok(frames)
}

fn encode_terminal_frame(
    process_id: workman_core::ProcessId,
    start_offset: u64,
    gap: bool,
    data: &[u8],
) -> Vec<u8> {
    let mut frame = Vec::with_capacity(TERMINAL_FRAME_HEADER_LEN + data.len());
    frame.extend_from_slice(TERMINAL_FRAME_MAGIC);
    frame.extend_from_slice(&process_id.to_be_bytes());
    frame.extend_from_slice(&start_offset.to_be_bytes());
    frame.push(u8::from(gap));
    frame.extend_from_slice(data);
    frame
}

async fn authorize_local_request(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let bearer_is_valid = valid_bearer(request.headers(), &state.token);
    let process_token_is_valid = request.uri().path().starts_with("/mcp")
        && valid_process_token(request.headers(), &state.registry).await;
    if !bearer_is_valid && !process_token_is_valid {
        return (StatusCode::UNAUTHORIZED, "missing or invalid bearer token").into_response();
    }

    if !valid_host_and_origin(request.headers(), state.port) {
        return (StatusCode::FORBIDDEN, "invalid Host or Origin header").into_response();
    }

    next.run(request).await
}

async fn valid_process_token(headers: &HeaderMap, registry: &SharedProcessRegistry) -> bool {
    let Some(token) = headers
        .get(WORKMAN_MCP_TOKEN_HEADER)
        .and_then(|value| value.to_str().ok())
    else {
        return false;
    };
    registry
        .lock()
        .await
        .store()
        .get_process_by_mcp_token(token)
        .is_ok_and(|process| process.is_some())
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

pub fn database_path(data_dir: impl AsRef<Path>) -> PathBuf {
    data_dir.as_ref().join(DATABASE_FILE)
}

/// Resolve the platform data directory, with `WORKMAN_DATA_DIR` as an explicit override.
pub fn default_data_dir() -> PathBuf {
    if let Some(path) = env::var_os("WORKMAN_DATA_DIR") {
        return PathBuf::from(path);
    }
    migration::platform_data_dir("workman")
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

fn registry_io_error(error: impl fmt::Display) -> io::Error {
    io::Error::other(error.to_string())
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
                "workmand exited before becoming ready: {status}"
            )));
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "timed out waiting for workmand discovery",
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
        MaybeTlsStream, WebSocketStream, connect_async,
        tungstenite::{Message, client::IntoClientRequest},
    };

    use super::*;

    struct TestServer {
        discovery: Discovery,
        data_dir: PathBuf,
        registry: SharedProcessRegistry,
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
            let registry = server.registry();
            let (shutdown, receive_shutdown) = oneshot::channel();
            let task = tokio::spawn(server.serve_until(async move {
                let _ = receive_shutdown.await;
            }));

            Self {
                discovery,
                data_dir,
                registry,
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

    async fn rpc(
        socket: &mut WebSocketStream<MaybeTlsStream<TcpStream>>,
        id: u64,
        method: &str,
        params: serde_json::Value,
    ) -> serde_json::Value {
        socket
            .send(Message::Text(
                json!({ "id": id, "method": method, "params": params })
                    .to_string()
                    .into(),
            ))
            .await
            .unwrap();
        let message = socket.next().await.unwrap().unwrap();
        let Message::Text(message) = message else {
            panic!("expected JSON text response, got {message:?}");
        };
        let response: serde_json::Value = serde_json::from_str(&message).unwrap();
        assert_eq!(response["id"], id);
        response
    }

    fn process_params(id: i64, kind: &str, name: &str, command: &str) -> serde_json::Value {
        json!({
            "id": id,
            "project_id": 1,
            "kind": kind,
            "name": name,
            "command": command,
            "working_dir": "/tmp",
            "env": {},
            "auto_start": false,
            "auto_restart": false,
            "restart_when_changed": [],
            "source": "local",
            "trust_hash": null,
            "status": "crashed",
            "pid": null,
            "exit_code": 99,
            "exit_signal": null,
            "exited_at": 1,
            "agent_tool_id": null
        })
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
    async fn project_control_requests_register_list_select_and_rename() {
        let server = TestServer::start().await;
        let first_path = server.data_dir.join("first-project");
        let second_path = server.data_dir.join("second-project");
        std::fs::create_dir(&first_path).unwrap();
        std::fs::create_dir(&second_path).unwrap();
        let (mut socket, _) = connect_async(server.request()).await.unwrap();

        socket
            .send(Message::Text(
                json!({
                    "id": "register-first",
                    "method": "projects.register",
                    "params": { "path": first_path }
                })
                .to_string()
                .into(),
            ))
            .await
            .unwrap();
        let response = receive_json(&mut socket).await;
        assert_eq!(response["ok"], true);
        assert_eq!(response["result"][0]["selected"], true);
        assert_eq!(response["result"][0]["status"], "idle");
        let first_id = response["result"][0]["id"].as_i64().unwrap();

        socket
            .send(Message::Text(
                json!({
                    "id": "rename-first",
                    "method": "projects.rename",
                    "params": { "project_id": first_id, "name": "Frontend lab" }
                })
                .to_string()
                .into(),
            ))
            .await
            .unwrap();
        let response = receive_json(&mut socket).await;
        assert_eq!(response["result"][0]["display_name"], "Frontend lab");

        socket
            .send(Message::Text(
                json!({
                    "id": "settings-first",
                    "method": "projects.update_settings",
                    "params": {
                        "project_id": first_id,
                        "display_name": "Frontend studio",
                        "icon": "code-2",
                        "icon_color": "violet"
                    }
                })
                .to_string()
                .into(),
            ))
            .await
            .unwrap();
        let response = receive_json(&mut socket).await;
        assert_eq!(response["result"][0]["display_name"], "Frontend studio");
        assert_eq!(response["result"][0]["icon"], "code-2");
        assert_eq!(response["result"][0]["icon_color"], "violet");

        socket
            .send(Message::Text(
                json!({
                    "id": "register-second",
                    "method": "projects.register",
                    "params": { "path": second_path }
                })
                .to_string()
                .into(),
            ))
            .await
            .unwrap();
        let response = receive_json(&mut socket).await;
        let second_id = response["result"]
            .as_array()
            .unwrap()
            .iter()
            .find(|project| project["name"] == "second-project")
            .unwrap()["id"]
            .as_i64()
            .unwrap();

        socket
            .send(Message::Text(
                json!({
                    "id": "select-second",
                    "method": "projects.select",
                    "params": { "project_id": second_id }
                })
                .to_string()
                .into(),
            ))
            .await
            .unwrap();
        let response = receive_json(&mut socket).await;
        let selected = response["result"]
            .as_array()
            .unwrap()
            .iter()
            .find(|project| project["selected"] == true)
            .unwrap();
        assert_eq!(selected["id"], second_id);

        socket
            .send(Message::Text(
                json!({
                    "id": "list",
                    "method": "projects.list",
                    "params": {}
                })
                .to_string()
                .into(),
            ))
            .await
            .unwrap();
        let response = receive_json(&mut socket).await;
        assert_eq!(response["result"].as_array().unwrap().len(), 2);

        socket.close(None).await.unwrap();
        server.stop().await;
    }

    #[tokio::test]
    async fn websocket_drives_full_process_lifecycle_and_bulk_commands() {
        let server = TestServer::start().await;
        server
            .registry
            .lock()
            .await
            .store()
            .put_project(&workman_core::Project {
                id: 1,
                path: "/tmp/workman-control-test".into(),
                name: "control-test".into(),
                display_name: None,
                icon: None,
                selected: true,
                sort_order: 0,
            })
            .unwrap();
        let (mut socket, _) = connect_async(server.request()).await.unwrap();

        let long_running = "trap 'exit 0' TERM; printf 'ready\\n'; sleep 30";
        let created = rpc(
            &mut socket,
            1,
            "process.create",
            process_params(101, "command", "dev", long_running),
        )
        .await;
        assert_eq!(created["result"]["status"], "stopped");
        assert!(created["result"]["exit_code"].is_null());

        let started = rpc(
            &mut socket,
            2,
            "process.start",
            json!({ "process_id": 101 }),
        )
        .await;
        assert_eq!(started["result"]["status"], "running");
        assert!(started["result"]["pid"].as_u64().is_some());

        let renamed = rpc(
            &mut socket,
            3,
            "process.rename",
            json!({ "process_id": 101, "name": "web" }),
        )
        .await;
        assert_eq!(renamed["result"]["name"], "web");
        let selected = rpc(
            &mut socket,
            4,
            "process.select",
            json!({ "process_id": 101 }),
        )
        .await;
        assert_eq!(selected["result"]["selected_process_id"], 101);

        let restarted = rpc(
            &mut socket,
            5,
            "process.restart",
            json!({ "process_id": 101 }),
        )
        .await;
        assert_eq!(restarted["result"]["status"], "running");
        let stopped = rpc(&mut socket, 6, "process.stop", json!({ "process_id": 101 })).await;
        assert_eq!(stopped["result"]["status"], "stopped");
        assert!(stopped["result"]["exited_at"].as_i64().is_some());

        let closed = rpc(
            &mut socket,
            7,
            "process.close",
            json!({ "process_id": 101 }),
        )
        .await;
        assert!(closed["ok"].as_bool().unwrap());
        let missing = rpc(&mut socket, 8, "process.get", json!({ "process_id": 101 })).await;
        assert_eq!(missing["error"]["code"], "process_not_found");

        rpc(
            &mut socket,
            9,
            "process.create",
            process_params(102, "agent", "crasher", "exit 7"),
        )
        .await;
        rpc(
            &mut socket,
            10,
            "process.start",
            json!({ "process_id": 102 }),
        )
        .await;
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let crashed = rpc(&mut socket, 11, "process.get", json!({ "process_id": 102 })).await;
            if crashed["result"]["status"] == "crashed" {
                assert_eq!(crashed["result"]["exit_code"], 7);
                assert!(crashed["result"]["exited_at"].as_i64().is_some());
                assert_eq!(crashed["result"]["agent_state"]["state"], "exited");
                break;
            }
            assert!(
                Instant::now() < deadline,
                "process did not report its crash"
            );
            sleep(Duration::from_millis(10)).await;
        }

        rpc(
            &mut socket,
            12,
            "process.create",
            process_params(103, "command", "worker", long_running),
        )
        .await;
        rpc(
            &mut socket,
            13,
            "process.create",
            process_params(104, "terminal", "shell", long_running),
        )
        .await;
        let started_all = rpc(
            &mut socket,
            14,
            "process.start_all_commands",
            json!({ "project_id": 1 }),
        )
        .await;
        assert!(
            started_all["result"]["failures"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            started_all["result"]["processes"].as_array().unwrap().len(),
            1
        );
        assert_eq!(started_all["result"]["processes"][0]["id"], 103);
        assert_eq!(started_all["result"]["processes"][0]["status"], "running");

        let restarted_all = rpc(
            &mut socket,
            15,
            "process.restart_all_commands",
            json!({ "project_id": 1 }),
        )
        .await;
        assert!(
            restarted_all["result"]["failures"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        let stopped_all = rpc(
            &mut socket,
            16,
            "process.stop_all_commands",
            json!({ "project_id": 1 }),
        )
        .await;
        assert_eq!(stopped_all["result"]["processes"][0]["status"], "stopped");

        let listed = rpc(&mut socket, 17, "process.list", json!({ "project_id": 1 })).await;
        let terminal = listed["result"]
            .as_array()
            .unwrap()
            .iter()
            .find(|process| process["id"] == 104)
            .unwrap();
        assert_eq!(terminal["status"], "stopped");
        assert_eq!(terminal["agent_state"]["state"], "exited");

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

    async fn receive_json(
        socket: &mut tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    ) -> serde_json::Value {
        let message = socket.next().await.unwrap().unwrap();
        let Message::Text(text) = message else {
            panic!("expected text response, got {message:?}");
        };
        serde_json::from_str(&text).unwrap()
    }
}
