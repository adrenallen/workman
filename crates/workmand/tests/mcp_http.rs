use std::{collections::BTreeMap, error::Error};

use rmcp::{
    ServiceExt,
    model::{CallToolRequestParams, ClientInfo},
    transport::{
        StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use serde_json::{Map, Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use workman_core::{Process, ProcessKind, ProcessSource, ProcessStatus, Project};
use workmand::{DaemonConfig, DaemonServer};

async fn raw_mcp_post(
    port: u16,
    path: &str,
    token: Option<&str>,
    session_id: Option<&str>,
    body: &str,
) -> std::io::Result<String> {
    let authorization = token
        .map(|token| format!("Authorization: Bearer {token}\r\n"))
        .unwrap_or_default();
    let session = session_id
        .map(|session_id| format!("Mcp-Session-Id: {session_id}\r\n"))
        .unwrap_or_default();
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n{authorization}Accept: application/json, text/event-stream\r\nContent-Type: application/json\r\n{session}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port)).await?;
    stream.write_all(request.as_bytes()).await?;
    let mut response = String::new();
    stream.read_to_string(&mut response).await?;
    Ok(response)
}

async fn raw_mcp_get(port: u16, token: &str, path: &str) -> std::io::Result<String> {
    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {token}\r\nAccept: text/event-stream\r\nConnection: close\r\n\r\n"
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

    let response = raw_mcp_post(
        discovery.port,
        "/mcp",
        Some(&discovery.token),
        Some("bogus-session-id"),
        r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}"#,
    )
    .await?;
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

fn assert_schema_hygiene(value: &Value) {
    match value {
        Value::Array(values) => {
            for value in values {
                assert_schema_hygiene(value);
            }
        }
        Value::Object(object) => {
            assert!(!object.contains_key("$schema"));
            assert_ne!(object.get("default"), Some(&Value::Null));
            if let Some(Value::Object(properties)) = object.get("properties") {
                assert!(!properties.contains_key("project_id"));
            }
            if let Some(Value::Array(types)) = object.get("type") {
                assert!(!types.iter().any(Value::is_null));
                assert!(!types.iter().any(|value| value == "null"));
            }
            if object.get("type") == Some(&Value::String("integer".into())) {
                assert!(!object.contains_key("format"));
                assert_ne!(object.get("minimum").and_then(Value::as_i64), Some(0));
            }
            for value in object.values() {
                assert_schema_hygiene(value);
            }
        }
        _ => {}
    }
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
    std::fs::create_dir_all(&project_one_dir)?;
    std::fs::create_dir_all(&project_two_dir)?;

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
    let stateless_endpoint = format!("http://127.0.0.1:{}/mcp-stateless", discovery.port);

    let stateless_client = {
        let transport = StreamableHttpClientTransport::from_config(
            StreamableHttpClientTransportConfig::with_uri(stateless_endpoint)
                .auth_header(process_token.clone()),
        );
        ClientInfo::default().serve(transport).await?
    };
    let stateless_identity = call(&stateless_client, "whoami", json!({})).await;
    assert_eq!(stateless_identity["process_id"], 42);
    assert_eq!(stateless_identity["session_id"], "process:42");
    assert!(!stateless_client.list_all_tools().await?.is_empty());
    let _ = stateless_client.cancel().await;

    let idle_get = raw_mcp_get(discovery.port, &process_token, "/mcp-stateless").await?;
    assert!(
        idle_get.starts_with("HTTP/1.1 405 Method Not Allowed\r\n"),
        "stateless MCP must decline the idle SSE stream without creating a reconnectable body: {idle_get:?}"
    );

    for token in [None, Some("not-a-live-token")] {
        let response = raw_mcp_post(
            discovery.port,
            "/mcp-stateless",
            token,
            None,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#,
        )
        .await?;
        assert!(
            response.starts_with("HTTP/1.1 401 Unauthorized\r\n"),
            "stateless MCP accepted missing or invalid authentication: {response:?}"
        );
    }
    let forged_session = raw_mcp_post(
        discovery.port,
        "/mcp-stateless",
        Some(&process_token),
        Some("process:999"),
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"whoami","arguments":{}}}"#,
    )
    .await?;
    assert!(forged_session.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(forged_session.contains(r#"\"session_id\":\"process:42\""#));
    assert!(forged_session.contains(r#"\"process_id\":42"#));

    let actors_before_processless_call: i64 = registry
        .lock()
        .await
        .store()
        .connection()
        .query_row("SELECT COUNT(*) FROM actors", [], |row| row.get(0))?;
    let processless_call = raw_mcp_post(
        discovery.port,
        "/mcp-stateless",
        Some(&discovery.token),
        None,
        r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"whoami","arguments":{}}}"#,
    )
    .await?;
    assert!(processless_call.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(
        processless_call
            .contains("sessionless MCP tool calls require an active process credential")
    );
    let actors_after_processless_call: i64 = registry.lock().await.store().connection().query_row(
        "SELECT COUNT(*) FROM actors",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(
        actors_after_processless_call,
        actors_before_processless_call
    );

    let process_client = {
        let transport = StreamableHttpClientTransport::from_config(
            StreamableHttpClientTransportConfig::with_uri(endpoint.clone())
                .auth_header(process_token.clone()),
        );
        ClientInfo::default().serve(transport).await?
    };

    let server_instructions = process_client
        .peer_info()
        .and_then(|info| info.instructions.clone())
        .expect("Workman advertises MCP server instructions");
    assert!(server_instructions.len() <= 800);
    assert!(server_instructions.starts_with("Need human input"));
    assert!(server_instructions.contains("todo_assign(assignee=\"user\")"));
    assert!(server_instructions.contains("mention @user"));
    assert!(server_instructions.contains("Call whoami first"));
    assert!(server_instructions.contains("Use help for detailed guidance"));

    let tools = process_client.list_all_tools().await?;
    for tool in &tools {
        assert_schema_hygiene(&Value::Object((*tool.input_schema).clone()));
    }
    let tool_names: Vec<_> = tools
        .into_iter()
        .into_iter()
        .map(|tool| tool.name.into_owned())
        .collect();
    for required in [
        "whoami",
        "help",
        "mcp_tools_summary",
        "list_projects",
        "get_project",
        "get_project_status",
        "get_project_stats",
        "rename_project",
    ] {
        assert!(
            tool_names.iter().any(|name| name == required),
            "missing {required}"
        );
    }
    for removed in [
        "identify_session",
        "mcp_smoke_test",
        "select_project",
        "create_project",
        "delete_project",
        "agent_tool_configure_preview",
        "agent_tool_configure",
    ] {
        assert!(
            !tool_names.iter().any(|name| name == removed),
            "unexpected {removed}"
        );
    }

    let identity = call(&process_client, "whoami", json!({})).await;
    assert_eq!(identity["process_id"], 42);
    assert_eq!(identity["effective_project_id"], 1);
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
    let tools_help = call(&process_client, "help", json!({ "topic": "tools" })).await;
    assert!(
        tools_help["text"]
            .as_str()
            .unwrap()
            .contains("complete core tool list")
    );
    let spawning_help = call(&process_client, "help", json!({ "topic": "spawning" })).await;
    assert!(
        spawning_help["text"]
            .as_str()
            .unwrap()
            .contains("only when the user names a template or explicitly asks for one")
    );
    assert!(
        spawning_help["text"]
            .as_str()
            .unwrap()
            .contains("pick agent_tool_id from list_agent_tools")
    );
    assert!(
        spawning_help["text"]
            .as_str()
            .unwrap()
            .contains("set_notify_spawner_on_idle can toggle an existing direct child")
    );
    let timer_help = call(&process_client, "help", json!({ "topic": "timers" })).await;
    assert!(
        timer_help["text"]
            .as_str()
            .unwrap()
            .contains("immediately finish your response and end the current turn")
    );
    assert!(
        timer_help["text"]
            .as_str()
            .unwrap()
            .contains("no additional wait call is needed")
    );
    assert!(
        timer_help["text"]
            .as_str()
            .unwrap()
            .contains("no timer is necessary")
    );
    for guidance in [
        "opt-in is prospective",
        "spawner's last submitted input",
        "explicit pending idle timer wins",
        "human's unsent draft",
        "delay timer when a hung-child deadline matters",
        "parked Waiting on its own timer is not reported finished",
    ] {
        assert!(
            timer_help["text"].as_str().unwrap().contains(guidance),
            "timer help omitted {guidance:?}"
        );
    }
    let tools_summary = call(&process_client, "mcp_tools_summary", json!({})).await;
    assert!(tools_summary["count"].as_u64().unwrap() >= 13);
    assert!(
        tools_summary["spawn_agent_guidance"]
            .as_str()
            .unwrap()
            .contains("Prefer model for a per-launch override")
    );
    assert!(
        tools_summary["idle_timer_wait_guidance"]
            .as_str()
            .unwrap()
            .contains("Do not loop on timer_list or process status")
    );
    assert_eq!(
        call(&process_client, "list_projects", json!({})).await["projects"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    let own_project = call(&process_client, "get_project", json!({})).await;
    assert_eq!(own_project["id"], 1);
    let selected_project = call(&process_client, "get_project", json!({})).await;
    assert_eq!(selected_project["id"], 1);
    let explicit_project = call(&process_client, "get_project", json!({ "project_id": 1 })).await;
    assert_eq!(explicit_project["id"], 1);

    let status = call(&process_client, "get_project_status", json!({})).await;
    assert_eq!(status["project"]["id"], 1);
    let stats = call(
        &process_client,
        "get_project_stats",
        json!({ "project_id": 1 }),
    )
    .await;
    assert_eq!(stats["process_count"], 1);

    let renamed = call(
        &process_client,
        "rename_project",
        json!({ "project_id": 1, "name": "renamed" }),
    )
    .await;
    assert_eq!(renamed["name"], "one");
    assert_eq!(renamed["display_name"], "renamed");

    let created_todo = call(
        &process_client,
        "todo_create",
        json!({ "title": "keep this MCP session connected" }),
    )
    .await;
    assert!(created_todo["todo_id"].as_i64().unwrap() > 0);
    let spawned = call(
        &process_client,
        "spawn_process",
        json!({ "kind": "terminal", "name": "transport-regression" }),
    )
    .await;
    let spawned_id = spawned["process_id"].as_i64().unwrap();
    assert_eq!(
        call(
            &process_client,
            "close_process",
            json!({ "process_id": spawned_id }),
        )
        .await["closed"],
        true
    );
    let identity_after_mutations = call(&process_client, "whoami", json!({})).await;
    assert_eq!(identity_after_mutations["process_id"], 42);
    assert_eq!(
        identity_after_mutations["session_id"], identity["session_id"],
        "todo and process mutations must not reset the Streamable HTTP session"
    );

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
    let fallback_tool_names = fallback_client
        .list_all_tools()
        .await?
        .into_iter()
        .map(|tool| tool.name.into_owned())
        .collect::<Vec<_>>();
    assert!(
        fallback_tool_names
            .iter()
            .any(|name| name == "agent_tool_configure_preview")
    );
    assert!(
        fallback_tool_names
            .iter()
            .any(|name| name == "agent_tool_configure")
    );
    assert_eq!(
        call(&fallback_client, "list_projects", json!({})).await["projects"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let unidentified_scope = fallback_client
        .call_tool(
            CallToolRequestParams::new("get_project")
                .with_arguments(arguments(json!({ "project_id": 2 }))),
        )
        .await?;
    assert_eq!(unidentified_scope.is_error, Some(true));
    assert!(
        unidentified_scope.structured_content.unwrap()["message"]
            .as_str()
            .unwrap()
            .contains("authenticated process identity")
    );
    assert_eq!(
        call(&fallback_client, "whoami", json!({})).await["process_id"],
        Value::Null
    );
    assert_eq!(
        call(&fallback_client, "list_projects", json!({})).await["projects"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let _ = fallback_client.cancel().await;

    registry.lock().await.stop(42)?;
    let _ = shutdown_tx.send(());
    server_task.await??;
    Ok(())
}
