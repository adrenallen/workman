use std::{
    collections::{BTreeMap, HashMap},
    error::Error,
    time::{Duration, Instant},
};

use axum::http::{HeaderName, HeaderValue};
use gbuild_core::attention::AttentionState;
use gbuild_core::{Process, ProcessKind, ProcessSource, ProcessStatus, Project};
use gbuildd::{DaemonConfig, DaemonServer, GBUILD_MCP_TOKEN_HEADER};
use rmcp::{
    model::{CallToolRequestParams, CallToolResult, ClientInfo},
    transport::{
        streamable_http_client::StreamableHttpClientTransportConfig, StreamableHttpClientTransport,
    },
    ServiceExt,
};
use serde_json::{json, Map, Value};

type Client = rmcp::service::RunningService<rmcp::RoleClient, ClientInfo>;

fn arguments(value: Value) -> Map<String, Value> {
    value
        .as_object()
        .expect("tool arguments must be an object")
        .clone()
}

async fn call_result(client: &Client, name: &'static str, args: Value) -> CallToolResult {
    client
        .call_tool(CallToolRequestParams::new(name).with_arguments(arguments(args)))
        .await
        .unwrap_or_else(|error| panic!("{name} failed: {error}"))
}

async fn call(client: &Client, name: &'static str, args: Value) -> Value {
    let result = call_result(client, name, args).await;
    assert_ne!(result.is_error, Some(true), "{name} returned {result:?}");
    result
        .structured_content
        .unwrap_or_else(|| panic!("{name} returned no structured content"))
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
    }
}

fn paste_sensitive_tui() -> &'static str {
    r#"true claude; stty raw -echo; printf '\033[?2004h❯ '; exec perl -e '$|=1; while (1) { my $n = sysread(STDIN, my $chunk, 4096); exit 2 unless defined($n) && $n > 0; if ($chunk eq "\r") { print "\r\nSUBMITTED\r\nthinking...\r\nesc to interrupt\r\n"; sleep 5; exit 0; } print "\r\nPASTED:$n\r\n"; }'"#
}

