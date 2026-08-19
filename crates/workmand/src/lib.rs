//! Loopback-only HTTP and WebSocket control server for `workman`.

use std::{
    env, fmt,
    future::{Future, pending},
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
    sync::{Mutex, mpsc, watch},
    time::{Instant, MissedTickBehavior, interval, sleep, timeout},
};
use uuid::Uuid;

mod agent_sessions;
mod command_line;
pub mod config;
mod context_actions;
mod control;
mod coordination;
mod identity;
pub mod lifecycle;
mod mcp;
mod migration;
#[cfg(test)]
mod notification_pipeline_tests;
mod process_registry;
pub mod process_stats;
mod process_tree;
mod profiles;
mod project_titles;
pub mod readiness;
pub mod runtime_doctor;
mod settings;
mod status_invalidation;
mod subprocesses;
mod timer_events;
mod timers;
mod updates;
mod user_config;
mod user_environment;
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
pub use identity::{
    DEV_APP_BUNDLE_NAME, DEV_APPLICATION_NAME, RuntimeIdentity, STABLE_APP_BUNDLE_NAME,
    STABLE_APPLICATION_NAME, identity_from_executable,
};
pub use lifecycle::{
    LifecycleOptions, auto_start_project, spawn_lifecycle_supervisor,
    spawn_lifecycle_supervisor_with_options,
};
pub use mcp::WORKMAN_MCP_TOKEN_HEADER;
pub use migration::migrate_legacy_data_dir;
pub use process_registry::{
    BulkFailure, BulkProcessResult, OUTPUT_DIRECTORY, ProcessInputRouter, ProcessRegistry,
    ProcessStatusView, RegistryError, RegistryResult, WORKMAN_OUTPUT_CAPACITY_ENV,
    output_spill_capacity_from_env,
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
    McpSetupFormat, mcp_connection_info, mcp_connection_info_for,
};
pub use updates::UpdateStatus;
pub use user_config::{
    AgentToolSyncReport, USER_CONFIG_FILE, UserAgentTool, UserConfig, UserConfigError,
    UserTerminalConfig, UserUpdateConfig, WORKMAN_CONFIG_ENV, parse_user_config,
    resolve_update_key, sync_user_agent_tools, sync_user_config_file, user_config_path,
};
pub use user_environment::{
    EnvironmentCaptureMode, ResolvedUserEnvironment, UserEnvironmentInfo, UserEnvironmentResolver,
};
pub use version::{BUILD_ID, BUILD_VERSION, CONTROL_PROTOCOL_VERSION, DaemonVersion};

pub type SharedProcessRegistry = Arc<Mutex<ProcessRegistry>>;

const TERMINAL_FRAME_MAGIC: &[u8; 4] = b"WRK1";
const TERMINAL_FRAME_HEADER_LEN: usize = 21;
const TERMINAL_INPUT_MAGIC: &[u8; 4] = b"WRI1";
const TERMINAL_INPUT_HEADER_LEN: usize = 12;
const TERMINAL_STREAM_CHUNK_BYTES: usize = 64 * 1024;
const TERMINAL_STREAM_CHUNKS_PER_TICK: usize = 4;
const PROCESS_STATUS_STREAM_TICK: Duration = Duration::from_millis(500);
const UPDATE_RESTART_BACKSTOP: Duration = Duration::from_secs(120);

/// The name of the secure daemon discovery file in the workman data directory.
pub const DISCOVERY_FILE: &str = "daemon.json";

/// The private port and bearer credential retained across daemon restarts.
pub const MCP_ENDPOINT_FILE: &str = "mcp-endpoint.json";

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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct PersistentMcpEndpoint {
    port: u16,
    token: String,
}

impl PersistentMcpEndpoint {
    fn endpoint(&self) -> SocketAddr {
        SocketAddr::from((Ipv4Addr::LOCALHOST, self.port))
    }

    fn read(data_dir: &Path) -> io::Result<Option<Self>> {
        let path = mcp_endpoint_path(data_dir);
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        ensure_private_file(&path)?;
        let endpoint: Self = serde_json::from_slice(&bytes).map_err(invalid_data)?;
        if endpoint.port == 0 || endpoint.token.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "{} has an invalid port or token; remove it only if all MCP clients can be reconfigured",
                    path.display()
                ),
            ));
        }
        Ok(Some(endpoint))
    }

    fn publish(&self, data_dir: &Path) -> io::Result<()> {
        let path = mcp_endpoint_path(data_dir);
        let temporary = data_dir.join(format!(
            ".{MCP_ENDPOINT_FILE}.{}.{}.tmp",
            std::process::id(),
            Uuid::new_v4().simple()
        ));
        let bytes = serde_json::to_vec(self).map_err(invalid_data)?;
        write_private_file(&temporary, &bytes)?;
        if let Err(error) = std::fs::rename(&temporary, &path) {
            let _ = std::fs::remove_file(&temporary);
            return Err(error);
        }
        set_private_permissions(&path)?;
        Ok(())
    }
}

/// A bound server. Constructing this value writes its secure discovery file.
pub struct DaemonServer {
    listener: TcpListener,
    discovery: Discovery,
    discovery_guard: DiscoveryGuard,
    registry: SharedProcessRegistry,
    input_router: ProcessInputRouter,
    data_dir: PathBuf,
    started_at: Instant,
    updates: updates::UpdateService,
    user_environment: UserEnvironmentResolver,
}

