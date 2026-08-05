use std::{collections::BTreeMap, error::Error, path::Path};

use axum::http::{HeaderName, HeaderValue};
use futures_util::{SinkExt, StreamExt};
use gbuild_core::{Process, ProcessKind, ProcessSource, ProcessStatus, Project};
use gbuildd::{DaemonConfig, DaemonServer, Discovery};
use rmcp::{
    ServiceExt,
    model::{CallToolRequestParams, ClientInfo},
    transport::{
        StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use serde_json::{Value, json};
use tokio::{net::TcpStream, sync::oneshot, task::JoinHandle};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
    tungstenite::{Message, client::IntoClientRequest, http::header},
};

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;
type McpClient = rmcp::service::RunningService<rmcp::RoleClient, ClientInfo>;

fn project(id: i64, path: &Path, sort_order: i64) -> Project {
    Project {
        id,
        path: path.to_string_lossy().into_owned(),
        name: format!("project-{id}"),
        display_name: None,
        icon: None,
        selected: id == 1,
        sort_order,
    }
}

fn process(id: i64, project: &Project, kind: ProcessKind, name: &str) -> Process {
    Process {
        id,
        project_id: project.id,
        kind,
        name: name.into(),
        command: Some("true".into()),
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

async fn rpc(socket: &mut Socket, id: u64, method: &str, params: Value) -> Value {
    socket
        .send(Message::Text(
            json!({ "id": id, "method": method, "params": params })
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    loop {
        let Message::Text(text) = socket.next().await.unwrap().unwrap() else {
            continue;
        };
        let response: Value = serde_json::from_str(&text).unwrap();
        if response["id"] == id {
            assert_eq!(response["ok"], true, "RPC failed: {response}");
            return response["result"].clone();
        }
    }
}

async fn connect_ws(discovery: &Discovery) -> Result<Socket, Box<dyn Error>> {
    let mut request = format!("ws://127.0.0.1:{}/ws", discovery.port).into_client_request()?;
    request.headers_mut().insert(
        header::AUTHORIZATION,
        format!("Bearer {}", discovery.token).parse()?,
    );
    Ok(connect_async(request).await?.0)
}

async fn connect_mcp(discovery: &Discovery) -> Result<McpClient, Box<dyn Error>> {
    let headers = std::collections::HashMap::from([(
        HeaderName::from_static("authorization"),
        HeaderValue::from_str(&format!("Bearer {}", discovery.token))?,
    )]);
    let transport = StreamableHttpClientTransport::from_config(
        StreamableHttpClientTransportConfig::with_uri(format!(
            "http://127.0.0.1:{}/mcp",
            discovery.port
        ))
        .custom_headers(headers),
    );
    Ok(ClientInfo::default().serve(transport).await?)
}

async fn mcp_call(client: &McpClient, name: &'static str, arguments: Value) -> Value {
    let arguments = arguments
        .as_object()
        .expect("tool arguments must be an object")
        .clone();
    let result = client
        .call_tool(CallToolRequestParams::new(name).with_arguments(arguments))
        .await
        .unwrap();
    assert_ne!(result.is_error, Some(true), "{name} failed: {result:?}");
    result.structured_content.unwrap()
}

fn ids(values: &Value) -> Vec<i64> {
    values
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value["id"].as_i64().unwrap())
        .collect()
}

async fn serve(
    server: DaemonServer,
) -> (
    Discovery,
    oneshot::Sender<()>,
    JoinHandle<std::io::Result<()>>,
) {
    let discovery = server.discovery().clone();
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let task = tokio::spawn(server.serve_until(async move {
        let _ = shutdown_rx.await;
    }));
    (discovery, shutdown_tx, task)
}

#[tokio::test]
async fn websocket_reorder_is_scoped_and_mcp_order_survives_daemon_restart()
-> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let data_dir = temp.path().join("state");
    let first_dir = temp.path().join("first");
    let second_dir = temp.path().join("second");
    let third_dir = temp.path().join("third");
    std::fs::create_dir(&first_dir)?;
    std::fs::create_dir(&second_dir)?;
    std::fs::create_dir(&third_dir)?;

    let server = DaemonServer::bind(DaemonConfig {
        data_dir: data_dir.clone(),
        port: 0,
    })
    .await?;
    let registry_handle = server.registry();
    let first = project(1, &first_dir, 0);
    let second = project(2, &second_dir, 1);
    {
        let registry = server.registry();
        let mut registry = registry.lock().await;
        registry.store().put_project(&first)?;
        registry.store().put_project(&second)?;
        registry.create(process(10, &first, ProcessKind::Agent, "alpha"))?;
        registry.create(process(11, &first, ProcessKind::Agent, "beta"))?;
        registry.create(process(12, &first, ProcessKind::Terminal, "terminal"))?;
        registry.create(process(13, &first, ProcessKind::Command, "command"))?;
    }
    let (discovery, shutdown, task) = serve(server).await;
    let mut socket = connect_ws(&discovery).await?;

    let projects = rpc(
        &mut socket,
        1,
        "project.reorder",
        json!({ "ordered_ids": [2, 1] }),
    )
    .await;
    assert_eq!(ids(&projects), [2, 1]);
    assert_eq!(projects[0]["sort_order"], 0);
    assert_eq!(projects[1]["sort_order"], 1);
    let projects = rpc(
        &mut socket,
        2,
        "projects.register",
        json!({ "path": third_dir }),
    )
    .await;
    assert_eq!(
        ids(&projects),
        [2, 1, 3],
        "new projects append after a reorder"
    );
    assert_eq!(projects[2]["sort_order"], 2);

    let processes = rpc(
        &mut socket,
        3,
        "process.reorder",
        json!({ "project_id": 1, "kind": "agent", "ordered_ids": [11, 10] }),
    )
    .await;
    let agents = processes
        .as_array()
        .unwrap()
        .iter()
        .filter(|process| process["kind"] == "agent")
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(ids(&Value::Array(agents.clone())), [11, 10]);
    assert_eq!(agents[0]["sort_order"], 0);
    assert_eq!(agents[1]["sort_order"], 1);
    assert_eq!(
        processes
            .as_array()
            .unwrap()
            .iter()
            .find(|process| process["id"] == 12)
            .unwrap()["sort_order"],
        0,
        "terminal order is scoped independently from agents"
    );
    registry_handle
        .lock()
        .await
        .create(process(14, &first, ProcessKind::Agent, "gamma"))?;
    let processes = rpc(&mut socket, 4, "process.list", json!({ "project_id": 1 })).await;
    let agents = processes
        .as_array()
        .unwrap()
        .iter()
        .filter(|process| process["kind"] == "agent")
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(
        ids(&Value::Array(agents.clone())),
        [11, 10, 14],
        "new processes append within their project/kind group"
    );
    assert_eq!(agents[2]["sort_order"], 2);

    drop(socket);
    let _ = shutdown.send(());
    task.await??;

    let restarted = DaemonServer::bind(DaemonConfig { data_dir, port: 0 }).await?;
    let (discovery, shutdown, task) = serve(restarted).await;
    let client = connect_mcp(&discovery).await?;
    let projects = mcp_call(&client, "list_projects", json!({})).await;
    assert_eq!(ids(&projects["projects"]), [2, 1, 3]);
    assert_eq!(projects["projects"][0]["sort_order"], 0);

    let processes = mcp_call(&client, "list_processes", json!({ "project_id": 1 })).await;
    let agents = processes["processes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|process| process["kind"] == "agent")
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(ids(&Value::Array(agents.clone())), [11, 10, 14]);
    assert_eq!(agents[0]["sort_order"], 0);
    assert_eq!(agents[1]["sort_order"], 1);
    assert_eq!(agents[2]["sort_order"], 2);

    let _ = client.cancel().await;
    let _ = shutdown.send(());
    task.await??;
    Ok(())
}