async fn wait_for_state(
    registry: &gbuildd::SharedProcessRegistry,
    process_id: i64,
    expected: AttentionState,
) -> Result<(), Box<dyn Error>> {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let state = registry
            .lock()
            .await
            .get_status(process_id)?
            .agent_state
            .state;
        if state == expected {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "process {process_id} did not reach {expected:?}; current state is {state:?}"
            )
            .into());
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[tokio::test]
async fn rmcp_process_tools_cover_lifecycle_output_and_input() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let project_dir = temp.path().join("project");
    std::fs::create_dir_all(&project_dir)?;
    let project = Project {
        id: 1,
        path: project_dir.to_string_lossy().into_owned(),
        name: "process-tools".into(),
        display_name: None,
        icon: None,
        selected: false,
    };

    let server = DaemonServer::bind(DaemonConfig {
        data_dir: temp.path().join("state"),
        port: 0,
    })
    .await?;
    let discovery = server.discovery().clone();
    let registry = server.registry();
    let process_token = {
        let mut registry = registry.lock().await;
        registry.store().put_project(&project)?;
        registry.create(process(
            1,
            &project,
            ProcessKind::Agent,
            "self-agent",
            "sleep 30",
        ))?;
        registry.create(process(
            2,
            &project,
            ProcessKind::Terminal,
            "interactive",
            concat!(
                "printf '\\033[31mREADY\\033[0m\\n'; ",
                "while IFS= read -r line; do printf 'reply:%s\\n' \"$line\"; done"
            ),
        ))?;
        registry.create(process(
            3,
            &project,
            ProcessKind::Command,
            "command-one",
            "sleep 30",
        ))?;
        registry.create(process(
            4,
            &project,
            ProcessKind::Command,
            "command-two",
            "sleep 30",
        ))?;
        registry.create(process(
            5,
            &project,
            ProcessKind::Agent,
            "paste-sensitive-agent",
            paste_sensitive_tui(),
        ))?;
        registry.start(1)?;
        registry.store().connection().query_row(
            "SELECT token FROM process_mcp_tokens WHERE process_id = 1",
            [],
            |row| row.get::<_, String>(0),
        )?
    };

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let server_task = tokio::spawn(server.serve_until(async move {
        let _ = shutdown_rx.await;
    }));
    let headers = HashMap::from([(
        HeaderName::from_static(GBUILD_MCP_TOKEN_HEADER),
        HeaderValue::from_str(&process_token)?,
    )]);
    let transport = StreamableHttpClientTransport::from_config(
        StreamableHttpClientTransportConfig::with_uri(format!(
            "http://127.0.0.1:{}/mcp",
            discovery.port
        ))
        .custom_headers(headers),
    );
    let client = ClientInfo::default().serve(transport).await?;

    let tool_names: Vec<_> = client
        .list_all_tools()
        .await?
        .into_iter()
        .map(|tool| tool.name.into_owned())
        .collect();
    for required in [
        "list_processes",
        "get_process_status",
        "start_process",
        "stop_process",
        "restart_process",
        "close_process",
        "rename_process",
        "select_process",
        "start_all_commands",
        "stop_all_commands",
        "restart_all_commands",
        "get_process_output",
        "get_process_raw_output",
        "search_output",
        "search_raw_output",
        "clear_output",
        "send_input",
    ] {
        assert!(
            tool_names.iter().any(|name| name == required),
            "missing {required}"
        );
    }

    let processes = call(&client, "list_processes", json!({})).await;
    assert_eq!(processes.as_array().unwrap().len(), 5);
    let own_status = call(&client, "get_process_status", json!({})).await;
    assert_eq!(own_status["id"], 1);
    assert!(own_status["agent_state"].is_object());

    call(&client, "start_process", json!({ "process_id": 5 })).await;
    wait_for_state(&registry, 5, AttentionState::Idle).await?;
    let short_prompt = "Reply with exactly PONG.";
    assert!(short_prompt.len() < 100);
    let paste_submit = call(
        &client,
        "send_input",
        json!({
            "process_id": 5,
            "input": short_prompt,
            "submit": true,
            "wait_ms": 250
        }),
    )
    .await;
    assert!(
        paste_submit["fresh_raw_output"]
            .as_str()
            .unwrap()
            .contains("SUBMITTED")
    );
    assert_eq!(paste_submit["status"]["agent_state"]["state"], "working");
    assert!(paste_submit["status"]["agent_state"]["last_input_at"].is_number());

    let started = call(
        &client,
        "start_process",
        json!({ "process_name": "interactive" }),
    )
    .await;
    assert_eq!(started["status"], "running");

    let text_input = call(
        &client,
        "send_input",
        json!({
            "process_id": 2,
            "input": "hello",
            "submit": true,
            "wait_ms": 1
        }),
    )
    .await;
    assert_eq!(text_input["waited_ms"], 250);
    assert!(text_input["fresh_raw_output"]
        .as_str()
        .unwrap()
        .contains("reply:hello"));

    let unsubmitted = call(
        &client,
        "send_input",
        json!({ "process_id": 2, "input": "partial", "submit": false }),
    )
    .await;
    assert_eq!(unsubmitted["bytes_sent"], 7);
    assert_eq!(unsubmitted["output"], Value::Null);
    let submitted = call(
        &client,
        "send_input",
        json!({ "process_id": 2, "bytes": [13], "wait_ms": 250 }),
    )
    .await;
    assert!(submitted["fresh_raw_output"]
        .as_str()
        .unwrap()
        .contains("reply:partial"));

    let raw_input = call(
        &client,
        "send_input",
        json!({ "process_id": 2, "bytes": [114, 97, 119, 13], "wait_ms": 250 }),
    )
    .await;
    assert!(raw_input["fresh_raw_output"]
        .as_str()
        .unwrap()
        .contains("reply:raw"));

    let rendered = call(
        &client,
        "get_process_output",
        json!({ "process_id": 2, "lines": 10 }),
    )
    .await;
    assert!(rendered["output"].as_str().unwrap().contains("reply:raw"));
    let ranged = call(
        &client,
        "get_process_output",
        json!({ "process_id": 2, "start_row": 0, "end_row": 5 }),
    )
    .await;
    assert!(ranged["end_row"].as_u64().unwrap() <= 5);

    let raw = call(
        &client,
        "get_process_raw_output",
        json!({ "process_id": 2, "lines": 20 }),
    )
    .await;
    assert!(!raw["data_base64"].as_str().unwrap().is_empty());
    assert!(raw["output"].as_str().unwrap().contains("reply:raw"));

    let rendered_matches = call(
        &client,
        "search_output",
        json!({ "process_id": 2, "pattern": "REPLY:RAW" }),
    )
    .await;
    assert!(!rendered_matches["matches"].as_array().unwrap().is_empty());
    let raw_matches = call(
        &client,
        "search_raw_output",
        json!({ "process_id": 2, "pattern": "REPLY:RAW" }),
    )
    .await;
    assert!(!raw_matches["matches"].as_array().unwrap().is_empty());

    let selected = call(&client, "select_process", json!({ "process_id": 2 })).await;
    assert_eq!(selected["selected_process_id"], 2);
    let renamed = call(
        &client,
        "rename_process",
        json!({ "process_id": 2, "new_name": "shell" }),
    )
    .await;
    assert_eq!(renamed["name"], "shell");

    let cleared = call(&client, "clear_output", json!({ "process_id": 2 })).await;
    assert_eq!(cleared["cleared"], true);
    let cleared_search = call(
        &client,
        "search_output",
        json!({ "process_id": 2, "pattern": "reply:raw" }),
    )
    .await;
    assert!(cleared_search["matches"].as_array().unwrap().is_empty());
    let cleared_raw_search = call(
        &client,
        "search_raw_output",
        json!({ "process_id": 2, "pattern": "reply:raw" }),
    )
    .await;
    assert!(cleared_raw_search["matches"].as_array().unwrap().is_empty());
    let cleared_raw = call(
        &client,
        "get_process_raw_output",
        json!({ "process_id": 2 }),
    )
    .await;
    assert!(cleared_raw["data_base64"].as_str().unwrap().is_empty());

    assert_eq!(
        call(&client, "stop_process", json!({ "process_name": "shell" })).await["status"],
        "stopped"
    );
    assert_eq!(
        call(&client, "restart_process", json!({ "process_id": 2 })).await["status"],
        "running"
    );
    call(&client, "stop_process", json!({ "process_id": 2 })).await;

    for tool in [
        "start_all_commands",
        "restart_all_commands",
        "stop_all_commands",
    ] {
        let result = call(&client, tool, json!({})).await;
        assert_eq!(result["processes"].as_array().unwrap().len(), 2);
        assert!(result["failures"].as_array().unwrap().is_empty());
    }

    let closed = call(&client, "close_process", json!({ "process_id": 2 })).await;
    assert_eq!(closed["closed"], true);
    let guarded = call_result(&client, "close_process", json!({ "process_id": 1 })).await;
    assert_eq!(guarded.is_error, Some(true));
    assert_eq!(
        guarded.structured_content.unwrap()["code"],
        "self_close_confirmation_required"
    );
    let self_closed = call(
        &client,
        "close_process",
        json!({ "process_id": 1, "confirm_self_close": true }),
    )
    .await;
    assert_eq!(self_closed["closed"], true);

    let _ = client.cancel().await;
    let _ = shutdown_tx.send(());
    server_task.await??;
    Ok(())
}
