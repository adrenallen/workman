#![cfg(unix)]

use std::{fs, path::Path, time::Duration};

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use sysinfo::{Pid, System};
use tempfile::TempDir;
use tokio::sync::oneshot;
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
    tungstenite::{Message, client::IntoClientRequest},
};
use workman_core::{Project, Store};
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

async fn wait_for_python_child(socket: &mut Socket, next_id: &mut u64, process_id: i64) -> Value {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let result = rpc(
                socket,
                *next_id,
                "process.subprocesses",
                json!({ "process_id": process_id }),
            )
            .await;
            *next_id += 1;
            if let Some(child) = result["subprocesses"].as_array().and_then(|children| {
                children.iter().find(|child| {
                    child["name"]
                        .as_str()
                        .is_some_and(|name| name.to_ascii_lowercase().contains("python"))
                        || child["command"]
                            .as_str()
                            .is_some_and(|command| command.to_ascii_lowercase().contains("python"))
                })
            }) {
                return child.clone();
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("python listener did not appear in the descendant tree")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn subprocess_rpc_lists_and_only_kills_live_descendants() {
    let root = TempDir::new().unwrap();
    let project_path = workman_core::canonical_path(root.path()).unwrap();
    let server = TestServer::start(&root.path().join("state")).await;
    let store = Store::open(root.path().join("state").join(workmand::DATABASE_FILE)).unwrap();
    store
        .put_project(&Project {
            id: 1,
            path: project_path.to_string_lossy().into_owned(),
            name: "subprocess-rpc".into(),
            display_name: None,
            icon: None,
            selected: true,
            sort_order: 0,
        })
        .unwrap();

    let (mut socket, _) = connect_async(server.request()).await.unwrap();
    let terminal = rpc(
        &mut socket,
        1,
        "process.spawn_terminal",
        json!({ "project_id": 1, "name": "listener shell" }),
    )
    .await;
    let process_id = terminal["id"].as_i64().unwrap();
    let root_pid = terminal["pid"].as_u64().unwrap();
    rpc(
        &mut socket,
        2,
        "process.send_input",
        json!({
            "process_id": process_id,
            "data": "cHl0aG9uMyAtbSBodHRwLnNlcnZlciAw",
            "submit": true
        }),
    )
    .await;

    let mut next_id = 3;
    let child = wait_for_python_child(&mut socket, &mut next_id, process_id).await;
    let child_pid = child["pid"].as_u64().unwrap();
    assert_ne!(child_pid, root_pid);
    assert!(child["parent_pid"].as_u64().is_some());
    assert!(child["cpu_percent"].as_f64().is_some());
    assert!(child["memory_bytes"].as_u64().is_some());

    let rejected = rpc_response(
        &mut socket,
        next_id,
        "process.kill_subprocess",
        json!({ "process_id": process_id, "pid": root_pid }),
    )
    .await;
    next_id += 1;
    assert_eq!(rejected["ok"], false);
    assert_eq!(rejected["error"]["code"], "subprocess_not_found");

    let unrelated_pid = std::process::id();
    let rejected = rpc_response(
        &mut socket,
        next_id,
        "process.kill_subprocess",
        json!({ "process_id": process_id, "pid": unrelated_pid }),
    )
    .await;
    next_id += 1;
    assert_eq!(rejected["ok"], false);
    assert_eq!(rejected["error"]["code"], "subprocess_not_found");

    let killed = rpc(
        &mut socket,
        next_id,
        "process.kill_subprocess",
        json!({ "process_id": process_id, "pid": child_pid }),
    )
    .await;
    next_id += 1;
    assert_eq!(killed["pid"], child_pid);
    assert_eq!(killed["signal"], "term");
    assert_eq!(killed["delivered"], true);

    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let listed = rpc(
                &mut socket,
                next_id,
                "process.subprocesses",
                json!({ "process_id": process_id }),
            )
            .await;
            next_id += 1;
            if listed["subprocesses"]
                .as_array()
                .is_some_and(|children| children.iter().all(|child| child["pid"] != child_pid))
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("signaled child remained in the descendant tree");

    let root_status = rpc(
        &mut socket,
        next_id,
        "process.get",
        json!({ "process_id": process_id }),
    )
    .await;
    next_id += 1;
    assert_eq!(root_status["status"], "running");
    tokio::time::sleep(Duration::from_millis(250)).await;

    // A nested non-interactive shell lets Python create a new session, while ignored HUP/TERM
    // ensures that PTY closure and the root process-group signal cannot clean it up by accident.
    rpc(
        &mut socket,
        next_id,
        "process.send_input",
        json!({
            "process_id": process_id,
            "data": "c2ggLWMgJ3B5dGhvbjMgLWMgImltcG9ydCBvcyxzaWduYWwsdGltZTsgb3Muc2V0c2lkKCk7IHNpZ25hbC5zaWduYWwoc2lnbmFsLlNJR0hVUCxzaWduYWwuU0lHX0lHTik7IHNpZ25hbC5zaWduYWwoc2lnbmFsLlNJR1RFUk0sc2lnbmFsLlNJR19JR04pOyBwcmludChvcy5nZXRwaWQoKSxmbHVzaD1UcnVlKTsgdGltZS5zbGVlcCgzMCkiICYgd2FpdCc=",
            "submit": true
        }),
    )
    .await;
    next_id += 1;
    let detached = wait_for_python_child(&mut socket, &mut next_id, process_id).await;
    let detached_pid = detached["pid"].as_u64().unwrap() as u32;

    let stopped = rpc(
        &mut socket,
        next_id,
        "process.stop",
        json!({ "process_id": process_id }),
    )
    .await;
    next_id += 1;
    assert_eq!(stopped["status"], "stopped");
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if System::new_all()
                .process(Pid::from_u32(detached_pid))
                .is_none()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("detached process-group descendant survived parent stop");

    rpc(
        &mut socket,
        next_id,
        "process.close",
        json!({ "process_id": process_id }),
    )
    .await;
    socket.close(None).await.unwrap();
    server.stop().await;
}