impl DaemonServer {
    /// Bind only IPv4 loopback and publish this identity's stable port and bearer token.
    pub async fn bind(config: DaemonConfig) -> io::Result<Self> {
        let started_at = Instant::now();
        migration::migrate_default_paths_if_needed(&config.data_dir)?;
        std::fs::create_dir_all(&config.data_dir)?;
        let store = workman_core::Store::open(database_path(&config.data_dir))
            .map_err(registry_io_error)?;
        let user_config_path = user_config_path();
        let user_environment = UserEnvironmentResolver::new(&user_config_path);
        if store
            .active_profile_needs_legacy_config_import()
            .map_err(registry_io_error)?
        {
            sync_user_config_file(&store, &user_config_path).map_err(|error| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("{}: {error}", user_config_path.display()),
                )
            })?;
            let legacy_shell = match std::fs::read_to_string(&user_config_path) {
                Ok(yaml) => {
                    parse_user_config(&yaml)
                        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?
                        .terminal
                        .shell
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => None,
                Err(error) => return Err(error),
            };
            store
                .set_active_profile_terminal_shell(legacy_shell.as_deref())
                .map_err(registry_io_error)?;
            store
                .mark_active_profile_legacy_config_imported()
                .map_err(registry_io_error)?;
        }
        // Shell rc files are user code and can take the full bounded capture deadline. Warm the
        // shared cache on Tokio's blocking pool while daemon binding continues; hot-path resolves
        // serve the inherited environment until the capture is ready.
        user_environment.prewarm();
        let command_environment = user_environment.resolve().command_environment();
        if let Err(error) =
            worktrees::reconcile_existing_projects_with_environment(&store, &command_environment)
        {
            eprintln!("workman daemon: worktree metadata reconciliation skipped: {error}");
        }
        let process_registry = ProcessRegistry::with_output_persistence_and_environment(
            store,
            config.data_dir.join(OUTPUT_DIRECTORY),
            output_spill_capacity_from_env(),
            user_environment.clone(),
        )
        .map_err(registry_io_error)?;
        let input_router = process_registry.input_router();
        let registry = Arc::new(Mutex::new(process_registry));
        // Build the HTTP update client before publishing discovery so readiness never advertises
        // a listener that is still loading platform TLS state.
        let updates = updates::UpdateService::new(&config.data_dir).map_err(io::Error::other)?;
        let persisted = PersistentMcpEndpoint::read(&config.data_dir).map_err(|error| {
            io::Error::new(
                error.kind(),
                format!(
                    "could not read persistent MCP endpoint {}: {error}",
                    mcp_endpoint_path(&config.data_dir).display()
                ),
            )
        })?;
        // During the first upgrade to persistent endpoints, a prior daemon generation may still
        // have left valid discovery behind (for example while its live MCP connection drains).
        // Adopt that address and credential once so the upgrade itself can be seamless too.
        let persisted = persisted.or_else(|| {
            Discovery::read(&config.data_dir)
                .ok()
                .map(|discovery| PersistentMcpEndpoint {
                    port: discovery.port,
                    token: discovery.token,
                })
        });
        let requested_port = if config.port == 0 {
            persisted.as_ref().map_or(0, |endpoint| endpoint.port)
        } else {
            config.port
        };
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, requested_port))
            .await
            .map_err(|error| {
                let source = if config.port != 0 {
                    "configured"
                } else if persisted.is_some() {
                    "persisted"
                } else {
                    "ephemeral"
                };
                io::Error::new(
                    error.kind(),
                    format!(
                        "could not bind {source} Workman MCP port {requested_port} on 127.0.0.1: {error}; another identity or process may be using it. Stop that process or explicitly choose a free --port (the daemon will persist the override in {})",
                        mcp_endpoint_path(&config.data_dir).display()
                    ),
                )
            })?;
        let port = listener.local_addr()?.port();
        let token = persisted.map_or_else(new_token, |endpoint| endpoint.token);
        PersistentMcpEndpoint {
            port,
            token: token.clone(),
        }
        .publish(&config.data_dir)
        .map_err(|error| {
            io::Error::new(
                error.kind(),
                format!(
                    "could not persist stable Workman MCP endpoint {}: {error}",
                    mcp_endpoint_path(&config.data_dir).display()
                ),
            )
        })?;
        let discovery = Discovery {
            port,
            token,
            pid: std::process::id(),
        };
        let discovery_guard = DiscoveryGuard::publish(&config.data_dir, &discovery)?;

        Ok(Self {
            listener,
            discovery,
            discovery_guard,
            registry,
            input_router,
            data_dir: config.data_dir,
            started_at,
            updates,
            user_environment,
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
        let status_invalidations = self.registry.lock().await.status_invalidations();
        let lifecycle_task =
            spawn_lifecycle_supervisor(self.registry.clone(), shutdown_rx.clone())?;
        let timer_events = timer_events::TimerLifecycleHub::new(status_invalidations.clone());
        let worktree_operations =
            worktree_operations::WorktreeOperationHub::new(status_invalidations.clone());
        worktree_operations::resume_interrupted_removals(&self.registry, &worktree_operations)
            .await
            .map_err(|error| {
                io::Error::other(format!(
                    "could not reconcile interrupted project removals ({}): {}",
                    error.code, error.message
                ))
            })?;
        let timer_task = timers::spawn_timer_scheduler(
            self.registry.clone(),
            timer_events.clone(),
            shutdown_rx.clone(),
        );
        let live_stats = process_stats::LiveStatsHub::new(status_invalidations.clone());
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
            self.user_environment,
        );
        let state = AppState {
            token: self.discovery.token.clone(),
            port: self.discovery.port,
            shutdown: shutdown_rx.clone(),
            shutdown_request: shutdown_tx.clone(),
            settings: runtime_settings,
            registry: self.registry,
            input_router: self.input_router,
            status_invalidations,
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
    input_router: ProcessInputRouter,
    status_invalidations: status_invalidation::StatusInvalidationHub,
    live_stats: process_stats::LiveStatsHub,
    timer_events: timer_events::TimerLifecycleHub,
    worktree_operations: worktree_operations::WorktreeOperationHub,
}

fn router(state: AppState) -> Router {
    let mcp_url = format!("http://127.0.0.1:{}/mcp", state.port);
    let (mcp_service, mcp_sessions) = mcp::streamable_http_service(
        state.registry.clone(),
        state.input_router.clone(),
        mcp_url.clone(),
        state.timer_events.clone(),
    );
    let stateless_mcp_service = mcp::stateless_http_service(
        state.registry.clone(),
        state.input_router.clone(),
        mcp_url,
        state.timer_events.clone(),
    );
    Router::new()
        .route("/health", get(health))
        .route("/ws", get(ws_upgrade))
        .nest_service("/mcp-stateless", stateless_mcp_service)
        .nest_service("/mcp", mcp_service)
        .fallback(|| async { StatusCode::NOT_FOUND })
        .layer(middleware::from_fn_with_state(
            mcp_sessions,
            mcp::require_known_session,
        ))
        .layer(middleware::from_fn_with_state(
            state.status_invalidations.clone(),
            invalidate_status_after_mcp_request,
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
            state.input_router,
            state.status_invalidations,
            state.live_stats,
            state.timer_events,
            state.worktree_operations,
        )
    })
}

