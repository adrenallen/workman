use std::{
    collections::{BTreeMap, HashMap},
    error::Error,
    time::{Duration, Instant},
};

use axum::http::{HeaderName, HeaderValue};
use gbuild_core::{
    AgentTool, AgentToolSource, Process, ProcessKind, ProcessSource, ProcessStatus, Project,
    attention::AttentionState,
};
use gbuildd::{DaemonConfig, DaemonServer, GBUILD_MCP_TOKEN_HEADER};
use rmcp::{
    ServiceExt,
    model::{CallToolRequestParams, ClientInfo},
    transport::{
        StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use serde_json::{Map, Value, json};

const PROJECT_ID: i64 = 7;
const DELIVERY_ID: i64 = 20;
const WORKER_ID: i64 = 21;
const STALLED_ID: i64 = 22;

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

async fn wait_for_output(
    registry: &gbuildd::SharedProcessRegistry,
    process_id: i64,
    needle: &str,
) -> Result<(), Box<dyn Error>> {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let output = registry.lock().await.rendered_output(process_id)?.text;
        if output.contains(needle) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!("process {process_id} output did not contain {needle:?}").into());
        }
        tokio::time::sleep(Duration::from_millis(15)).await;
    }
}

async fn output_contains(
    registry: &gbuildd::SharedProcessRegistry,
    process_id: i64,
    needle: &str,
) -> Result<bool, Box<dyn Error>> {
    Ok(registry
        .lock()
        .await
        .rendered_output(process_id)?
        .text
        .contains(needle))
}

async fn wait_for_state(
    registry: &gbuildd::SharedProcessRegistry,
    process_id: i64,
    expected: AttentionState,
) -> Result<(), Box<dyn Error>> {
    let deadline = Instant::now() + Duration::from_secs(4);
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
        tokio::time::sleep(Duration::from_millis(15)).await;
    }
}

fn process(id: i64, name: &str, command: &str, agent_tool_id: Option<i64>) -> Process {
    Process {
        id,
        project_id: PROJECT_ID,
        kind: ProcessKind::Agent,
        name: name.into(),
        command: Some(command.into()),
        working_dir: "/tmp".into(),
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
        agent_tool_id,
        spawned_by_process_id: None,
        sort_order: 0,
    }
}

