#![cfg(unix)]

use std::{collections::BTreeMap, fs, path::Path, time::Duration};

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::sync::oneshot;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use workman_core::{
    Process, ProcessKind, ProcessSource, ProcessStatus, Store, attention::AttentionState,
};
use workmand::{DATABASE_FILE, DaemonConfig, DaemonServer};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn project_mark_read_rpc_returns_counts_and_preserves_other_projects() {
    let root = TempDir::new().unwrap();
    let data_dir = root.path().join("state");
    let first_path = root.path().join("first");
    let second_path = root.path().join("second");
    fs::create_dir_all(&first_path).unwrap();
    fs::create_dir_all(&second_path).unwrap();
    let server = TestServer::start(&data_dir).await;
    let (mut socket, _) = connect_async(server.request()).await.unwrap();

    let first = rpc(
        &mut socket,
        1,
        "projects.register",
        json!({ "path": fs::canonicalize(&first_path).unwrap() }),
    )
    .await;
    let first_id = project_id(&first, &first_path);
    let second = rpc(
        &mut socket,
        2,
        "projects.register",
        json!({ "path": fs::canonicalize(&second_path).unwrap() }),
    )
    .await;
    let second_id = project_id(&second, &second_path);

    let store = Store::open(data_dir.join(DATABASE_FILE)).unwrap();
    seed_unread_agent(&store, first_id, 101);
    seed_unread_agent(&store, second_id, 202);

    let result = rpc(
        &mut socket,
        3,
        "projects.mark_read",
        json!({ "project_id": first_id }),
    )
    .await;
    assert_eq!(result["notifications_updated"], 1);
    assert_eq!(result["processes_updated"], 1);

    let unread = store.list_notifications(Some(false), 10).unwrap();
    assert_eq!(unread.len(), 1);
    assert_eq!(unread[0].project_id, Some(second_id));
    server.stop().await;
}

fn seed_unread_agent(store: &Store, project_id: i64, process_id: i64) {
    store
        .put_process(&Process {
            id: process_id,
            project_id,
            kind: ProcessKind::Agent,
            name: format!("agent-{process_id}"),
            command: Some("true".into()),
            working_dir: "/tmp".into(),
            env: BTreeMap::new(),
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
        })
        .unwrap();
    store
        .observe_agent_attention(process_id, AttentionState::Working, false, true, 10)
        .unwrap();
    assert!(
        store
            .observe_agent_attention(process_id, AttentionState::Idle, false, true, 20)
            .unwrap()
            .unread
    );
}

fn project_id(projects: &Value, path: &Path) -> i64 {
    let canonical = fs::canonicalize(path)
        .unwrap()
        .to_string_lossy()
        .into_owned();
    projects
        .as_array()
        .unwrap()
        .iter()
        .find(|project| project["path"] == canonical)
        .unwrap()["id"]
        .as_i64()
        .unwrap()
}

struct TestServer {
    discovery: workmand::Discovery,
    shutdown: oneshot::Sender<()>,
    task: tokio::task::JoinHandle<std::io::Result<()>>,
}

impl TestServer {
    async fn start(data_dir: &Path) -> Self {
        let server = DaemonServer::bind(DaemonConfig {
            data_dir: data_dir.to_path_buf(),
            port: 0,
        })
        .await
        .unwrap();
        let discovery = server.discovery().clone();
        let (shutdown, shutdown_rx) = oneshot::channel();
        let task = tokio::spawn(server.serve_until(async move {
            let _ = shutdown_rx.await;
        }));
        Self {
            discovery,
            shutdown,
            task,
        }
    }

    fn request(&self) -> axum::http::Request<()> {
        use tokio_tungstenite::tungstenite::client::IntoClientRequest;

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
    socket: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
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
        panic!("expected JSON response");
    };
    let response: Value = serde_json::from_str(&message).unwrap();
    assert_eq!(response["id"], id);
    assert_eq!(response["ok"], true, "{response}");
    response["result"].clone()
}
