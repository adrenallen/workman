use std::{future::pending, path::PathBuf, time::Duration};

use awmd::{DaemonConfig, DaemonServer, Discovery, discovery_path};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::{task::JoinHandle, time::timeout};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
    tungstenite::{Message, client::IntoClientRequest, http::header},
};

struct TestServer {
    discovery: Discovery,
    data_dir: PathBuf,
    task: JoinHandle<std::io::Result<()>>,
    _temp: TempDir,
}

impl TestServer {
    async fn start() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let data_dir = temp.path().join("isolated-settings-state");
        let server = DaemonServer::bind(DaemonConfig {
            data_dir: data_dir.clone(),
            port: 0,
        })
        .await
        .unwrap();
        let discovery = server.discovery().clone();
        let task = tokio::spawn(server.serve_until(pending()));
        Self {
            discovery,
            data_dir,
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
}

type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

async fn rpc(socket: &mut Socket, id: &str, method: &str) -> Value {
    socket
        .send(Message::Text(
            json!({ "id": id, "method": method, "params": {} })
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
            assert_eq!(response["ok"], true, "RPC failed: {response}");
            return response["result"].clone();
        }
    }
}

#[tokio::test]
async fn settings_report_mcp_setup_data_and_restart_the_isolated_daemon() {
    let server = TestServer::start().await;
    let (mut socket, _) = connect_async(server.request()).await.unwrap();

    let info = rpc(&mut socket, "info", "daemon.info").await;
    assert_eq!(info["data_dir"], server.data_dir.to_string_lossy().as_ref());
    assert_eq!(info["port"], server.discovery.port);
    assert_eq!(info["pid"], std::process::id());
    assert_eq!(info["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(info["build_id"], awmd::BUILD_ID);
    assert_eq!(
        info["control_protocol_version"],
        awmd::CONTROL_PROTOCOL_VERSION
    );
    assert!(info["uptime_ms"].as_u64().is_some());
    assert_eq!(info["update"]["automatic_checks"], true);
    assert_eq!(info["update"]["last_checked_at"], Value::Null);
    assert_eq!(
        info["update"]["check"]["current"],
        env!("CARGO_PKG_VERSION")
    );
    assert_eq!(
        info["mcp"]["endpoint"],
        format!("http://127.0.0.1:{}/mcp", server.discovery.port)
    );
    assert_eq!(info["mcp"]["token"], server.discovery.token);
    let setups = info["mcp"]["setups"].as_array().unwrap();
    assert_eq!(setups.len(), 5);
    let claude = setups
        .iter()
        .find(|setup| setup["client"] == "claude")
        .unwrap();
    assert!(
        claude["fields"][0]["value"]
            .as_str()
            .unwrap()
            .contains("claude mcp add --transport http awm")
    );
    let codex = setups
        .iter()
        .find(|setup| setup["client"] == "codex")
        .unwrap();
    assert!(
        codex["fields"][1]["value"]
            .as_str()
            .unwrap()
            .contains("env_http_headers")
    );
    assert!(setups.iter().all(|setup| {
        setup["fields"].as_array().unwrap().iter().any(|field| {
            field["value"]
                .as_str()
                .unwrap()
                .contains(&server.discovery.token)
        })
    }));

    let hello = rpc(&mut socket, "hello", "daemon.hello").await;
    assert_eq!(hello["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(hello["build_id"], awmd::BUILD_ID);
    assert_eq!(
        hello["control_protocol_version"],
        awmd::CONTROL_PROTOCOL_VERSION
    );

    let restarted = rpc(&mut socket, "restart", "daemon.restart").await;
    assert_eq!(restarted["restarting"], true);
    timeout(Duration::from_secs(3), server.task)
        .await
        .expect("daemon did not stop after restart request")
        .unwrap()
        .unwrap();
    assert!(!discovery_path(server.data_dir).exists());
}
