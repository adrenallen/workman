use std::{
    collections::{BTreeMap, HashMap},
    error::Error,
};

use awm_core::{Process, ProcessKind, ProcessSource, ProcessStatus, Project};
use awmd::{AWM_MCP_TOKEN_HEADER, DaemonConfig, DaemonServer};
use axum::http::{HeaderName, HeaderValue};
use rmcp::{
    ServiceExt,
    model::{CallToolRequestParams, ClientInfo},
    transport::{
        StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use serde_json::{Map, Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn raw_mcp_post(port: u16, token: &str, session_id: &str) -> std::io::Result<String> {
    let body = r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}"#;
    let request = format!(
        "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {token}\r\nAccept: application/json, text/event-stream\r\nContent-Type: application/json\r\nMcp-Session-Id: {session_id}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port)).await?;
    stream.write_all(request.as_bytes()).await?;
    let mut response = String::new();
    stream.read_to_string(&mut response).await?;
    Ok(response)
}

#[tokio::test]
async fn unknown_mcp_session_returns_404() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let server = DaemonServer::bind(DaemonConfig {
        data_dir: temp.path().join("state"),
        port: 0,
    })
    .await?;
    let discovery = server.discovery().clone();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let server_task = tokio::spawn(server.serve_until(async move {
        let _ = shutdown_rx.await;
    }));

    let response = raw_mcp_post(discovery.port, &discovery.token, "bogus-session-id").await?;
    assert!(
        response.starts_with("HTTP/1.1 404 Not Found\r\n"),
        "unknown MCP session response was {response:?}"
    );
    assert!(response.contains("Session not found"));

    let _ = shutdown_tx.send(());
    server_task.await??;
    Ok(())
}

fn arguments(value: Value) -> Map<String, Value> {
    value
        .as_object()
        .expect("tool arguments must be an object")
        .clone()
}

async fn call(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ClientInfo>,
    name: &'static str,
    arguments_value: Value,
) -> Value {
    let result = client
        .call_tool(CallToolRequestParams::new(name).with_arguments(arguments(arguments_value)))
        .await
        .unwrap_or_else(|error| panic!("{name} failed: {error}"));
    assert_ne!(result.is_error, Some(true), "{name} returned an error");
    result
        .structured_content
        .unwrap_or_else(|| panic!("{name} returned no structured content"))
}

#[tokio::test]
async fn rmcp_client_reaches_mcp_and_resolves_process_and_project_scope()
-> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let project_one_dir = temp.path().join("one");
    let project_two_dir = temp.path().join("two");
    let project_three_dir = temp.path().join("three");
    std::fs::create_dir_all(&project_one_dir)?;
    std::fs::create_dir_all(&project_two_dir)?;
    std::fs::create_dir_all(&project_three_dir)?;

    let server = DaemonServer::bind(DaemonConfig {
        data_dir: temp.path().join("state"),
        port: 0,
    })
    .await?;
    let discovery = server.discovery().clone();
    let registry = server.registry();
    let process_token = {
        let mut registry = registry.lock().await;
        registry.store().put_project(&Project {
            id: 1,
            path: project_one_dir.to_string_lossy().into_owned(),
            name: "one".into(),
            display_name: None,
            icon: None,
            selected: false,
            sort_order: 0,
        })?;
        registry.store().put_project(&Project {
            id: 2,
            path: project_two_dir.to_string_lossy().into_owned(),
            name: "two".into(),
            display_name: None,
            icon: None,
            selected: false,
            sort_order: 0,
        })?;
        registry.store().put_process(&Process {
            id: 42,
            project_id: 1,
            kind: ProcessKind::Agent,
            name: "agent".into(),
            command: Some("sleep 30".into()),
            working_dir: project_one_dir.to_string_lossy().into_owned(),
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
        })?;
        registry.start(42)?;
        registry.store().connection().query_row(
            "SELECT token FROM process_mcp_tokens WHERE process_id = 42",
            [],
            |row| row.get::<_, String>(0),
        )?
    };

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let server_task = tokio::spawn(server.serve_until(async move {
        let _ = shutdown_rx.await;
    }));
    let endpoint = format!("http://127.0.0.1:{}/mcp", discovery.port);

    let process_client = {
        let headers = HashMap::from([(
            HeaderName::from_static(AWM_MCP_TOKEN_HEADER),
            HeaderValue::from_str(&process_token)?,
        )]);
        let transport = StreamableHttpClientTransport::from_config(
            StreamableHttpClientTransportConfig::with_uri(endpoint.clone()).custom_headers(headers),
        );
        ClientInfo::default().serve(transport).await?
    };

    let tool_names: Vec<_> = process_client
        .list_all_tools()
        .await?
        .into_iter()
        .map(|tool| tool.name.into_owned())
        .collect();
    for required in [
        "whoami",
        "help",
        "mcp_tools_summary",
        "mcp_smoke_test",
        "list_projects",
        "select_project",
        "get_project",
        "get_project_status",
        "get_project_stats",
        "create_project",
        "rename_project",
        "delete_project",
    ] {
        assert!(
            tool_names.iter().any(|name| name == required),
            "missing {required}"
        );
    }

    let identity = call(&process_client, "whoami", json!({})).await;
    assert_eq!(identity["process_id"], 42);
    assert_eq!(identity["effective_project_id"], 1);
    let identity_cannot_be_retargeted = call(
        &process_client,
        "identify_session",
        json!({ "process_id": 999 }),
    )
    .await;
    assert_eq!(identity_cannot_be_retargeted["process_id"], 42);
    assert_eq!(
        call(&process_client, "help", json!({ "topic": "scoping" })).await["topic"],
        "scoping"
    );
    let scratchpad_help = call(&process_client, "help", json!({ "topic": "scratchpads" })).await;
    assert!(
        scratchpad_help["text"]
            .as_str()
            .unwrap()
            .contains("shared notes, plans, briefs, and hand-offs")
    );
    assert!(
        scratchpad_help["text"]
            .as_str()
            .unwrap()
            .contains("read it back with scratchpad_read or todo_get and reference its ID")
    );
    assert!(
        call(&process_client, "mcp_tools_summary", json!({})).await["count"]
            .as_u64()
            .unwrap()
            >= 13
    );
    assert_eq!(
        call(&process_client, "list_projects", json!({})).await["projects"]
            .as_array()
            .unwrap()
            .len(),
        2
    );

    let own_project = call(&process_client, "get_project", json!({})).await;
    assert_eq!(own_project["id"], 1);
    call(
        &process_client,
        "select_project",
        json!({ "project_id": 2 }),
    )
    .await;
    let selected_project = call(&process_client, "get_project", json!({})).await;
    assert_eq!(selected_project["id"], 2);
    let explicit_project = call(&process_client, "get_project", json!({ "project_id": 1 })).await;
    assert_eq!(explicit_project["id"], 1);

    let status = call(&process_client, "get_project_status", json!({})).await;
    assert_eq!(status["project"]["id"], 2);
    let stats = call(
        &process_client,
        "get_project_stats",
        json!({ "project_id": 1 }),
    )
    .await;
    assert_eq!(stats["process_count"], 1);

    let created = call(
        &process_client,
        "create_project",
        json!({ "path": project_three_dir, "name": "three" }),
    )
    .await;
    let created_id = created["id"].as_i64().unwrap();
    let created_again = call(
        &process_client,
        "create_project",
        json!({ "path": project_three_dir }),
    )
    .await;
    assert_eq!(created_again["id"], created_id);
    let renamed = call(
        &process_client,
        "rename_project",
        json!({ "project_id": created_id, "name": "renamed" }),
    )
    .await;
    assert_eq!(renamed["name"], "renamed");

    let unconfirmed = process_client
        .call_tool(
            CallToolRequestParams::new("delete_project")
                .with_arguments(arguments(json!({ "project_id": created_id }))),
        )
        .await?;
    assert_eq!(unconfirmed.is_error, Some(true));
    assert_eq!(
        unconfirmed.structured_content.unwrap()["code"],
        "confirmation_required"
    );
    let deleted = call(
        &process_client,
        "delete_project",
        json!({ "project_id": created_id, "confirm_delete": true }),
    )
    .await;
    assert_eq!(deleted["deleted"], true);

    let smoke = call(&process_client, "mcp_smoke_test", json!({})).await;
    assert_eq!(smoke["ok"], true);

    let session_id = identity["session_id"].as_str().unwrap();
    assert!(
        registry
            .lock()
            .await
            .store()
            .get_actor_by_session_id(session_id)?
            .is_some()
    );
    let _ = process_client.cancel().await;

    let fallback_client = {
        let transport = StreamableHttpClientTransport::from_config(
            StreamableHttpClientTransportConfig::with_uri(endpoint)
                .auth_header(discovery.token.clone()),
        );
        ClientInfo::default().serve(transport).await?
    };
    let unidentified = call(&fallback_client, "whoami", json!({})).await;
    assert_eq!(unidentified["process_id"], Value::Null);
    let identified = call(
        &fallback_client,
        "identify_session",
        json!({ "process_id": 42 }),
    )
    .await;
    assert_eq!(identified["process_id"], 42);
    assert_eq!(identified["effective_project_id"], 1);
    let _ = fallback_client.cancel().await;

    registry.lock().await.stop(42)?;
    let _ = shutdown_tx.send(());
    server_task.await??;
    Ok(())
}
