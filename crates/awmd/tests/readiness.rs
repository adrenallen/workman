#![cfg(unix)]

use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    sync::Arc,
    time::{Duration, Instant},
};

use awm_core::{Process, ProcessKind, ProcessSource, ProcessStatus, Project, Store};
use awmd::{
    DaemonConfig, DaemonServer, ProcessRegistry, ReadinessService, ReadinessState,
    SharedProcessRegistry,
};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::sync::{Mutex, oneshot};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
    tungstenite::{Message, client::IntoClientRequest},
};

const HELPER_ENV: &str = "AWM_READINESS_TEST_HELPER";
const HELPER_DELAY_ENV: &str = "AWM_READINESS_TEST_DELAY_MS";

#[test]
fn listener_helper() {
    if std::env::var(HELPER_ENV).as_deref() != Ok("1") {
        return;
    }
    let delay = std::env::var(HELPER_DELAY_ENV)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    std::thread::sleep(Duration::from_millis(delay));
    let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
    println!("bound:{}", listener.local_addr().unwrap().port());
    std::io::stdout().flush().unwrap();
    std::thread::sleep(Duration::from_secs(30));
    drop(listener);
}

fn fixture() -> (TempDir, ProcessRegistry) {
    let root = tempfile::tempdir().unwrap();
    let project_path = fs::canonicalize(root.path()).unwrap();
    let store = Store::open_in_memory().unwrap();
    store
        .put_project(&Project {
            id: 1,
            path: project_path.to_string_lossy().into_owned(),
            name: "readiness-fixture".into(),
            display_name: None,
            icon: None,
            selected: true,
            sort_order: 0,
        })
        .unwrap();
    (root, ProcessRegistry::new(store).unwrap())
}

fn helper_process(root: &TempDir, id: i64, delay: Duration) -> Process {
    let executable = std::env::current_exe().unwrap();
    Process {
        id,
        project_id: 1,
        kind: ProcessKind::Command,
        name: format!("listener-{id}"),
        command: Some(format!(
            "{} --exact listener_helper --nocapture; status=$?; exit \"$status\"",
            shell_quote(&executable.to_string_lossy())
        )),
        working_dir: fs::canonicalize(root.path())
            .unwrap()
            .to_string_lossy()
            .into_owned(),
        env: BTreeMap::from([
            (HELPER_ENV.into(), "1".into()),
            (HELPER_DELAY_ENV.into(), delay.as_millis().to_string()),
        ]),
        auto_start: false,
        auto_restart: false,
        restart_when_changed: Vec::new(),
        source: ProcessSource::Local,
        trust_hash: None,
        status: ProcessStatus::Stopped,
        pid: None,
        exit_code: None,
        exit_signal: None,
        exited_at: None,
        agent_tool_id: None,
        spawned_by_process_id: None,
        sort_order: 0,
    }
}

