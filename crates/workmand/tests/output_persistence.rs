use std::{
    collections::{BTreeMap, HashMap},
    error::Error,
    time::{Duration, Instant},
};

use axum::http::{HeaderName, HeaderValue};
use futures_util::{SinkExt, StreamExt};
use rmcp::{
    ServiceExt,
    model::{CallToolRequestParams, ClientInfo},
    transport::{
        StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use serde_json::{Map, Value, json};
use tokio::sync::oneshot;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{Message, client::IntoClientRequest},
};
use workman_core::{Process, ProcessKind, ProcessSource, ProcessStatus, Project};
use workmand::{DaemonConfig, DaemonServer, OUTPUT_DIRECTORY, WORKMAN_MCP_TOKEN_HEADER};

const PROCESS_ID: i64 = 313;
const ACTOR_PROCESS_ID: i64 = 314;
const DISTINCTIVE: &str = "PERSISTED-DAEMON-RESTART-313";
const FRAME_HEADER_LEN: usize = 21;

#[tokio::test]
async fn daemon_restart_replays_output_for_ui_control_and_mcp() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let data_dir = temp.path().join("state");
    let project_dir = temp.path().join("project");
    std::fs::create_dir(&project_dir)?;
    let project = Project {
        id: 1,
        path: project_dir.to_string_lossy().into_owned(),
        name: "output-persistence".into(),
        display_name: None,
        icon: None,
        selected: true,
        sort_order: 0,
    };

    let first = DaemonServer::bind(DaemonConfig {
        data_dir: data_dir.clone(),
        port: 0,
    })
    .await?;
    let first_registry = first.registry();
    {
        let mut registry = first_registry.lock().await;
        registry.store().put_project(&project)?;
        registry.create(process(
            PROCESS_ID,
            &project,
            ProcessKind::Terminal,
            "persistent-terminal",
            &format!("printf '\\033[36m{DISTINCTIVE}\\033[0m\\nsecond-line\\n'; sleep 30"),
        ))?;
        registry.start(PROCESS_ID)?;
    }
    wait_for_rendered(&first_registry, PROCESS_ID, DISTINCTIVE).await?;

    let (first_shutdown, first_shutdown_rx) = oneshot::channel();
    let first_task = tokio::spawn(first.serve_until(async move {
        let _ = first_shutdown_rx.await;
    }));
    drop(first_registry);
    let _ = first_shutdown.send(());
    first_task.await??;

    let spill_path = data_dir
        .join(OUTPUT_DIRECTORY)
        .join(format!("{PROCESS_ID}.raw"));
    assert!(
        spill_path.exists(),
        "graceful shutdown did not flush output"
    );
    assert!(
        String::from_utf8_lossy(&std::fs::read(&spill_path)?).contains(DISTINCTIVE),
        "spill file omitted the distinctive output"
    );

    let second = DaemonServer::bind(DaemonConfig {
        data_dir: data_dir.clone(),
        port: 0,
    })
    .await?;
    let discovery = second.discovery().clone();
    let second_registry = second.registry();
    let actor_token = {
        let mut registry = second_registry.lock().await;
        assert_eq!(registry.get(PROCESS_ID)?.status, ProcessStatus::Stopped);
        assert!(
            registry
                .rendered_output(PROCESS_ID)?
                .text
                .contains(DISTINCTIVE)
        );
        registry.create(process(
            ACTOR_PROCESS_ID,
            &project,
            ProcessKind::Agent,
            "output-reader",
            "sleep 30",
        ))?;
        registry.start(ACTOR_PROCESS_ID)?;
        registry.store().connection().query_row(
            "SELECT token FROM process_mcp_tokens WHERE process_id = ?1",
            [ACTOR_PROCESS_ID],
            |row| row.get::<_, String>(0),
        )?
    };

    let (second_shutdown, second_shutdown_rx) = oneshot::channel();
    let second_task = tokio::spawn(second.serve_until(async move {
        let _ = second_shutdown_rx.await;
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
                "id": "ui-attach",
                "method": "terminal.attach",
                "params": { "process_id": PROCESS_ID, "offset": 0 }
            })
            .to_string()
            .into(),
        ))
        .await?;
    assert_eq!(
        receive_response(&mut socket, "ui-attach").await?["ok"],
        true
    );
    let ui_bytes = receive_terminal_frame(&mut socket).await?;
    assert!(
        String::from_utf8_lossy(&ui_bytes).contains(DISTINCTIVE),
        "UI terminal stream did not receive replayed raw output"
    );

    socket
        .send(Message::Text(
            json!({
                "id": "cli-rendered",
                "method": "process.rendered_output",
                "params": { "process_id": PROCESS_ID }
            })
            .to_string()
            .into(),
        ))
        .await?;
    let control = receive_response(&mut socket, "cli-rendered").await?;
    assert!(
        control["result"]["text"]
            .as_str()
            .unwrap()
            .contains(DISTINCTIVE)
    );

    let headers = HashMap::from([(
        HeaderName::from_static(WORKMAN_MCP_TOKEN_HEADER),
        HeaderValue::from_str(&actor_token)?,
    )]);
    let transport = StreamableHttpClientTransport::from_config(
        StreamableHttpClientTransportConfig::with_uri(format!(
            "http://127.0.0.1:{}/mcp",
            discovery.port
        ))
        .custom_headers(headers),
    );
    let client = ClientInfo::default().serve(transport).await?;
    let result = client
        .call_tool(
            CallToolRequestParams::new("get_process_output")
                .with_arguments(arguments(json!({ "process_id": PROCESS_ID, "lines": 50 }))),
        )
        .await?;
    assert_ne!(result.is_error, Some(true), "MCP output failed: {result:?}");
    assert!(
        result
            .structured_content
            .as_ref()
            .and_then(|value| value["output"].as_str())
            .is_some_and(|output| output.contains(DISTINCTIVE)),
        "MCP get_process_output omitted replayed history: {result:?}"
    );

    client.cancel().await?;
    socket.close(None).await?;
    {
        let mut registry = second_registry.lock().await;
        registry.stop(ACTOR_PROCESS_ID)?;
        registry.close(ACTOR_PROCESS_ID)?;
        registry.close(PROCESS_ID)?;
    }
    assert!(
        !spill_path.exists(),
        "close_process left the raw spill file"
    );
    drop(second_registry);
    let _ = second_shutdown.send(());
    second_task.await??;
    Ok(())
}

fn process(id: i64, project: &Project, kind: ProcessKind, name: &str, command: &str) -> Process {
    Process {
        id,
        project_id: project.id,
        kind,
        name: name.into(),
        command: Some(command.into()),
        working_dir: project.path.clone(),
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

async fn wait_for_rendered(
    registry: &workmand::SharedProcessRegistry,
    process_id: i64,
    needle: &str,
) -> Result<(), Box<dyn Error>> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if registry
            .lock()
            .await
            .rendered_output(process_id)?
            .text
            .contains(needle)
        {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!("timed out waiting for {needle}").into());
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

fn arguments(value: Value) -> Map<String, Value> {
    value.as_object().unwrap().clone()
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
) -> Result<Vec<u8>, Box<dyn Error>> {
    loop {
        let message = tokio::time::timeout(Duration::from_secs(2), socket.next())
            .await?
            .ok_or("websocket closed")??;
        if let Message::Binary(bytes) = message {
            if bytes.len() >= FRAME_HEADER_LEN && &bytes[..4] == b"WRK1" {
                assert_eq!(i64::from_be_bytes(bytes[4..12].try_into()?), PROCESS_ID);
                return Ok(bytes[FRAME_HEADER_LEN..].to_vec());
            }
        }
    }
}
