#![cfg(unix)]

use std::{fs, path::Path, time::Duration};

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::sync::oneshot;
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
    tungstenite::{Message, client::IntoClientRequest},
};
use workmand::{DaemonConfig, DaemonServer};

type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

struct TestServer {
    discovery: workmand::Discovery,
    shutdown: oneshot::Sender<()>,
    task: tokio::task::JoinHandle<std::io::Result<()>>,
}

impl TestServer {
    async fn start(state_dir: &Path) -> Self {
        let server = DaemonServer::bind(DaemonConfig {
            data_dir: state_dir.to_path_buf(),
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

async fn rpc(socket: &mut Socket, id: u64, method: &str, params: Value) -> Value {
    let response = rpc_response(socket, id, method, params).await;
    assert_eq!(response["ok"], true, "{response}");
    response["result"].clone()
}

async fn rpc_response(socket: &mut Socket, id: u64, method: &str, params: Value) -> Value {
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
    response
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn context_action_rpcs_confirm_kill_remove_and_manage_scratchpads() {
    let root = TempDir::new().unwrap();
    let project_path = root.path().join("project");
    fs::create_dir(&project_path).unwrap();
    let canonical_project = fs::canonicalize(&project_path).unwrap();
    let server = TestServer::start(&root.path().join("state")).await;
    let (mut socket, _) = connect_async(server.request()).await.unwrap();

    let projects = rpc(
        &mut socket,
        1,
        "projects.register",
        json!({ "path": canonical_project }),
    )
    .await;
    let project_id = projects
        .as_array()
        .unwrap()
        .iter()
        .find(|project| project["selected"] == true)
        .unwrap()["id"]
        .as_i64()
        .unwrap();

    let scratchpad = rpc(
        &mut socket,
        2,
        "coordination.scratchpad_create",
        json!({
            "project_id": project_id,
            "name": "Runbook",
            "content": "context action fixture"
        }),
    )
    .await;
    let scratchpad_id = scratchpad["id"].as_i64().unwrap();
    let renamed = rpc(
        &mut socket,
        3,
        "coordination.scratchpad_rename",
        json!({
            "project_id": project_id,
            "scratchpad_id": scratchpad_id,
            "name": "Deploy runbook",
            "expected_revision": scratchpad["revision"]
        }),
    )
    .await;
    assert_eq!(renamed["name"], "Deploy runbook");
    let archived = rpc(
        &mut socket,
        4,
        "coordination.scratchpad_archive",
        json!({
            "project_id": project_id,
            "scratchpad_id": scratchpad_id,
            "expected_revision": renamed["revision"]
        }),
    )
    .await;
    assert_eq!(archived["archived"], true);
    let deleted = rpc(
        &mut socket,
        5,
        "coordination.scratchpad_delete",
        json!({
            "project_id": project_id,
            "scratchpad_id": scratchpad_id,
            "expected_revision": archived["revision"]
        }),
    )
    .await;
    assert_eq!(
        deleted,
        json!({ "scratchpad_id": scratchpad_id, "deleted": true })
    );

    let terminal = rpc(
        &mut socket,
        6,
        "process.spawn_terminal",
        json!({ "project_id": project_id, "name": "hard-kill fixture" }),
    )
    .await;
    let process_id = terminal["id"].as_i64().unwrap();
    let rejected = rpc_response(
        &mut socket,
        7,
        "process.kill",
        json!({ "process_id": process_id }),
    )
    .await;
    assert_eq!(rejected["ok"], false);
    assert_eq!(rejected["error"]["code"], "confirmation_required");
    let killed = rpc(
        &mut socket,
        8,
        "process.kill",
        json!({ "process_id": process_id, "confirm_kill": true }),
    )
    .await;
    assert_eq!(killed["status"], "stopped");
    assert_eq!(killed["pid"], Value::Null);
    assert_eq!(killed["exit_signal"], 9);

    let rejected = rpc_response(
        &mut socket,
        9,
        "projects.remove",
        json!({ "project_id": project_id }),
    )
    .await;
    assert_eq!(rejected["ok"], false);
    assert_eq!(rejected["error"]["code"], "confirmation_required");
    let removed = rpc(
        &mut socket,
        10,
        "projects.remove",
        json!({ "project_id": project_id, "confirm_remove": true }),
    )
    .await;
    assert_eq!(removed["removed"], true);
    assert_eq!(removed["files_removed"], false);
    assert!(
        canonical_project.is_dir(),
        "project files were unexpectedly removed"
    );
    assert_eq!(
        rpc(&mut socket, 11, "projects.list", json!({})).await,
        json!([])
    );

    socket.close(None).await.unwrap();
    server.stop().await;
}
