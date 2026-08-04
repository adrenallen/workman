#![cfg(unix)]

use std::{path::PathBuf, time::Duration};

use futures_util::{SinkExt, StreamExt};
use gbuildd::{DaemonConfig, DaemonServer, Discovery};
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::{sync::oneshot, task::JoinHandle, time::timeout};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
    tungstenite::{Message, client::IntoClientRequest, http::header},
};

struct TestServer {
    discovery: Discovery,
    data_dir: PathBuf,
    shutdown: Option<oneshot::Sender<()>>,
    task: JoinHandle<std::io::Result<()>>,
    _temp: TempDir,
}

impl TestServer {
    async fn start() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let data_dir = temp.path().join("state");
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

    fn request(&self) -> tokio_tungstenite::tungstenite::http::Request<()> {
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

type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

async fn rpc(socket: &mut Socket, id: &str, method: &str, params: Value) -> Value {
    socket
        .send(Message::Text(
            json!({ "id": id, "method": method, "params": params })
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    loop {
        let message = socket.next().await.unwrap().unwrap();
        let Message::Text(message) = message else {
            continue;
        };
        let response: Value = serde_json::from_str(&message).unwrap();
        if response["id"] == id {
            return response;
        }
    }
}

#[tokio::test]
async fn websocket_syncs_reviews_and_trusts_yml_processes() {
    let server = TestServer::start().await;
    let project = server.data_dir.join("review-project");
    std::fs::create_dir(&project).unwrap();
    std::fs::write(
        project.join("gbuild.yml"),
        "processes:\n  Web:\n    command: \"trap 'exit 0' TERM; sleep 30\"\n    auto_start: true\n    env: { MODE: dev }\n",
    )
    .unwrap();
    let (mut socket, _) = connect_async(server.request()).await.unwrap();

    let registered = rpc(
        &mut socket,
        "register",
        "projects.register",
        json!({ "path": project }),
    )
    .await;
    let project_id = registered["result"][0]["id"].as_i64().unwrap();
    let synced = rpc(
        &mut socket,
        "sync",
        "config.sync",
        json!({ "project_id": project_id }),
    )
    .await;
    assert_eq!(synced["result"]["synced"], true);

    let listed = rpc(
        &mut socket,
        "list",
        "process.list",
        json!({ "project_id": project_id }),
    )
    .await;
    let process_id = listed["result"][0]["id"].as_i64().unwrap();
    assert_eq!(listed["result"][0]["source"], "yml");
    assert!(listed["result"][0]["trust_hash"].is_null());
    assert!(listed["result"][0]["agent_state"].is_object());

    let rejected = rpc(
        &mut socket,
        "start-untrusted",
        "process.start",
        json!({ "process_id": process_id }),
    )
    .await;
    assert_eq!(rejected["error"]["code"], "process_untrusted");

    let initial = rpc(
        &mut socket,
        "review-initial",
        "process.trust_review",
        json!({ "process_id": process_id }),
    )
    .await;
    assert_eq!(initial["result"]["trusted"], false);
    assert_eq!(initial["result"]["changes"].as_array().unwrap().len(), 6);
    assert!(initial["result"]["changes"][0]["previous"].is_null());
    let first_hash = initial["result"]["expected_hash"]
        .as_str()
        .unwrap()
        .to_owned();
    let approved = rpc(
        &mut socket,
        "approve-initial",
        "process.trust_yml",
        json!({ "process_id": process_id, "expected_hash": first_hash }),
    )
    .await;
    assert_eq!(approved["result"]["status"], "running");

    tokio::time::sleep(Duration::from_millis(250)).await;
    std::fs::write(
        project.join("gbuild.yml"),
        "processes:\n  Web:\n    command: \"trap 'exit 0' TERM; printf changed; sleep 30\"\n    auto_start: true\n    env: { MODE: production }\n",
    )
    .unwrap();
    let changed = timeout(Duration::from_secs(3), async {
        let mut attempt = 0;
        loop {
            attempt += 1;
            let review = rpc(
                &mut socket,
                &format!("review-changed-{attempt}"),
                "process.trust_review",
                json!({ "process_id": process_id }),
            )
            .await;
            if review["result"]["changes"]
                .as_array()
                .is_some_and(|changes| !changes.is_empty())
            {
                break review;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("gbuild.yml watcher did not resync the changed command");
    assert_eq!(changed["result"]["trusted"], false);
    let changed_fields = changed["result"]["changes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|change| change["field"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(changed_fields, ["command", "env"]);
    assert_eq!(
        changed["result"]["changes"][0]["previous"],
        "trap 'exit 0' TERM; sleep 30"
    );
    assert_eq!(
        changed["result"]["changes"][0]["current"],
        "trap 'exit 0' TERM; printf changed; sleep 30"
    );

    let stale = rpc(
        &mut socket,
        "approve-stale",
        "process.trust_yml",
        json!({ "process_id": process_id, "expected_hash": first_hash }),
    )
    .await;
    assert_eq!(stale["error"]["code"], "trust_hash_mismatch");
    let next_hash = changed["result"]["expected_hash"].as_str().unwrap();
    let approved = rpc(
        &mut socket,
        "approve-changed",
        "process.trust_yml",
        json!({ "process_id": process_id, "expected_hash": next_hash }),
    )
    .await;
    assert_eq!(approved["result"]["status"], "running");

    let terminal = rpc(
        &mut socket,
        "spawn-terminal",
        "process.spawn_terminal",
        json!({ "project_id": project_id }),
    )
    .await;
    let terminal_id = terminal["result"]["id"].as_i64().unwrap();
    assert_eq!(terminal["result"]["kind"], "terminal");
    assert_eq!(terminal["result"]["status"], "running");

    let subscribed = rpc(
        &mut socket,
        "subscribe-statuses",
        "process.status_subscribe",
        json!({}),
    )
    .await;
    assert_eq!(subscribed["result"]["subscribed"], true);

    let event = timeout(Duration::from_secs(2), async {
        loop {
            let Message::Text(message) = socket.next().await.unwrap().unwrap() else {
                continue;
            };
            let event: Value = serde_json::from_str(&message).unwrap();
            if event["event"] == "process.statuses" {
                break event;
            }
        }
    })
    .await
    .expect("process status stream did not publish");
    assert!(
        event["processes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|process| process["id"] == terminal_id && process["agent_state"].is_object())
    );

    rpc(
        &mut socket,
        "close-terminal",
        "process.close",
        json!({ "process_id": terminal_id }),
    )
    .await;
    rpc(
        &mut socket,
        "close-yml-process",
        "process.close",
        json!({ "process_id": process_id }),
    )
    .await;
    socket.close(None).await.unwrap();
    server.stop().await;
}