#[allow(clippy::too_many_arguments)]
async fn control_session(
    mut socket: WebSocket,
    mut shutdown: watch::Receiver<bool>,
    shutdown_request: watch::Sender<bool>,
    settings: settings::DaemonRuntimeSettings,
    registry: SharedProcessRegistry,
    input_router: ProcessInputRouter,
    status_invalidations: status_invalidation::StatusInvalidationHub,
    live_stats: process_stats::LiveStatsHub,
    timer_events: timer_events::TimerLifecycleHub,
    worktree_operations: worktree_operations::WorktreeOperationHub,
) {
    let mcp_url = settings.info().mcp.endpoint;
    let mut terminal = TerminalSubscription::default();
    let concurrent_invalidations = status_invalidations.clone();
    let mut status_subscription =
        ProcessStatusSubscription::new(live_stats.clone(), status_invalidations);
    let mut update_progress_subscription = UpdateProgressSubscription::default();
    let mut update_progress = settings.updates().subscribe_progress();
    let (control_request_tx, mut control_request_rx) = mpsc::unbounded_channel::<String>();
    let (control_response_tx, mut control_response_rx) = mpsc::unbounded_channel::<String>();
    let control_registry = registry.clone();
    let control_input_router = input_router.clone();
    let control_settings = settings.clone();
    let control_shutdown_request = shutdown_request.clone();
    let control_worktree_operations = worktree_operations.clone();
    let control_timer_events = timer_events.clone();
    let control_invalidations = concurrent_invalidations.clone();
    let control_live_stats = live_stats.clone();
    let control_mcp_url = mcp_url.clone();
    let control_task = tokio::spawn(async move {
        // Preserve control-request ordering on its own lane. Slow filesystem, Git, network,
        // readiness, and update work can queue ordinary RPCs, but can never occupy the socket
        // pump responsible for terminal input, terminal output, or heartbeats.
        let mut detached_terminal = TerminalSubscription::default();
        let mut detached_status =
            ProcessStatusSubscription::new(control_live_stats, control_invalidations.clone());
        let mut detached_update_progress = UpdateProgressSubscription::default();
        while let Some(text) = control_request_rx.recv().await {
            let response = match handle_session_control(
                &text,
                &control_registry,
                &control_input_router,
                &control_settings,
                &control_shutdown_request,
                &mut detached_terminal,
                &mut detached_status,
                &mut detached_update_progress,
                &control_worktree_operations,
            )
            .await
            {
                Some(response) => response,
                None => {
                    control::handle_text(
                        &text,
                        &control_registry,
                        &control_input_router,
                        &control_mcp_url,
                        control_settings.data_dir(),
                        &control_timer_events,
                    )
                    .await
                }
            };
            control_invalidations.invalidate();
            if control_response_tx.send(response).is_err() {
                break;
            }
        }
    });
    let mut timer_event_cursor = timer_events.latest_sequence();
    let mut status_tick = interval(PROCESS_STATUS_STREAM_TICK);
    status_tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
    status_tick.tick().await;

    'session: loop {
        let output_ready = terminal.output_ready();
        tokio::pin!(output_ready);
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
                            if !request_requires_session_state(&text) {
                                if control_request_tx.send(text.to_string()).is_err() {
                                    break;
                                }
                                continue;
                            }
                            let response = match handle_session_control(
                                &text,
                                &registry,
                                &input_router,
                                &settings,
                                &shutdown_request,
                                &mut terminal,
                                &mut status_subscription,
                                &mut update_progress_subscription,
                                &worktree_operations,
                            ).await {
                                Some(response) => response,
                                // Raw keystrokes are the only non-session request executed in
                                // this loop. Their registry-free router is deliberately bounded
                                // to a PTY writer lock and cannot inherit another RPC's latency.
                                None => control::handle_text(
                                    &text,
                                    &registry,
                                    &input_router,
                                    &mcp_url,
                                    settings.data_dir(),
                                    &timer_events,
                                ).await,
                            };
                            status_subscription.status_invalidations.invalidate();
                            Message::Text(response.into())
                        } else {
                            Message::Text(json!({
                                "type": "error",
                                "error": "text frames must contain valid JSON"
                            }).to_string().into())
                        }
                    }
                    Message::Binary(bytes) => {
                        if let Some((process_id, data)) = decode_terminal_input_frame(&bytes) {
                            match input_router.send_input(process_id, data) {
                                Ok(_) => continue,
                                Err(error) => Message::Text(
                                    json!({
                                        "event": "terminal.error",
                                        "process_id": process_id,
                                        "error": {
                                            "code": error.code(),
                                            "message": error.to_string()
                                        }
                                    })
                                    .to_string()
                                    .into(),
                                ),
                            }
                        } else {
                            Message::Binary(bytes)
                        }
                    }
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
            Some(response) = control_response_rx.recv() => {
                if socket.send(Message::Text(response.into())).await.is_err() {
                    break;
                }
            }
            progress = update_progress.recv() => {
                match progress {
                    Ok(progress) if update_progress_subscription.accepts(&progress) => {
                        let event = json!({
                            "event": "daemon.update_progress",
                            "request_id": progress.request_id,
                            "progress": progress.progress,
                        });
                        if socket.send(Message::Text(event.to_string().into())).await.is_err() {
                            break;
                        }
                    }
                    Ok(_) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
            _ = &mut output_ready => {
                match terminal_output_frames(&mut terminal) {
                    Ok(frames) => {
                        for frame in frames {
                            if socket.send(Message::Binary(frame.into())).await.is_err() {
                                break 'session;
                            }
                        }
                    }
                    Err(error) => {
                        let process_id = terminal.process_id;
                        terminal.detach();
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
            _ = status_tick.tick(), if status_subscription.subscribed => {
                let Some(status_version) =
                    status_subscription.pending_version(timers::now_millis())
                else {
                    continue;
                };
                let (statuses, timers, projects) = {
                    // Status telemetry is best-effort. Never make the terminal socket wait for
                    // an unrelated lifecycle mutation that currently owns the registry.
                    let Ok(mut registry) = registry.try_lock() else {
                        continue;
                    };
                    let statuses = registry.list_statuses(None);
                    let timers = timers::TimerService::new(&mut registry)
                        .list_active(timers::now_millis());
                    let projects = registry.store().list_projects();
                    (statuses, timers, projects)
                };
                if let (Ok(processes), Ok(timers), Ok(projects)) = (statuses, timers, projects) {
                    status_subscription.last_version = Some(status_version);
                    let stats = live_stats.snapshot().await;
                    let (latest_timer_event, lifecycle_events) =
                        timer_events.events_since(timer_event_cursor);
                    timer_event_cursor = latest_timer_event;
                    let worktree_operations = worktree_operations.snapshot_reconciled(&projects);
                    let event = json!({
                        "event": "process.statuses",
                        "processes": processes,
                        "stats": stats,
                        "timers": timers,
                        "timer_events": lifecycle_events,
                        "worktree_operations": worktree_operations,
                    });
                    if let Some(event) = status_event_if_changed(&mut status_subscription.last_event, event)
                        && socket.send(Message::Text(event.into())).await.is_err()
                    {
                        break;
                    }
                }
            }
        }
    }
    control_task.abort();
}

fn request_requires_session_state(text: &str) -> bool {
    let Ok(request) = serde_json::from_str::<serde_json::Value>(text) else {
        return true;
    };
    let Some(method) = request.get("method").and_then(serde_json::Value::as_str) else {
        return true;
    };
    if method == "process.send_input" {
        return !request
            .get("params")
            .and_then(|params| params.get("submit"))
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
    }
    matches!(
        method,
        "daemon.hello"
            | "daemon.info"
            | "daemon.restart"
            | "terminal.attach"
            | "terminal.detach"
            | "process.status_subscribe"
            | "process.status_unsubscribe"
            | "daemon.update_progress_subscribe"
            | "daemon.update_progress_unsubscribe"
    )
}

#[derive(Default)]
struct TerminalSubscription {
    process_id: Option<workman_core::ProcessId>,
    output: Option<workman_core::pty::RawOutput>,
    terminal_output: Option<workman_core::terminal::TerminalOutput>,
    offset: u64,
    replay_end_offset: u64,
}

impl TerminalSubscription {
    fn detach(&mut self) {
        self.process_id = None;
        self.output = None;
        self.terminal_output = None;
        self.offset = 0;
        self.replay_end_offset = 0;
    }

    fn output_ready(&self) -> impl Future<Output = ()> + Send + 'static {
        let output = self.output.clone();
        let offset = self.offset;
        async move {
            let Some(output) = output else {
                pending::<()>().await;
                return;
            };
            // Register first, then inspect the condition. If output lands between these two
            // operations the listener is notified; if it landed earlier the offset check wins.
            let listener = output.listen();
            if output.total_bytes_seen() <= offset {
                listener.await;
            }
        }
    }
}

struct ProcessStatusSubscription {
    subscribed: bool,
    live_stats: process_stats::LiveStatsHub,
    status_invalidations: status_invalidation::StatusInvalidationHub,
    live_stats_client: Option<process_stats::LiveStatsClientGuard>,
    last_version: Option<u64>,
    last_event: Option<String>,
}

#[derive(Default)]
struct UpdateProgressSubscription {
    request_id: Option<String>,
}

impl UpdateProgressSubscription {
    fn accepts(&self, progress: &updates::UpdateProgressEvent) -> bool {
        self.request_id.as_deref() == Some(progress.request_id.as_str())
    }
}

impl ProcessStatusSubscription {
    fn new(
        live_stats: process_stats::LiveStatsHub,
        status_invalidations: status_invalidation::StatusInvalidationHub,
    ) -> Self {
        Self {
            subscribed: false,
            live_stats,
            status_invalidations,
            live_stats_client: None,
            last_version: None,
            last_event: None,
        }
    }

    fn set_subscribed(&mut self, subscribed: bool) {
        if subscribed && !self.subscribed {
            self.last_event = None;
            self.last_version = None;
        }
        self.subscribed = subscribed;
        if subscribed {
            self.live_stats_client
                .get_or_insert_with(|| self.live_stats.client_connected());
        } else {
            self.live_stats_client.take();
        }
    }

    fn pending_version(&self, now: i64) -> Option<u64> {
        let version = self.status_invalidations.version_at(now);
        (self.last_version != Some(version)).then_some(version)
    }
}

#[allow(clippy::too_many_arguments)]
async fn handle_session_control(
    text: &str,
    registry: &SharedProcessRegistry,
    input_router: &ProcessInputRouter,
    settings: &settings::DaemonRuntimeSettings,
    shutdown_request: &watch::Sender<bool>,
    terminal: &mut TerminalSubscription,
    status_subscription: &mut ProcessStatusSubscription,
    update_progress_subscription: &mut UpdateProgressSubscription,
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
            | "daemon.update_progress_subscribe"
            | "daemon.update_progress_unsubscribe"
            | "worktree.create_async"
            | "worktree.fork_async"
            | "worktree.adopt_async"
            | "worktree.remove_async"
            | "worktree.operation_dismiss"
    ) {
        return None;
    }

    let id = request.get("id").cloned().unwrap_or_default();
    if method == "worktree.operation_dismiss" {
        let params = request.get("params").cloned().unwrap_or_default();
        return Some(
            match worktree_operations::dismiss(params, worktree_operations) {
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
    if matches!(
        method,
        "worktree.create_async"
            | "worktree.fork_async"
            | "worktree.adopt_async"
            | "worktree.remove_async"
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
        let params = request.get("params").cloned().unwrap_or_default();
        let force = params
            .get("force")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        let key =
            match params.get("key") {
                Some(value) => match value.as_str() {
                    Some(key) => Some(key),
                    None => {
                        return Some(json!({
                        "id": id, "ok": false,
                        "error": { "code": "invalid_params", "message": "key must be a string" }
                    }).to_string());
                    }
                },
                None => None,
            };
        let result = match key {
            Some(key) => settings.updates().check_with_key(force, Some(key)).await,
            None => settings.updates().check(force).await,
        };
        return Some(match result {
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
    if method == "daemon.update_progress_subscribe" {
        let request_id = request
            .get("params")
            .and_then(|params| params.get("request_id"))
            .and_then(serde_json::Value::as_str);
        let Some(request_id) = request_id.filter(|value| !value.is_empty() && value.len() <= 128)
        else {
            return Some(
                json!({
                    "id": id,
                    "ok": false,
                    "error": {
                        "code": "invalid_params",
                        "message": "request_id must be a non-empty string of at most 128 bytes"
                    }
                })
                .to_string(),
            );
        };
        update_progress_subscription.request_id = Some(request_id.to_owned());
        return Some(json!({ "id": id, "ok": true, "result": { "subscribed": true } }).to_string());
    }
    if method == "daemon.update_progress_unsubscribe" {
        update_progress_subscription.request_id = None;
        return Some(
            json!({ "id": id, "ok": true, "result": { "subscribed": false } }).to_string(),
        );
    }
    if method == "daemon.update_apply" {
        let params = request.get("params").cloned().unwrap_or_default();
        let key =
            match params.get("key") {
                Some(value) => match value.as_str() {
                    Some(key) => Some(key),
                    None => {
                        return Some(json!({
                        "id": id, "ok": false,
                        "error": { "code": "invalid_params", "message": "key must be a string" }
                    }).to_string());
                    }
                },
                None => None,
            };
        let request_id = match params.get("request_id") {
            Some(value) => match value.as_str() {
                Some(value) if !value.is_empty() && value.len() <= 128 => Some(value.to_owned()),
                _ => {
                    return Some(
                        json!({
                            "id": id,
                            "ok": false,
                            "error": {
                                "code": "invalid_params",
                                "message": "request_id must be a non-empty string of at most 128 bytes"
                            }
                        })
                        .to_string(),
                    );
                }
            },
            None => None,
        };
        let result = match (key, request_id) {
            (Some(key), Some(request_id)) => {
                settings
                    .updates()
                    .install_with_key_for(Some(key), Some(request_id))
                    .await
            }
            (None, Some(request_id)) => {
                settings
                    .updates()
                    .install_with_key_for(None, Some(request_id))
                    .await
            }
            (Some(key), None) => settings.updates().install_with_key(Some(key)).await,
            (None, None) => settings.updates().install().await,
        };
        return Some(match result {
            Ok(result) => {
                // Installation no longer races a fixed reply-then-shutdown timer. The report's
                // restart plan transfers ownership to the caller: desktop first presents the
                // installed state, then its native bridge stops the daemon and relaunches the
                // refreshed bundle; `wrk update` explicitly requests daemon.restart.
                // If that ownership transfer is interrupted, the old daemon still exits within
                // a bounded window instead of running forever against replaced binaries.
                schedule_update_restart_backstop(shutdown_request.clone());
                json!({ "id": id, "ok": true, "result": result }).to_string()
            }
            Err(error) => update_error_reply(id, error),
        });
    }
    if matches!(
        method,
        "process.status_subscribe" | "process.status_unsubscribe"
    ) {
        status_subscription.set_subscribed(method == "process.status_subscribe");
        return Some(
            json!({
                "id": id,
                "ok": true,
                "result": { "subscribed": status_subscription.subscribed }
            })
            .to_string(),
        );
    }
    if method == "terminal.detach" {
        terminal.detach();
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

    let (process, output, terminal_output, replay_start_offset, replay_end_offset) =
        match input_router.terminal_attachment(process_id, offset) {
            Ok(attachment) => {
                // Selection is ephemeral UI bookkeeping. Keep it best-effort rather than allowing
                // a lifecycle operation to delay terminal output reattachment.
                if let Ok(mut registry) = registry.try_lock() {
                    let _ = registry.select(process_id);
                }
                (
                    attachment.process,
                    Some(attachment.raw_output),
                    Some(attachment.terminal_output),
                    attachment.replay_start_offset,
                    attachment.replay_end_offset,
                )
            }
            Err(RegistryError::NotRunning(_)) => {
                // Persisted output for stopped processes has no live I/O-router entry. Preserve
                // the existing replay behavior; only genuinely live PTYs need the bounded path.
                let mut registry = registry.lock().await;
                let process = match registry.select(process_id) {
                    Ok(process) => process,
                    Err(error) => {
                        return Some(
                            json!({
                                "id": id,
                                "ok": false,
                                "error": { "code": error.code(), "message": error.to_string() }
                            })
                            .to_string(),
                        );
                    }
                };
                let replay = match registry.raw_output(process_id, Some(offset), 0) {
                    Ok(replay) => replay,
                    Err(error) => {
                        return Some(
                            json!({
                                "id": id,
                                "ok": false,
                                "error": { "code": error.code(), "message": error.to_string() }
                            })
                            .to_string(),
                        );
                    }
                };
                let output = match registry.raw_output_source(process_id) {
                    Ok(output) => output,
                    Err(error) => {
                        return Some(
                            json!({
                                "id": id,
                                "ok": false,
                                "error": { "code": error.code(), "message": error.to_string() }
                            })
                            .to_string(),
                        );
                    }
                };
                let terminal_output = match registry.terminal_output_source(process_id) {
                    Ok(output) => output,
                    Err(error) => {
                        return Some(
                            json!({
                                "id": id,
                                "ok": false,
                                "error": { "code": error.code(), "message": error.to_string() }
                            })
                            .to_string(),
                        );
                    }
                };
                (
                    process,
                    output,
                    terminal_output,
                    replay.start_offset,
                    replay.total_bytes,
                )
            }
            Err(error) => {
                return Some(
                    json!({
                        "id": id,
                        "ok": false,
                        "error": { "code": error.code(), "message": error.to_string() }
                    })
                    .to_string(),
                );
            }
        };
    let project_id = process.project_id;
    let focus_reporting = terminal_output
        .as_ref()
        .is_some_and(|output| output.is_focus_reporting());
    let keyboard_protocol = terminal_output
        .as_ref()
        .map(|output| output.keyboard_protocol())
        .unwrap_or_default();
    terminal.process_id = Some(process_id);
    terminal.output = output;
    terminal.terminal_output = terminal_output;
    // The raw-output snapshot clamps stale or future offsets to the retained stream. Start the
    // readiness cursor at that effective offset; otherwise a client-supplied offset past the
    // current end could wait forever for bytes that were never part of the replay.
    terminal.offset = replay_start_offset;
    terminal.replay_end_offset = replay_end_offset;
    Some(
        json!({
            "id": id,
            "ok": true,
            "result": {
                "process_id": process_id,
                "project_id": project_id,
                "offset": offset,
                "replay_start_offset": replay_start_offset,
                "replay_end_offset": replay_end_offset,
                "focus_reporting": focus_reporting,
                "keyboard_protocol": {
                    "kitty_flags": keyboard_protocol.kitty_flags,
                    "modify_other_keys": keyboard_protocol.modify_other_keys
                }
            }
        })
        .to_string(),
    )
}

fn schedule_update_restart_backstop(shutdown_request: watch::Sender<bool>) {
    schedule_update_restart_backstop_after(shutdown_request, UPDATE_RESTART_BACKSTOP);
}

fn schedule_update_restart_backstop_after(shutdown_request: watch::Sender<bool>, delay: Duration) {
    tokio::spawn(async move {
        sleep(delay).await;
        let _ = shutdown_request.send(true);
    });
}

fn status_event_if_changed(
    previous: &mut Option<String>,
    event: serde_json::Value,
) -> Option<String> {
    let event = event.to_string();
    if previous.as_deref() == Some(event.as_str()) {
        return None;
    }
    *previous = Some(event.clone());
    Some(event)
}

fn update_error_reply(id: serde_json::Value, error: workman_core::UpdateError) -> String {
    json!({
        "id": id,
        "ok": false,
        "error": { "code": "update_failed", "message": error.to_string() }
    })
    .to_string()
}

fn terminal_output_frames(terminal: &mut TerminalSubscription) -> RegistryResult<Vec<Vec<u8>>> {
    let Some(process_id) = terminal.process_id else {
        return Ok(Vec::new());
    };
    let output = terminal
        .output
        .as_ref()
        .ok_or(RegistryError::NotRunning(process_id))?;
    let mut frames = Vec::new();
    for _ in 0..TERMINAL_STREAM_CHUNKS_PER_TICK {
        let requested_offset = terminal.offset;
        let replay_bytes_remaining = terminal.replay_end_offset.saturating_sub(requested_offset);
        let max_bytes = if replay_bytes_remaining > 0 {
            usize::try_from(replay_bytes_remaining)
                .unwrap_or(usize::MAX)
                .min(TERMINAL_STREAM_CHUNK_BYTES)
        } else {
            TERMINAL_STREAM_CHUNK_BYTES
        };
        let chunk = output.read(Some(requested_offset), max_bytes);
        let keyboard_protocol = terminal
            .terminal_output
            .as_ref()
            .map(workman_core::terminal::TerminalOutput::keyboard_protocol)
            .unwrap_or_default();
        terminal.offset = chunk.end_offset;
        if chunk.data.is_empty() {
            break;
        }
        frames.push(encode_terminal_frame(
            process_id,
            chunk.start_offset,
            chunk.start_offset > requested_offset,
            keyboard_protocol,
            &chunk.data,
        ));
        // Keep retained replay and newly produced live output in separate xterm writes. The
        // frontend only enables protocol replies after the replay-ending write has parsed.
        if requested_offset < terminal.replay_end_offset
            && chunk.end_offset >= terminal.replay_end_offset
        {
            break;
        }
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
    keyboard_protocol: workman_core::terminal::TerminalKeyboardProtocol,
    data: &[u8],
) -> Vec<u8> {
    let mut frame = Vec::with_capacity(TERMINAL_FRAME_HEADER_LEN + data.len());
    frame.extend_from_slice(TERMINAL_FRAME_MAGIC);
    frame.extend_from_slice(&process_id.to_be_bytes());
    frame.extend_from_slice(&start_offset.to_be_bytes());
    let flags = u8::from(gap)
        | ((keyboard_protocol.kitty_flags & 1) << 1)
        | ((keyboard_protocol.modify_other_keys & 3) << 2);
    frame.push(flags);
    frame.extend_from_slice(data);
    frame
}

fn decode_terminal_input_frame(bytes: &[u8]) -> Option<(workman_core::ProcessId, &[u8])> {
    if bytes.len() < TERMINAL_INPUT_HEADER_LEN || &bytes[..4] != TERMINAL_INPUT_MAGIC {
        return None;
    }
    let process_id = i64::from_be_bytes(bytes[4..12].try_into().ok()?);
    Some((process_id, &bytes[TERMINAL_INPUT_HEADER_LEN..]))
}

async fn authorize_local_request(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let bearer_is_valid = valid_bearer(request.headers(), &state.token);
    let process_token_is_valid = is_mcp_request_path(request.uri().path())
        && valid_process_token(request.headers(), &state.registry).await;
    if !bearer_is_valid && !process_token_is_valid {
        return (StatusCode::UNAUTHORIZED, "missing or invalid bearer token").into_response();
    }

    if !valid_host_and_origin(request.headers(), state.port) {
        return (StatusCode::FORBIDDEN, "invalid Host or Origin header").into_response();
    }

    next.run(request).await
}

async fn invalidate_status_after_mcp_request(
    State(invalidations): State<status_invalidation::StatusInvalidationHub>,
    request: Request,
    next: Next,
) -> Response {
    let invalidates =
        request.method() == axum::http::Method::POST && is_mcp_request_path(request.uri().path());
    let response = next.run(request).await;
    if invalidates {
        // MCP tools mutate several status-adjacent store tables directly. Conservatively
        // coalesce one dirty edge after a completed request; idle connections do no work.
        invalidations.invalidate();
    }
    response
}

fn is_mcp_request_path(path: &str) -> bool {
    path == "/mcp"
        || path.starts_with("/mcp/")
        || path == "/mcp-stateless"
        || path.starts_with("/mcp-stateless/")
}

async fn valid_process_token(headers: &HeaderMap, registry: &SharedProcessRegistry) -> bool {
    let token = headers
        .get(WORKMAN_MCP_TOKEN_HEADER)
        .and_then(|value| value.to_str().ok())
        .or_else(|| {
            headers
                .get(header::AUTHORIZATION)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.strip_prefix("Bearer "))
        });
    let Some(token) = token else {
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

pub fn mcp_endpoint_path(data_dir: impl AsRef<Path>) -> PathBuf {
    data_dir.as_ref().join(MCP_ENDPOINT_FILE)
}

pub fn database_path(data_dir: impl AsRef<Path>) -> PathBuf {
    data_dir.as_ref().join(DATABASE_FILE)
}

/// Resolve the platform data directory, with `WORKMAN_DATA_DIR` as an explicit override.
pub fn default_data_dir() -> PathBuf {
    if let Some(path) = env::var_os("WORKMAN_DATA_DIR") {
        return PathBuf::from(path);
    }
    migration::platform_data_dir(RuntimeIdentity::current().application_name())
}

struct DiscoveryGuard {
    path: PathBuf,
    token: String,
    pid: u32,
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
            pid: discovery.pid,
        })
    }
}

impl Drop for DiscoveryGuard {
    fn drop(&mut self) {
        // The bearer token is intentionally stable now, so PID is the generation marker that
        // prevents an old daemon from removing a replacement daemon's discovery record.
        let ours = std::fs::read(&self.path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Discovery>(&bytes).ok())
            .is_some_and(|discovery| discovery.token == self.token && discovery.pid == self.pid);
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
    #[cfg(not(unix))]
    let _ = path;
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
    #[cfg(not(unix))]
    let _ = path;
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

/// Keeps this process's standard handles out of a spawned child's handle table.
///
/// Windows copies every inheritable parent handle into a child created with
/// explicit standard handles, so a shell pipeline reading this process's output
/// would otherwise wait on the long-lived daemon even though the daemon's own
/// stdio is null. Prior inheritance flags are restored on drop.
#[cfg(windows)]
struct StdHandleInheritanceGuard {
    restore: Vec<windows_sys::Win32::Foundation::HANDLE>,
}

#[cfg(windows)]
impl StdHandleInheritanceGuard {
    fn disable() -> Self {
        use windows_sys::Win32::Foundation::{
            GetHandleInformation, HANDLE_FLAG_INHERIT, SetHandleInformation,
        };
        use windows_sys::Win32::System::Console::{
            GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
        };

        let mut restore = Vec::new();
        // SAFETY: standard pseudo-handles are process-owned, and toggling the
        // inherit flag only changes what future children may receive.
        unsafe {
            for std_handle in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
                let handle = GetStdHandle(std_handle);
                let mut flags = 0_u32;
                if handle.is_null() || GetHandleInformation(handle, &mut flags) == 0 {
                    continue;
                }
                if flags & HANDLE_FLAG_INHERIT != 0
                    && SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0) != 0
                {
                    restore.push(handle);
                }
            }
        }
        Self { restore }
    }
}

#[cfg(windows)]
impl Drop for StdHandleInheritanceGuard {
    fn drop(&mut self) {
        use windows_sys::Win32::Foundation::{HANDLE_FLAG_INHERIT, SetHandleInformation};

        for handle in self.restore.drain(..) {
            // SAFETY: the handle was valid when captured and standard handles
            // live for the process; this only re-enables future inheritance.
            let _ =
                unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT) };
        }
    }
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

    if let Some(endpoint) = PersistentMcpEndpoint::read(data_dir)?
        && TcpStream::connect(endpoint.endpoint()).await.is_ok()
    {
        return Err(io::Error::new(
            io::ErrorKind::AddrInUse,
            format!(
                "persisted Workman MCP port {} from {} is already in use, but no authenticated daemon discovery is available; stop the occupying process or run workmand --data-dir {} --port PORT once with a free port",
                endpoint.port,
                mcp_endpoint_path(data_dir).display(),
                data_dir.display()
            ),
        ));
    }

    #[cfg(windows)]
    let inheritance_guard = StdHandleInheritanceGuard::disable();
    let child = Command::new(daemon_executable.as_ref())
        .arg("--data-dir")
        .arg(data_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    #[cfg(windows)]
    drop(inheritance_guard);
    let mut child = child?;
    let child_pid = child.id();
    let deadline = Instant::now() + wait_timeout;

    loop {
        if let Ok(discovery) = Discovery::read(data_dir)
            && probe(&discovery).await
        {
            return Ok(discovery);
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                clean_failed_spawn(&mut child, child_pid, data_dir).await;
                return Err(io::Error::other(format!(
                    "workmand exited before becoming ready: {status}"
                )));
            }
            Ok(None) => {}
            Err(error) => {
                clean_failed_spawn(&mut child, child_pid, data_dir).await;
                return Err(error);
            }
        }
        if Instant::now() >= deadline {
            clean_failed_spawn(&mut child, child_pid, data_dir).await;
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "timed out waiting for workmand discovery",
            ));
        }
        sleep(Duration::from_millis(50)).await;
    }
}

async fn clean_failed_spawn(child: &mut tokio::process::Child, pid: Option<u32>, data_dir: &Path) {
    if child.try_wait().ok().flatten().is_none() {
        let _ = child.kill().await;
    }
    if let Some(pid) = pid
        && Discovery::read(data_dir).is_ok_and(|discovery| discovery.pid == pid)
    {
        let _ = std::fs::remove_file(discovery_path(data_dir));
    }
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use std::time::Duration;

    use futures_util::{SinkExt, StreamExt};
    use tempfile::TempDir;
    use tokio::sync::oneshot;
    use tokio_tungstenite::{
        MaybeTlsStream, WebSocketStream, connect_async,
        tungstenite::{Message, client::IntoClientRequest},
    };

    use super::*;

    #[test]
    fn only_connection_local_controls_and_raw_input_stay_on_the_socket_pump() {
        for method in [
            "daemon.hello",
            "daemon.info",
            "daemon.restart",
            "terminal.attach",
            "terminal.detach",
            "process.status_subscribe",
            "process.status_unsubscribe",
            "daemon.update_progress_subscribe",
            "daemon.update_progress_unsubscribe",
        ] {
            assert!(request_requires_session_state(
                &json!({ "id": 1, "method": method, "params": {} }).to_string()
            ));
        }
        assert!(request_requires_session_state(
            &json!({
                "id": 1,
                "method": "process.send_input",
                "params": { "process_id": 7, "data": "eA==" }
            })
            .to_string()
        ));
        for method in [
            "process.wait_for_bound_port",
            "worktree.list",
            "daemon.update_check",
            "agents.spawn",
        ] {
            assert!(!request_requires_session_state(
                &json!({ "id": 1, "method": method, "params": {} }).to_string()
            ));
        }
        assert!(!request_requires_session_state(
            &json!({
                "id": 1,
                "method": "process.send_input",
                "params": { "process_id": 7, "data": "eA==", "submit": true }
            })
            .to_string()
        ));
    }

    #[test]
    fn update_progress_subscription_accepts_only_its_correlated_request() {
        let subscription = UpdateProgressSubscription {
            request_id: Some("desktop-update-1".to_owned()),
        };
        let matching = updates::UpdateProgressEvent {
            request_id: "desktop-update-1".to_owned(),
            progress: workman_core::UpdateProgress::stage(
                workman_core::UpdateStage::Downloading,
                "Downloading",
            ),
        };
        let foreign = updates::UpdateProgressEvent {
            request_id: "cli-update-2".to_owned(),
            progress: matching.progress.clone(),
        };
        assert!(subscription.accepts(&matching));
        assert!(!subscription.accepts(&foreign));
    }

    #[tokio::test]
    async fn successful_update_restart_backstop_eventually_requests_shutdown() {
        let (shutdown, mut requested) = watch::channel(false);
        schedule_update_restart_backstop_after(shutdown, Duration::from_millis(10));
        timeout(Duration::from_secs(1), requested.changed())
            .await
            .expect("backstop fired")
            .expect("shutdown sender remained alive");
        assert!(*requested.borrow());
    }

    #[test]
    fn identical_status_events_are_suppressed() {
        let mut previous = None;
        assert_eq!(
            status_event_if_changed(&mut previous, json!({ "event": "process.statuses" })),
            Some("{\"event\":\"process.statuses\"}".to_owned())
        );
        assert_eq!(
            status_event_if_changed(&mut previous, json!({ "event": "process.statuses" })),
            None
        );
    }

    #[test]
    fn changed_status_events_are_emitted() {
        let mut previous = Some("{\"processes\":[]}".to_owned());
        assert_eq!(
            status_event_if_changed(&mut previous, json!({ "processes": [1] })),
            Some("{\"processes\":[1]}".to_owned())
        );
    }

    #[test]
    fn clean_status_ticks_skip_snapshot_assembly() {
        let invalidations = status_invalidation::StatusInvalidationHub::default();
        let live_stats = process_stats::LiveStatsHub::new(invalidations.clone());
        let mut subscription = ProcessStatusSubscription::new(live_stats, invalidations.clone());

        let initial = subscription.pending_version(10).unwrap();
        subscription.last_version = Some(initial);
        assert_eq!(subscription.pending_version(10), None);

        invalidations.invalidate();
        assert_eq!(subscription.pending_version(10), Some(initial + 1));
    }

    #[test]
    fn attention_deadline_marks_a_clean_subscription_dirty() {
        let invalidations = status_invalidation::StatusInvalidationHub::default();
        let live_stats = process_stats::LiveStatsHub::new(invalidations.clone());
        let mut subscription = ProcessStatusSubscription::new(live_stats, invalidations.clone());
        subscription.last_version = subscription.pending_version(10);

        invalidations.arm_deadline(20);
        assert_eq!(subscription.pending_version(19), None);
        assert_eq!(subscription.pending_version(20), Some(1));
    }

    #[tokio::test]
    async fn terminal_subscription_drains_backlog_then_parks_when_quiet() {
        let output = workman_core::pty::RawOutput::from_replay(64, b"retained output");
        let mut terminal = TerminalSubscription {
            process_id: Some(7),
            output: Some(output.clone()),
            terminal_output: None,
            offset: 0,
            replay_end_offset: output.total_bytes_seen(),
        };

        timeout(Duration::from_secs(1), terminal.output_ready())
            .await
            .expect("retained output should be immediately ready");
        terminal.offset = output.total_bytes_seen();
        assert!(
            timeout(Duration::from_millis(25), terminal.output_ready())
                .await
                .is_err(),
            "a caught-up quiet subscription must remain parked"
        );
    }

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

    #[test]
    fn terminal_input_binary_frames_decode_without_json_or_a_response() {
        let mut frame = Vec::from(*TERMINAL_INPUT_MAGIC);
        frame.extend_from_slice(&42_i64.to_be_bytes());
        frame.extend_from_slice(b"raw\x00input");

        let (process_id, data) = decode_terminal_input_frame(&frame).unwrap();
        assert_eq!(process_id, 42);
        assert_eq!(data, b"raw\x00input");
        assert!(decode_terminal_input_frame(b"not-terminal-input").is_none());
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

    #[tokio::test]
    async fn status_stream_delivers_time_driven_working_to_idle_edge_promptly() {
        let server = TestServer::start().await;
        server
            .registry
            .lock()
            .await
            .store()
            .put_project(&workman_core::Project {
                id: 1,
                path: "/tmp/workman-status-invalidation-test".into(),
                name: "status-invalidation".into(),
                display_name: None,
                icon: None,
                selected: true,
                sort_order: 0,
            })
            .unwrap();
        let (mut socket, _) = connect_async(server.request()).await.unwrap();
        rpc(
            &mut socket,
            1,
            "process.create",
            process_params(101, "agent", "idle-edge", "printf 'ready\\n$ '; sleep 30"),
        )
        .await;
        rpc(
            &mut socket,
            2,
            "process.start",
            json!({ "process_id": 101 }),
        )
        .await;
        rpc(&mut socket, 3, "process.status_subscribe", json!({})).await;

        let edge = timeout(Duration::from_secs(8), async {
            let mut saw_working = false;
            loop {
                let message = socket.next().await.unwrap().unwrap();
                let Message::Text(message) = message else {
                    continue;
                };
                let event: serde_json::Value = serde_json::from_str(&message).unwrap();
                if event["event"] != "process.statuses" {
                    continue;
                }
                let Some(process) = event["processes"]
                    .as_array()
                    .and_then(|processes| processes.iter().find(|process| process["id"] == 101))
                else {
                    continue;
                };
                match process["agent_state"]["state"].as_str() {
                    Some("working") => saw_working = true,
                    Some("idle") if saw_working => {
                        let idle_at = process["agent_state"]["last_content_change_at"]
                            .as_i64()
                            .unwrap()
                            + 5_000;
                        break timers::now_millis().saturating_sub(idle_at);
                    }
                    _ => {}
                }
            }
        })
        .await
        .expect("working-to-idle status edge was not delivered");
        eprintln!("working_to_idle_status_edge_latency_ms={edge}");
        assert!(
            (0..=750).contains(&edge),
            "working-to-idle edge arrived {edge} ms after its attention deadline"
        );

        socket.close(None).await.unwrap();
        server.stop().await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn status_stream_delivers_silent_process_exit_before_next_stats_sample() {
        let server = TestServer::start().await;
        server
            .registry
            .lock()
            .await
            .store()
            .put_project(&workman_core::Project {
                id: 1,
                path: "/tmp/workman-status-lifecycle-test".into(),
                name: "status-lifecycle".into(),
                display_name: None,
                icon: None,
                selected: true,
                sort_order: 0,
            })
            .unwrap();
        let (mut socket, _) = connect_async(server.request()).await.unwrap();
        rpc(
            &mut socket,
            1,
            "process.create",
            process_params(102, "command", "silent-exit", "sleep 1"),
        )
        .await;
        let started_at = Instant::now();
        rpc(
            &mut socket,
            2,
            "process.start",
            json!({ "process_id": 102 }),
        )
        .await;
        rpc(&mut socket, 3, "process.status_subscribe", json!({})).await;

        let delivered_after = timeout(Duration::from_secs(3), async {
            loop {
                let message = socket.next().await.unwrap().unwrap();
                let Message::Text(message) = message else {
                    continue;
                };
                let event: serde_json::Value = serde_json::from_str(&message).unwrap();
                if event["event"] != "process.statuses" {
                    continue;
                }
                let exited = event["processes"]
                    .as_array()
                    .and_then(|processes| processes.iter().find(|process| process["id"] == 102))
                    .is_some_and(|process| process["status"] == "exited");
                if exited {
                    break started_at.elapsed();
                }
            }
        })
        .await
        .expect("silent process exit was not delivered");
        eprintln!(
            "silent_process_exit_status_delivery_ms={}",
            delivered_after.as_millis()
        );
        assert!(
            delivered_after <= Duration::from_millis(1_800),
            "silent exit took {delivered_after:?} to reach the status stream"
        );

        socket.close(None).await.unwrap();
        server.stop().await;
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
                    "params": {
                        "path": first_path,
                        "display_name": "  First workspace  "
                    }
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
        assert_eq!(response["result"][0]["display_name"], "First workspace");
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
                    "id": "register-first-again",
                    "method": "projects.register",
                    "params": {
                        "path": first_path,
                        "display_name": "Folder default must not replace a rename"
                    }
                })
                .to_string()
                .into(),
            ))
            .await
            .unwrap();
        let response = receive_json(&mut socket).await;
        assert_eq!(response["ok"], true);
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
        let second = response["result"]
            .as_array()
            .unwrap()
            .iter()
            .find(|project| project["name"] == "second-project")
            .unwrap();
        assert!(second["display_name"].is_null());
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
        #[cfg(unix)]
        {
            let path = discovery_path(&server.data_dir);
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
            let endpoint_path = mcp_endpoint_path(&server.data_dir);
            let endpoint_mode = std::fs::metadata(&endpoint_path)
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(endpoint_mode, 0o600);
        }
        assert_eq!(Discovery::read(&server.data_dir).unwrap(), server.discovery);
        assert!(probe(&server.discovery).await);

        let data_dir = server.data_dir.clone();
        server.stop().await;
        assert!(!discovery_path(&data_dir).exists());
    }

    #[tokio::test]
    async fn mcp_endpoint_survives_restart_and_stays_isolated_by_data_directory() {
        let temp = tempfile::tempdir().unwrap();
        let primary_dir = temp.path().join("com.workman.todo462");
        let secondary_dir = temp.path().join("com.workman.todo462.second");

        let first = DaemonServer::bind(DaemonConfig {
            data_dir: primary_dir.clone(),
            port: 0,
        })
        .await
        .unwrap();
        let original = first.discovery().clone();
        let (shutdown, receive_shutdown) = oneshot::channel();
        let task = tokio::spawn(first.serve_until(async move {
            let _ = receive_shutdown.await;
        }));
        assert!(probe(&original).await);
        shutdown.send(()).unwrap();
        task.await.unwrap().unwrap();

        let restarted = DaemonServer::bind(DaemonConfig {
            data_dir: primary_dir.clone(),
            port: 0,
        })
        .await
        .unwrap();
        assert_eq!(restarted.discovery().port, original.port);
        assert_eq!(restarted.discovery().token, original.token);
        assert_eq!(
            mcp_connection_info(restarted.discovery()),
            mcp_connection_info(&original),
            "runtime-doctor and setup output must remain byte-stable across restart"
        );
        let restarted_discovery = restarted.discovery().clone();
        let (shutdown, receive_shutdown) = oneshot::channel();
        let task = tokio::spawn(restarted.serve_until(async move {
            let _ = receive_shutdown.await;
        }));
        assert!(
            probe(&original).await,
            "the pre-restart URL and bearer token must authenticate to the restarted daemon"
        );

        let secondary = DaemonServer::bind(DaemonConfig {
            data_dir: secondary_dir,
            port: 0,
        })
        .await
        .unwrap();
        assert_ne!(secondary.discovery().port, restarted_discovery.port);
        assert_ne!(secondary.discovery().token, restarted_discovery.token);
        drop(secondary);

        shutdown.send(()).unwrap();
        task.await.unwrap().unwrap();

        let blocker = TcpListener::bind((Ipv4Addr::LOCALHOST, original.port))
            .await
            .unwrap();
        let error = match DaemonServer::bind(DaemonConfig {
            data_dir: primary_dir.clone(),
            port: 0,
        })
        .await
        {
            Ok(_) => panic!("a persisted port conflict must not silently choose another port"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), io::ErrorKind::AddrInUse);
        assert!(error.to_string().contains("persisted Workman MCP port"));
        assert!(error.to_string().contains(&original.port.to_string()));
        let spawn_error = discover_or_spawn(
            &primary_dir,
            temp.path().join("daemon-should-not-be-spawned"),
            Duration::from_millis(100),
        )
        .await
        .unwrap_err();
        assert_eq!(spawn_error.kind(), io::ErrorKind::AddrInUse);
        assert!(
            spawn_error
                .to_string()
                .contains("no authenticated daemon discovery")
        );
        drop(blocker);

        let free = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let override_port = free.local_addr().unwrap().port();
        drop(free);
        let overridden = DaemonServer::bind(DaemonConfig {
            data_dir: primary_dir.clone(),
            port: override_port,
        })
        .await
        .unwrap();
        assert_eq!(overridden.discovery().port, override_port);
        assert_eq!(overridden.discovery().token, original.token);
        drop(overridden);

        let after_override = DaemonServer::bind(DaemonConfig {
            data_dir: primary_dir,
            port: 0,
        })
        .await
        .unwrap();
        assert_eq!(after_override.discovery().port, override_port);
        assert_eq!(after_override.discovery().token, original.token);
    }

    #[test]
    fn old_discovery_guard_cannot_remove_a_new_pid_with_the_persistent_token() {
        let temp = tempfile::tempdir().unwrap();
        let old = Discovery {
            port: 41_700,
            token: "persistent-token".to_owned(),
            pid: 100,
        };
        let old_guard = DiscoveryGuard::publish(temp.path(), &old).unwrap();
        let new = Discovery { pid: 101, ..old };
        let _new_guard = DiscoveryGuard::publish(temp.path(), &new).unwrap();

        drop(old_guard);
        assert_eq!(Discovery::read(temp.path()).unwrap(), new);
    }

    #[tokio::test]
    async fn first_persistent_boot_adopts_existing_discovery_credentials() {
        let temp = tempfile::tempdir().unwrap();
        let free = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let port = free.local_addr().unwrap().port();
        drop(free);
        let previous = Discovery {
            port,
            token: "pre-persistence-token".to_owned(),
            pid: 100,
        };
        let previous_guard = DiscoveryGuard::publish(temp.path(), &previous).unwrap();

        let upgraded = DaemonServer::bind(DaemonConfig {
            data_dir: temp.path().to_path_buf(),
            port: 0,
        })
        .await
        .unwrap();
        assert_eq!(upgraded.discovery().port, previous.port);
        assert_eq!(upgraded.discovery().token, previous.token);
        assert_eq!(
            PersistentMcpEndpoint::read(temp.path()).unwrap(),
            Some(PersistentMcpEndpoint {
                port: previous.port,
                token: previous.token.clone(),
            })
        );

        drop(previous_guard);
        assert_eq!(
            Discovery::read(temp.path()).unwrap(),
            upgraded.discovery().clone()
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn timed_out_discovery_reaps_the_spawned_daemon() {
        let temp = tempfile::tempdir().unwrap();
        let data_dir = temp.path().join("data");
        let daemon = temp.path().join("slow-workmand");
        std::fs::write(
            &daemon,
            "#!/bin/sh\nmkdir -p \"$2\"\nprintf '%s' \"$$\" > \"$2/spawn.pid\"\nexec /bin/sleep 60\n",
        )
        .unwrap();
        std::fs::set_permissions(&daemon, std::fs::Permissions::from_mode(0o700)).unwrap();

        // Give the helper shell time to publish its PID even when the workspace test runner is
        // saturating the host; the assertion is about timeout cleanup, not sub-second startup.
        let error = discover_or_spawn(&data_dir, &daemon, Duration::from_secs(1))
            .await
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);

        let pid = std::fs::read_to_string(data_dir.join("spawn.pid")).unwrap();
        let process_is_running = std::process::Command::new("kill")
            .args(["-0", pid.trim()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success();
        if process_is_running {
            let _ = std::process::Command::new("kill")
                .args(["-TERM", pid.trim()])
                .status();
        }
        assert!(!process_is_running, "timed-out daemon {pid} was leaked");
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
