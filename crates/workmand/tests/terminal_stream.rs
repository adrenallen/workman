use std::{collections::BTreeMap, error::Error, time::Duration};

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{Message, client::IntoClientRequest},
};
use workman_core::{Process, ProcessKind, ProcessSource, ProcessStatus, Project};
use workmand::{DaemonConfig, DaemonServer};

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
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let ready = {
                let mut registry = registry.lock().await;
                let focus_reporting = registry.terminal_focus_reporting(101).unwrap_or(false);
                let keyboard = registry.terminal_keyboard_protocol(101).unwrap_or_default();
                focus_reporting && keyboard.kitty_flags == 1 && keyboard.modify_other_keys == 2
            };
            if ready {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await?;

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
    assert!(attached["result"]["replay_start_offset"].is_u64());
    assert!(attached["result"]["replay_end_offset"].is_u64());
    assert_eq!(attached["result"]["focus_reporting"], true);
    assert_eq!(attached["result"]["keyboard_protocol"]["kitty_flags"], 1);
    assert_eq!(
        attached["result"]["keyboard_protocol"]["modify_other_keys"],
        2
    );
    assert!(
        attached["result"]["replay_start_offset"].as_u64().unwrap()
            <= attached["result"]["replay_end_offset"].as_u64().unwrap()
    );
    let replay_end_offset = attached["result"]["replay_end_offset"].as_u64().unwrap();
    let first = receive_terminal_frame(&mut socket).await?;
    assert_eq!(first.process_id, 101);
    assert_eq!(first.kitty_keyboard_flags, 1);
    assert_eq!(first.modify_other_keys, 2);
    assert_frame_does_not_cross_replay_boundary(&first, replay_end_offset);
    let mut replayed_data = first.data.clone();
    let mut replayed_through = first.end_offset();
    while replayed_through < replay_end_offset {
        let frame = receive_terminal_frame(&mut socket).await?;
        assert_eq!(frame.process_id, 101);
        assert_frame_does_not_cross_replay_boundary(&frame, replay_end_offset);
        replayed_data.extend_from_slice(&frame.data);
        replayed_through = frame.end_offset();
    }
    assert_eq!(replayed_through, replay_end_offset);
    assert!(String::from_utf8_lossy(&replayed_data).contains("one:"));

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
            frame.process_id, 102,
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
        "printf '\\033[?1004h\\033[>1u\\033[>4;2m'; i=0; while [ \"$i\" -lt 30000 ]; do printf '{label}:%05d\\n' \"$i\"; i=$((i+1)); done; sleep 5"
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
) -> Result<DecodedTerminalFrame, Box<dyn Error>> {
    loop {
        let message = tokio::time::timeout(std::time::Duration::from_secs(2), socket.next())
            .await?
            .ok_or("websocket closed")??;
        if let Message::Binary(bytes) = message {
            if bytes.len() < FRAME_HEADER_LEN || &bytes[..4] != b"WRK1" {
                continue;
            }
            let process_id = i64::from_be_bytes(bytes[4..12].try_into()?);
            let start_offset = u64::from_be_bytes(bytes[12..20].try_into()?);
            let flags = bytes[20];
            return Ok(DecodedTerminalFrame {
                process_id,
                start_offset,
                kitty_keyboard_flags: (flags >> 1) & 1,
                modify_other_keys: (flags >> 2) & 3,
                data: bytes[FRAME_HEADER_LEN..].to_vec(),
            });
        }
    }
}

struct DecodedTerminalFrame {
    process_id: i64,
    start_offset: u64,
    kitty_keyboard_flags: u8,
    modify_other_keys: u8,
    data: Vec<u8>,
}

impl DecodedTerminalFrame {
    fn end_offset(&self) -> u64 {
        self.start_offset + self.data.len() as u64
    }
}

fn assert_frame_does_not_cross_replay_boundary(frame: &DecodedTerminalFrame, boundary: u64) {
    assert!(
        frame.end_offset() <= boundary || frame.start_offset >= boundary,
        "terminal frame {}..{} crossed replay boundary {boundary}",
        frame.start_offset,
        frame.end_offset(),
    );
}
