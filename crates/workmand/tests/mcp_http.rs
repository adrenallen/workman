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
use workman_core::{McpToolsProfile, Process, ProcessKind, ProcessSource, ProcessStatus, Project};
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
        registry
            .store()
            .set_process_mcp_tools_profile(42, McpToolsProfile::Extended)?;
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
    assert!(server_instructions.contains("todo_update(assignee=\"user\")"));
    assert!(server_instructions.contains("mention @user"));
    assert!(server_instructions.contains("Call whoami first"));
    assert!(server_instructions.contains("Use help for todos"));

    let tools = process_client.list_all_tools().await?;
    let tools_list_bytes = serde_json::to_vec(&json!({ "tools": &tools }))?.len();
    eprintln!("tools/list compact JSON bytes: {tools_list_bytes}");
    // 26,480 bytes at introduction, with roughly ten percent growth headroom.
    const TOOLS_LIST_BUDGET_BYTES: usize = 29_200;
    assert!(
        tools_list_bytes <= TOOLS_LIST_BUDGET_BYTES,
        "tools/list grew beyond its size budget: {tools_list_bytes} bytes"
    );
    for tool in &tools {
        assert_schema_hygiene(&Value::Object((*tool.input_schema).clone()));
    }
    let mut tool_names: Vec<_> = tools
        .into_iter()
        .map(|tool| tool.name.into_owned())
        .collect();
    tool_names.sort();
    let mut expected = vec![
        "agent_tool_check",
        "clear_output",
        "close_process",
        "commands_control",
        "get_process_output",
        "get_process_status",
        "help",
        "list_agent_tools",
        "list_processes",
        "lock",
        "process_control",
        "project_update",
        "scratchpad_append",
        "scratchpad_comment_create",
        "scratchpad_comment_delete",
        "scratchpad_comment_update",
        "scratchpad_delete",
        "scratchpad_edit",
        "scratchpad_find",
        "scratchpad_list",
        "scratchpad_load_from_file",
        "scratchpad_read",
        "scratchpad_save_to_file",
        "scratchpad_update",
        "scratchpad_write",
        "search_output",
        "send_input",
        "services_list",
        "spawn_agent",
        "spawn_terminal",
        "timer_cancel",
        "timer_fire_when_idle",
        "timer_list",
        "timer_pause",
        "timer_set",
        "todo_comment_create",
        "todo_comment_delete",
        "todo_comment_update",
        "todo_complete",
        "todo_create",
        "todo_delete",
        "todo_get",
        "todo_list",
        "todo_lock",
        "todo_unlock",
        "todo_update",
        "update_process",
        "wait_for_bound_port",
        "whoami",
        "worktree_env_forget",
        "worktree_health",
        "worktree_list",
    ];
    expected.sort();
    assert_eq!(tool_names, expected);
    for required in ["whoami", "help", "project_update"] {
        assert!(
            tool_names.iter().any(|name| name == required),
            "missing {required}"
        );
    }
    let identity = call(&process_client, "whoami", json!({})).await;
    assert_eq!(identity["process_id"], 42);
    assert_eq!(identity["effective_project_id"], 1);
    assert_eq!(identity["project"]["id"], 1);
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
            .contains("fixed when an agent launches")
    );
    assert!(
        tools_help["text"]
            .as_str()
            .unwrap()
            .contains("Extended adds")
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
            .contains("update_process can toggle an existing direct child")
    );
    let timer_help = call(&process_client, "help", json!({ "topic": "timers" })).await;
    assert!(
        timer_help["text"]
            .as_str()
            .unwrap()
            .contains("finish the response and end the turn")
    );
    assert!(timer_help["text"].as_str().unwrap().contains("do not poll"));
    assert!(
        timer_help["text"]
            .as_str()
            .unwrap()
            .contains("no timer is needed")
    );
    for guidance in [
        "opt-in is prospective",
        "delay timer when a hung-child deadline matters",
        "wait_for=\"any\" or wait_for=\"all\"",
    ] {
        assert!(
            timer_help["text"].as_str().unwrap().contains(guidance),
            "timer help omitted {guidance:?}"
        );
    }
    let stats = call(&process_client, "list_processes", json!({})).await;
    assert_eq!(stats["process_count"], 1);

    let renamed = call(
        &process_client,
        "project_update",
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
        "spawn_terminal",
        json!({ "name": "transport-regression" }),
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
            .any(|name| name == "agent_tool_configure")
    );
    let unidentified_scope = fallback_client
        .call_tool(
            CallToolRequestParams::new("project_update")
                .with_arguments(arguments(json!({ "project_id": 2, "name": "foreign" }))),
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
    let _ = fallback_client.cancel().await;

    registry.lock().await.stop(42)?;
    let _ = shutdown_tx.send(());
    server_task.await??;
    Ok(())
}
