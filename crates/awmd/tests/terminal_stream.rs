use std::{collections::BTreeMap, error::Error};

use awm_core::{Process, ProcessKind, ProcessSource, ProcessStatus, Project};
use awmd::{DaemonConfig, DaemonServer};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{Message, client::IntoClientRequest},
};

const FRAME_HEADER_LEN: usize = 21;

#[tokio::test]
async fn websocket_streams_raw_bytes_for_only_the_attached_process() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let project_dir = temp.path().join("project");
    std::fs::create_dir(&project_dir)?;
    let server = DaemonServer::bind(DaemonConfig {
        data_dir: temp.path().join("state"),
        port: 0,
    })
    .await?;
    let discovery = server.discovery().clone();
    let registry = server.registry();
    {
        let mut registry = registry.lock().await;
        registry.store().put_project(&Project {
            id: 1,
            path: project_dir.to_string_lossy().into_owned(),
            name: "stream-test".into(),
            display_name: None,
            icon: None,
            selected: true,
            sort_order: 0,
        })?;
        registry.create(test_process(101, "one", &project_dir.to_string_lossy()))?;
        registry.create(test_process(102, "two", &project_dir.to_string_lossy()))?;
        registry.start(101)?;
        registry.start(102)?;
    }

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let server_task = tokio::spawn(server.serve_until(async move {
        let _ = shutdown_rx.await;
    }));
    let mut request = format!("ws://127.0.0.1:{}/ws", discovery.port).into_client_request()?;
    request.headers_mut().insert(
        "Authorization",
        format!("Bearer {}", discovery.token).parse()?,
    );
    let (mut socket, _) = connect_async(request).await?;

    socket
        .send(Message::Text(
            json!({
                "id": "attach-one",
                "method": "terminal.attach",
                "params": { "process_id": 101 }
            })
            .to_string()
            .into(),
        ))
        .await?;
    let attached = receive_response(&mut socket, "attach-one").await?;
    assert_eq!(attached["ok"], true);
    let first = receive_terminal_frame(&mut socket).await?;
    assert_eq!(first.0, 101);
    assert!(String::from_utf8_lossy(&first.1).contains("one:"));

    socket
        .send(Message::Text(
            json!({
                "id": "attach-two",
                "method": "terminal.attach",
                "params": { "process_id": 102 }
            })
            .to_string()
            .into(),
        ))
        .await?;
    let attached = receive_response(&mut socket, "attach-two").await?;
    assert_eq!(attached["result"]["process_id"], 102);
    for _ in 0..3 {
        let frame = receive_terminal_frame(&mut socket).await?;
        assert_eq!(
            frame.0, 102,
            "a background process leaked into the UI stream"
        );
    }

    socket.close(None).await?;
    {
        let mut registry = registry.lock().await;
        let _ = registry.stop(101);
        let _ = registry.stop(102);
    }
    let _ = shutdown_tx.send(());
    server_task.await??;
    Ok(())
}

fn test_process(id: i64, label: &str, working_dir: &str) -> Process {
    let command = format!(
        "i=0; while [ \"$i\" -lt 30000 ]; do printf '{label}:%05d\\n' \"$i\"; i=$((i+1)); done; sleep 5"
    );
    Process {
        id,
        project_id: 1,
        kind: ProcessKind::Terminal,
        name: label.into(),
        command: Some(command),
        working_dir: working_dir.into(),
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
    }
}

async fn receive_response(
    socket: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    id: &str,
) -> Result<Value, Box<dyn Error>> {
    loop {
        let message = socket.next().await.ok_or("websocket closed")??;
        if let Message::Text(text) = message {
            let response: Value = serde_json::from_str(&text)?;
            if response["id"] == id {
                return Ok(response);
            }
        }
    }
}

async fn receive_terminal_frame(
    socket: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
) -> Result<(i64, Vec<u8>), Box<dyn Error>> {
    loop {
        let message = tokio::time::timeout(std::time::Duration::from_secs(2), socket.next())
            .await?
            .ok_or("websocket closed")??;
        if let Message::Binary(bytes) = message {
            if bytes.len() < FRAME_HEADER_LEN || &bytes[..4] != b"AWM1" {
                continue;
            }
            let process_id = i64::from_be_bytes(bytes[4..12].try_into()?);
            return Ok((process_id, bytes[FRAME_HEADER_LEN..].to_vec()));
        }
    }
}