#[tokio::test]
async fn mcp_timers_deliver_pause_resume_watch_idle_and_scope_to_owner()
-> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let project_dir = temp.path().join("workspace");
    std::fs::create_dir_all(&project_dir)?;
    let server = DaemonServer::bind(DaemonConfig {
        data_dir: temp.path().join("state"),
        port: 0,
    })
    .await?;
    let discovery = server.discovery().clone();
    let registry = server.registry();
    let delivery_token = {
        let mut registry = registry.lock().await;
        registry.store().put_project(&Project {
            id: PROJECT_ID,
            path: project_dir.to_string_lossy().into_owned(),
            name: "workspace".into(),
            display_name: None,
            icon: None,
            selected: false,
            sort_order: 0,
        })?;
        registry.store().put_agent_tool(&AgentTool {
            id: 90,
            name: "MCP Timer Claude".into(),
            command: "scripted-timer-claude".into(),
            tool_type: "claude_code".into(),
            enabled: true,
            source: AgentToolSource::Local,
        })?;
        registry.create(process(
            DELIVERY_ID,
            "orchestrator",
            "while IFS= read -r line; do printf 'received:[%s]\\n' \"$line\"; done",
            None,
        ))?;
        registry.create(process(
            WORKER_ID,
            "worker",
            "printf '❯\\n'; while IFS= read -r line; do if [ \"$line\" = go ]; then printf 'thinking...\\nesc to interrupt\\n'; sleep 0.7; printf '❯\\n'; fi; done",
            Some(90),
        ))?;
        registry.create(process(STALLED_ID, "stalled", "sleep 30", None))?;
        registry.start(DELIVERY_ID)?;
        registry.start(WORKER_ID)?;
        registry.store().connection().query_row(
            "SELECT token FROM process_mcp_tokens WHERE process_id = ?1",
            [DELIVERY_ID],
            |row| row.get::<_, String>(0),
        )?
    };
    wait_for_state(&registry, WORKER_ID, AttentionState::Idle).await?;

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let server_task = tokio::spawn(server.serve_until(async move {
        let _ = shutdown_rx.await;
    }));
    let endpoint = format!("http://127.0.0.1:{}/mcp", discovery.port);
    let owner_headers = HashMap::from([(
        HeaderName::from_static(GBUILD_MCP_TOKEN_HEADER),
        HeaderValue::from_str(&delivery_token)?,
    )]);
    let owner_transport = StreamableHttpClientTransport::from_config(
        StreamableHttpClientTransportConfig::with_uri(endpoint.clone())
            .custom_headers(owner_headers),
    );
    let owner = ClientInfo::default().serve(owner_transport).await?;

    let delayed = call(
        &owner,
        "timer_set",
        json!({ "delay_ms": 40, "body": "literal $(wake) [one]" }),
    )
    .await;
    assert_eq!(delayed["timer"]["delivery_process_id"], DELIVERY_ID);
    wait_for_output(&registry, DELIVERY_ID, "received:[literal $(wake) [one]]").await?;

    let paused = call(
        &owner,
        "timer_set",
        json!({ "delay_ms": 180, "body": "pause-resume wake" }),
    )
    .await;
    let paused_id = paused["timer"]["id"].as_i64().unwrap();
    call(&owner, "timer_pause", json!({ "timer_id": paused_id })).await;
    tokio::time::sleep(Duration::from_millis(230)).await;
    assert!(!output_contains(&registry, DELIVERY_ID, "pause-resume wake").await?);
    call(&owner, "timer_resume", json!({ "timer_id": paused_id })).await;
    wait_for_output(&registry, DELIVERY_ID, "received:[pause-resume wake]").await?;

    let cancelled = call(
        &owner,
        "timer_set",
        json!({ "delay_ms": 100, "body": "cancelled wake" }),
    )
    .await;
    let cancelled_id = cancelled["timer"]["id"].as_i64().unwrap();
    call(&owner, "timer_cancel", json!({ "timer_id": cancelled_id })).await;
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert!(!output_contains(&registry, DELIVERY_ID, "cancelled wake").await?);

    let already = call(
        &owner,
        "timer_fire_when_idle_all",
        json!({
            "processes": ["worker"],
            "max_wait_ms": 2_000,
            "body": "already idle",
        }),
    )
    .await;
    assert_eq!(already["already_satisfied"], true);
    assert_eq!(already["delivered_immediately"], true);
    assert_eq!(already["delivery_process_id"], DELIVERY_ID);
    assert_eq!(already["timer"], Value::Null);
    wait_for_output(&registry, DELIVERY_ID, "received:[already idle]").await?;

    let any = call(
        &owner,
        "timer_fire_when_idle_any",
        json!({
            "processes": [{ "process_name": "worker" }],
            "max_wait_ms": 3_000,
            "body": "fresh idle wake",
        }),
    )
    .await;
    assert_eq!(any["already_satisfied"], false);
    assert_eq!(any["delivered_immediately"], false);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!output_contains(&registry, DELIVERY_ID, "fresh idle wake").await?);
    call(
        &owner,
        "send_input",
        json!({ "process_id": WORKER_ID, "input": "go" }),
    )
    .await;
    wait_for_state(&registry, WORKER_ID, AttentionState::Working).await?;
    wait_for_state(&registry, WORKER_ID, AttentionState::Idle).await?;
    wait_for_output(&registry, DELIVERY_ID, "received:[fresh idle wake]").await?;

    call(
        &owner,
        "timer_fire_when_idle_any",
        json!({
            "processes": [STALLED_ID],
            "max_wait_ms": 60,
            "body": "timeout wake",
        }),
    )
    .await;
    wait_for_output(&registry, DELIVERY_ID, "received:[timeout wake]").await?;

    let owner_timers = call(&owner, "timer_list", json!({ "limit": 100 })).await;
    assert!(owner_timers["timers"].as_array().unwrap().len() >= 4);

    let other_transport = StreamableHttpClientTransport::from_config(
        StreamableHttpClientTransportConfig::with_uri(endpoint)
            .auth_header(discovery.token.clone()),
    );
    let other = ClientInfo::default().serve(other_transport).await?;
    let other_timers = call(
        &other,
        "timer_list",
        json!({ "project_id": PROJECT_ID, "limit": 100 }),
    )
    .await;
    assert!(other_timers["timers"].as_array().unwrap().is_empty());

    let _ = other.cancel().await;
    let _ = owner.cancel().await;
    let _ = shutdown_tx.send(());
    server_task.await??;
    Ok(())
}
