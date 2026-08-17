use std::{collections::BTreeMap, error::Error};

use axum::http::{HeaderName, HeaderValue};
use futures_util::{SinkExt, StreamExt};
use rmcp::{
    ServiceExt,
    model::{CallToolRequestParams, CallToolResult, ClientInfo},
    transport::{
        StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use serde_json::{Map, Value, json};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{Message, client::IntoClientRequest, http::header},
};
use workman_core::{Process, ProcessKind, ProcessSource, ProcessStatus, Project};
use workmand::{DaemonConfig, DaemonServer, WORKMAN_MCP_TOKEN_HEADER};

type McpClient = rmcp::service::RunningService<rmcp::RoleClient, ClientInfo>;

fn arguments(value: Value) -> Map<String, Value> {
    value
        .as_object()
        .expect("tool arguments must be an object")
        .clone()
}

async fn invoke(client: &McpClient, name: &'static str, args: Value) -> CallToolResult {
    client
        .call_tool(CallToolRequestParams::new(name).with_arguments(arguments(args)))
        .await
        .unwrap_or_else(|error| panic!("{name} transport failed: {error}"))
}

async fn call(client: &McpClient, name: &'static str, args: Value) -> Value {
    let result = invoke(client, name, args).await;
    assert_ne!(result.is_error, Some(true), "{name} returned {result:?}");
    result.structured_content.expect("structured MCP result")
}

async fn rejected(client: &McpClient, name: &'static str, args: Value) -> Value {
    let result = invoke(client, name, args).await;
    assert_eq!(result.is_error, Some(true), "{name} unexpectedly succeeded");
    result.structured_content.expect("structured MCP error")
}

fn assert_jailed(error: &Value, owning_project_id: i64) {
    assert!(
        error["message"]
            .as_str()
            .is_some_and(|message| message.contains(&format!(
                "agent identities are scoped to project {owning_project_id}"
            ))),
        "scope error was not self-explanatory: {error}"
    );
}

fn project(id: i64, path: &std::path::Path) -> Project {
    Project {
        id,
        path: path.to_string_lossy().into_owned(),
        name: format!("project-{id}"),
        display_name: None,
        icon: None,
        selected: id == 1,
        sort_order: id,
    }
}

fn process(id: i64, project: &Project, name: &str) -> Process {
    Process {
        id,
        project_id: project.id,
        kind: ProcessKind::Agent,
        name: name.into(),
        command: Some("sleep 30".into()),
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

async fn control_call(
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
    loop {
        let Message::Text(text) = socket.next().await.unwrap().unwrap() else {
            continue;
        };
        let response: Value = serde_json::from_str(&text).unwrap();
        if response["id"] == id {
            assert_eq!(response["ok"], true, "control request failed: {response}");
            return response["result"].clone();
        }
    }
}

#[tokio::test]
async fn agent_identity_is_jailed_to_its_own_project_while_user_control_stays_global()
-> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let one_path = temp.path().join("one");
    let two_path = temp.path().join("two");
    std::fs::create_dir_all(&one_path)?;
    std::fs::create_dir_all(&two_path)?;
    let one = project(1, &one_path);
    let two = project(2, &two_path);
    let server = DaemonServer::bind(DaemonConfig {
        data_dir: temp.path().join("state"),
        port: 0,
    })
    .await?;
    let discovery = server.discovery().clone();
    let registry = server.registry();
    let process_token = "todo455-project-one-token";
    {
        let registry = registry.lock().await;
        registry.store().put_project(&one)?;
        registry.store().put_project(&two)?;
        registry
            .store()
            .put_process(&process(10, &one, "jailed-agent"))?;
        registry
            .store()
            .put_process(&process(20, &two, "foreign-agent"))?;
        registry
            .store()
            .set_process_mcp_token(10, process_token, 1_700_000_000_000)?;
    }

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let server_task = tokio::spawn(server.serve_until(async move {
        let _ = shutdown_rx.await;
    }));
    let endpoint = format!("http://127.0.0.1:{}/mcp", discovery.port);
    let headers = std::collections::HashMap::from([(
        HeaderName::from_static(WORKMAN_MCP_TOKEN_HEADER),
        HeaderValue::from_str(process_token)?,
    )]);
    let client = ClientInfo::default()
        .serve(StreamableHttpClientTransport::from_config(
            StreamableHttpClientTransportConfig::with_uri(endpoint.clone()).custom_headers(headers),
        ))
        .await?;

    let identity = call(&client, "whoami", json!({})).await;
    assert_eq!(identity["process_id"], 10);
    assert_eq!(identity["effective_project_id"], 1);
    let visible = call(&client, "list_projects", json!({})).await;
    assert_eq!(visible["projects"].as_array().unwrap().len(), 1);
    assert_eq!(visible["projects"][0]["id"], 1);

    let spawned = call(
        &client,
        "spawn_process",
        json!({ "project_id": 1, "kind": "terminal", "name": "allowed-terminal" }),
    )
    .await;
    let spawned_id = spawned["process_id"].as_i64().unwrap();
    call(
        &client,
        "close_process",
        json!({ "process_id": spawned_id }),
    )
    .await;

    for (name, args) in [
        (
            "spawn_process",
            json!({ "project_id": 2, "kind": "terminal", "name": "escape" }),
        ),
        (
            "spawn_agent",
            json!({ "project_id": 2, "agent_tool_id": 999 }),
        ),
        ("identify_session", json!({ "process_id": 20 })),
        ("select_project", json!({ "project_id": 2 })),
        ("get_project", json!({ "project_id": 2 })),
        (
            "create_project",
            json!({ "path": two_path, "name": "duplicate-foreign" }),
        ),
        (
            "todo_create",
            json!({ "project_id": 2, "title": "foreign todo" }),
        ),
        (
            "scratchpad_write",
            json!({ "project_id": 2, "name": "foreign", "content": "no" }),
        ),
        (
            "timer_set",
            json!({ "project_id": 2, "delay_ms": 1000, "body": "no" }),
        ),
        ("timer_list", json!({ "project_id": 2 })),
        (
            "lock_acquire",
            json!({ "project_id": 2, "lock_key": "foreign", "lease_ttl_seconds": 60 }),
        ),
        (
            "worktree_create",
            json!({ "project_id": 1, "branch": "would-create-another-project" }),
        ),
        (
            "agent_tool_configure_preview",
            json!({ "agent_tool_id": 999 }),
        ),
        ("send_input", json!({ "process_id": 20, "input": "no" })),
        ("stop_process", json!({ "process_id": 20 })),
    ] {
        let error = rejected(&client, name, args).await;
        assert_jailed(&error, 1);
    }

    let delivery_error = rejected(
        &client,
        "timer_set",
        json!({ "delay_ms": 1000, "body": "no", "delivery_process_id": 20 }),
    )
    .await;
    assert_jailed(&delivery_error, 1);
    let watch_error = rejected(
        &client,
        "timer_fire_when_idle_any",
        json!({ "processes": [20], "max_wait_ms": 1000, "body": "no" }),
    )
    .await;
    assert_jailed(&watch_error, 1);

    let todo = call(
        &client,
        "todo_create",
        json!({ "title": "own todo", "response_mode": "rich" }),
    )
    .await;
    let todo_transfer = rejected(
        &client,
        "todo_transfer",
        json!({ "todo_id": todo["id"], "target_project_id": 2 }),
    )
    .await;
    assert_jailed(&todo_transfer, 1);
    let scratchpad = call(
        &client,
        "scratchpad_write",
        json!({ "name": "own", "content": "safe" }),
    )
    .await;
    let scratchpad_transfer = rejected(
        &client,
        "scratchpad_transfer",
        json!({
            "scratchpad_id": scratchpad["scratchpad_id"],
            "target_project_id": 2,
            "expected_revision": scratchpad["revision"]
        }),
    )
    .await;
    assert_jailed(&scratchpad_transfer, 1);

    let bearer = ClientInfo::default()
        .serve(StreamableHttpClientTransport::from_config(
            StreamableHttpClientTransportConfig::with_uri(endpoint)
                .auth_header(discovery.token.clone()),
        ))
        .await?;
    assert_eq!(
        call(&bearer, "list_projects", json!({})).await["projects"]
            .as_array()
            .unwrap()
            .len(),
        2,
        "list_projects remains discovery for a bearer-authenticated unidentified session"
    );
    let unidentified = rejected(&bearer, "get_project", json!({ "project_id": 2 })).await;
    assert_eq!(unidentified["code"], "project_scope_error");
    assert!(
        unidentified["message"]
            .as_str()
            .unwrap()
            .contains("authenticated process identity")
    );

    let mut request = format!("ws://127.0.0.1:{}/ws", discovery.port).into_client_request()?;
    request.headers_mut().insert(
        header::AUTHORIZATION,
        format!("Bearer {}", discovery.token).parse()?,
    );
    let (mut user_control, _) = connect_async(request).await?;
    let user_projects = control_call(&mut user_control, 1, "projects.list", json!({})).await;
    assert_eq!(user_projects.as_array().unwrap().len(), 2);

    println!(
        "todo455 live demo: agent project=1 visible_projects=1 in_project_spawn={} cross_project_spawn='{}'; user_control_projects=2",
        spawned_id,
        rejected(
            &client,
            "spawn_process",
            json!({ "project_id": 2, "kind": "terminal", "name": "demo-escape" }),
        )
        .await["message"]
            .as_str()
            .unwrap()
    );

    let _ = user_control.close(None).await;
    let _ = bearer.cancel().await;
    let _ = client.cancel().await;
    let _ = shutdown_tx.send(());
    server_task.await??;
    Ok(())
}