fn sleeping_process(root: &TempDir, id: i64) -> Process {
    Process {
        command: Some("sleep 30".into()),
        env: BTreeMap::new(),
        name: format!("no-listener-{id}"),
        ..helper_process(root, id, Duration::ZERO)
    }
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

async fn start_process(registry: &SharedProcessRegistry, process: Process) {
    let process_id = process.id;
    let mut registry = registry.lock().await;
    registry.create(process).unwrap();
    assert_eq!(
        registry.start(process_id).unwrap().status,
        ProcessStatus::Running
    );
}

async fn stop_process(registry: &SharedProcessRegistry, process_id: i64) {
    let _ = registry.lock().await.stop(process_id);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_descendant_listener_is_detected_and_wait_unblocks_on_bind() {
    let (root, registry) = fixture();
    let registry = Arc::new(Mutex::new(registry));
    start_process(
        &registry,
        helper_process(&root, 1, Duration::from_millis(350)),
    )
    .await;

    let readiness = ReadinessService::default().with_poll_interval(Duration::from_millis(40));
    let started = Instant::now();
    let result = readiness
        .wait_for_bound_port(&registry, 1, Duration::from_secs(8))
        .await
        .unwrap();
    let elapsed = started.elapsed();

    assert!(result.ready);
    assert!(!result.timed_out);
    assert!(elapsed >= Duration::from_millis(200));
    assert!(elapsed < Duration::from_secs(8));
    let service = &result.services[0];
    assert_eq!(service.readiness, ReadinessState::Ready);
    assert_eq!(service.process_id, 1);
    assert!(!service.ports.is_empty());
    assert_eq!(service.ports.len(), service.urls.len());
    assert!(
        service
            .urls
            .iter()
            .all(|url| url.starts_with("http://localhost:"))
    );
    assert!(
        service
            .listeners
            .iter()
            .any(|listener| listener.listener_pid != service.root_pid.unwrap())
    );

    let listed = readiness.services_list(&registry, Some(1)).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].readiness, ReadinessState::Ready);
    stop_process(&registry, 1).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wait_for_bound_port_reports_timeout_without_holding_the_registry() {
    let (root, registry) = fixture();
    let registry = Arc::new(Mutex::new(registry));
    start_process(&registry, sleeping_process(&root, 2)).await;
    let readiness = ReadinessService::default().with_poll_interval(Duration::from_millis(40));
    let started = Instant::now();

    let result = readiness
        .wait_for_bound_port(&registry, 2, Duration::from_millis(250))
        .await
        .unwrap();
    assert!(!result.ready);
    assert!(result.timed_out);
    assert_eq!(result.process_id, 2);
    assert!(result.services.is_empty());
    assert!(started.elapsed() >= Duration::from_millis(250));
    assert!(started.elapsed() < Duration::from_secs(5));

    let service = readiness.get_process_ports(&registry, 2).await.unwrap();
    assert_eq!(service.readiness, ReadinessState::Waiting);
    stop_process(&registry, 2).await;
}

struct TestServer {
    discovery: awmd::Discovery,
    registry: SharedProcessRegistry,
    shutdown: oneshot::Sender<()>,
    task: tokio::task::JoinHandle<std::io::Result<()>>,
}

impl TestServer {
    async fn start(state_dir: &std::path::Path) -> Self {
        let server = DaemonServer::bind(DaemonConfig {
            data_dir: state_dir.to_path_buf(),
            port: 0,
        })
        .await
        .unwrap();
        let discovery = server.discovery().clone();
        let registry = server.registry();
        let (shutdown, shutdown_rx) = oneshot::channel();
        let task = tokio::spawn(server.serve_until(async move {
            let _ = shutdown_rx.await;
        }));
        Self {
            discovery,
            registry,
            shutdown,
            task,
        }
    }

    fn request(&self) -> axum::http::Request<()> {
        let mut request = format!("ws://127.0.0.1:{}/ws", self.discovery.port)
            .into_client_request()
            .unwrap();
        request.headers_mut().insert(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {}", self.discovery.token).parse().unwrap(),
        );
        request
    }

    async fn stop(self) {
        let _ = self.shutdown.send(());
        self.task.await.unwrap().unwrap();
    }
}

async fn rpc(
    socket: &mut WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
    id: u64,
    method: &str,
    params: Value,
) -> Value {
    socket
        .send(Message::Text(
            json!({ "id": id, "method": method, "params": params })
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    let message = tokio::time::timeout(Duration::from_secs(10), socket.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let Message::Text(message) = message else {
        panic!("expected JSON response, got {message:?}");
    };
    let response: Value = serde_json::from_str(&message).unwrap();
    assert_eq!(response["id"], id);
    assert_eq!(response["ok"], true, "{response}");
    response["result"].clone()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn websocket_exposes_services_ports_and_wait_contract() {
    let root = tempfile::tempdir().unwrap();
    let server = TestServer::start(&root.path().join("state")).await;
    server
        .registry
        .lock()
        .await
        .store()
        .put_project(&Project {
            id: 1,
            path: fs::canonicalize(root.path())
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            name: "ws-readiness".into(),
            display_name: None,
            icon: None,
            selected: true,
            sort_order: 0,
        })
        .unwrap();
    start_process(
        &server.registry,
        helper_process(&root, 3, Duration::from_millis(700)),
    )
    .await;
    let (mut socket, _) = connect_async(server.request()).await.unwrap();

    let timed_out = rpc(
        &mut socket,
        1,
        "process.wait_for_bound_port",
        json!({ "process_id": 3, "timeout_ms": 50 }),
    )
    .await;
    assert_eq!(timed_out["ready"], false);
    assert_eq!(timed_out["timed_out"], true);
    assert_eq!(timed_out["services"], json!([]));

    let waited = rpc(
        &mut socket,
        2,
        "process.wait_for_bound_port",
        json!({ "process_id": 3, "timeout_ms": 8_000 }),
    )
    .await;
    assert_eq!(waited["ready"], true);
    assert_eq!(waited["timed_out"], false);
    assert!(waited["services"][0]["ports"][0].as_u64().is_some());

    let ports = rpc(
        &mut socket,
        3,
        "process.get_ports",
        json!({ "process_id": 3 }),
    )
    .await;
    assert_eq!(ports["readiness"], "ready");
    assert!(
        ports["urls"][0]
            .as_str()
            .unwrap()
            .starts_with("http://localhost:")
    );

    let services = rpc(&mut socket, 4, "services.list", json!({ "project_id": 1 })).await;
    assert_eq!(services.as_array().unwrap().len(), 1);
    assert_eq!(services[0]["process_id"], 3);

    socket.close(None).await.unwrap();
    stop_process(&server.registry, 3).await;
    server.stop().await;
}
