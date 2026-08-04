#![cfg(unix)]

use std::{
    collections::{BTreeMap, HashMap},
    error::Error,
    io::Write,
    time::Duration,
};

use axum::http::{HeaderName, HeaderValue};
use gbuild_core::{Process, ProcessKind, ProcessSource, ProcessStatus, Project};
use gbuildd::{DaemonConfig, DaemonServer, GBUILD_MCP_TOKEN_HEADER};
use rmcp::{
    ServiceExt,
    model::{CallToolRequestParams, ClientInfo},
    transport::{
        StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use serde_json::{Map, Value, json};

const HELPER_ENV: &str = "GBUILD_MCP_READINESS_HELPER";
const HELPER_DELAY_ENV: &str = "GBUILD_MCP_READINESS_DELAY_MS";

type Client = rmcp::service::RunningService<rmcp::RoleClient, ClientInfo>;

#[test]
fn listener_helper() {
    if std::env::var(HELPER_ENV).as_deref() != Ok("1") {
        return;
    }
    let delay = std::env::var(HELPER_DELAY_ENV)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    std::thread::sleep(Duration::from_millis(delay));
    let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
    println!("bound:{}", listener.local_addr().unwrap().port());
    std::io::stdout().flush().unwrap();
    std::thread::sleep(Duration::from_secs(30));
    drop(listener);
}

fn arguments(value: Value) -> Map<String, Value> {
    value
        .as_object()
        .expect("tool arguments must be an object")
        .clone()
}

async fn call(client: &Client, name: &'static str, args: Value) -> Value {
    let result = client
        .call_tool(CallToolRequestParams::new(name).with_arguments(arguments(args)))
        .await
        .unwrap_or_else(|error| panic!("{name} failed: {error}"));
    assert_ne!(result.is_error, Some(true), "{name} returned {result:?}");
    let structured = result
        .structured_content
        .as_ref()
        .unwrap_or_else(|| panic!("{name} returned no structured content"));
    assert!(
        structured.is_object(),
        "{name} returned non-object structured content: {structured}"
    );
    let text = result
        .content
        .iter()
        .find_map(|content| content.as_text())
        .unwrap_or_else(|| panic!("{name} returned no text content"));
    assert_eq!(
        serde_json::from_str::<Value>(&text.text).unwrap(),
        *structured,
        "{name} text content diverged from structured content"
    );
    structured.clone()
}

fn self_process(project: &Project) -> Process {
    Process {
        id: 1,
        project_id: project.id,
        kind: ProcessKind::Agent,
        name: "test-agent".into(),
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
    }
}

fn helper_command(delay: Duration) -> String {
    let executable = std::env::current_exe().unwrap();
    format!(
        "{HELPER_ENV}=1 {HELPER_DELAY_ENV}={} {} --exact listener_helper --nocapture",
        delay.as_millis(),
        shell_quote(&executable.to_string_lossy())
    )
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rmcp_readiness_tools_drive_restart_wait_and_report_url() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let project_dir = temp.path().join("project");
    std::fs::create_dir_all(&project_dir)?;
    let project = Project {
        id: 1,
        path: project_dir.to_string_lossy().into_owned(),
        name: "readiness-tools".into(),
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
        registry.create(self_process(&project))?;
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

    let tool_names = client
        .list_all_tools()
        .await?
        .into_iter()
        .map(|tool| tool.name.into_owned())
        .collect::<Vec<_>>();
    for required in ["services_list", "get_process_ports", "wait_for_bound_port"] {
        assert!(
            tool_names.iter().any(|name| name == required),
            "missing {required}"
        );
    }

    let spawned = call(
        &client,
        "spawn_process",
        json!({ "kind": "terminal", "name": "dev-server" }),
    )
    .await;
    let dev_process_id = spawned["process_id"].as_i64().unwrap();

    let empty_wait = call(
        &client,
        "wait_for_bound_port",
        json!({ "process_id": dev_process_id, "timeout_ms": 50 }),
    )
    .await;
    assert_eq!(empty_wait["ready"], false);
    assert_eq!(empty_wait["timed_out"], true);
    assert_eq!(empty_wait["services"], json!([]));

    call(
        &client,
        "send_input",
        json!({
            "process_id": dev_process_id,
            "input": helper_command(Duration::from_millis(250)),
            "submit": true
        }),
    )
    .await;
    let first_ready = call(
        &client,
        "wait_for_bound_port",
        json!({ "process_id": dev_process_id, "timeout_ms": 8_000 }),
    )
    .await;
    assert_eq!(first_ready["ready"], true);
    assert_eq!(first_ready["timed_out"], false);

    let restarted = call(
        &client,
        "restart_process",
        json!({ "process_id": dev_process_id }),
    )
    .await;
    assert_eq!(restarted["status"], "running");
    let waiting = call(
        &client,
        "get_process_ports",
        json!({ "process_name": "dev-server" }),
    )
    .await;
    assert_eq!(waiting["readiness"], "waiting");

    call(
        &client,
        "send_input",
        json!({
            "process_name": "dev-server",
            "input": helper_command(Duration::from_millis(250)),
            "submit": true
        }),
    )
    .await;
    let ready = call(
        &client,
        "wait_for_bound_port",
        json!({ "process_name": "dev-server", "timeout_ms": 8_000 }),
    )
    .await;
    assert_eq!(ready["ready"], true);
    let url = ready["services"][0]["urls"][0].as_str().unwrap().to_owned();
    assert!(url.starts_with("http://localhost:"));

    let ports = call(
        &client,
        "get_process_ports",
        json!({ "process_id": dev_process_id }),
    )
    .await;
    assert_eq!(ports["readiness"], "ready");
    assert!(ports["ports"][0].as_u64().is_some());
    assert!(
        ports["urls"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == &url)
    );

    let services = call(&client, "services_list", json!({})).await;
    let dev_service = services
        .get("services")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .find(|service| service["process_id"] == dev_process_id)
        .unwrap();
    assert_eq!(dev_service["readiness"], "ready");
    assert!(
        dev_service["urls"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == &url)
    );

    let closed = call(
        &client,
        "close_process",
        json!({ "process_id": dev_process_id }),
    )
    .await;
    assert_eq!(closed["closed"], true);
    let _ = client.cancel().await;
    let _ = shutdown_tx.send(());
    server_task.await??;
    Ok(())
}
