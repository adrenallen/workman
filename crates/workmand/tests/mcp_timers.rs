// Drives Unix fixtures (POSIX shell one-liners on real PTYs, XDG data layouts);
// Windows fixture parity is tracked as follow-up work.
#![cfg(unix)]

use std::{
    collections::{BTreeMap, HashMap},
    error::Error,
    time::{Duration, Instant},
};

use axum::http::{HeaderName, HeaderValue};
use rmcp::{
    ServiceExt,
    model::{CallToolRequestParams, CallToolResult, ClientInfo},
    transport::{
        StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use serde_json::{Map, Value, json};
use workman_core::{
    AgentTool, AgentToolSource, Process, ProcessKind, ProcessSource, ProcessStatus, Project,
    attention::AttentionState,
};
use workmand::{DaemonConfig, DaemonServer, WORKMAN_MCP_TOKEN_HEADER};

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

async fn invoke(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ClientInfo>,
    name: &'static str,
    arguments_value: Value,
) -> CallToolResult {
    client
        .call_tool(CallToolRequestParams::new(name).with_arguments(arguments(arguments_value)))
        .await
        .unwrap_or_else(|error| panic!("{name} failed: {error}"))
}

async fn wait_for_output(
    registry: &workmand::SharedProcessRegistry,
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
    registry: &workmand::SharedProcessRegistry,
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
    registry: &workmand::SharedProcessRegistry,
    process_id: i64,
    expected: AttentionState,
) -> Result<(), Box<dyn Error>> {
    let deadline = Instant::now() + Duration::from_secs(7);
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
            resume_args: None,
            continue_args: None,
        })?;
        registry.create(process(
            DELIVERY_ID,
            "orchestrator",
            "while IFS= read -r line; do printf 'received:[%s]\\n' \"$line\"; done",
            None,
        ))?;
        let mut worker = process(
            WORKER_ID,
            "worker",
            "printf '❯\\n'; while IFS= read -r line; do if [ \"$line\" = go ]; then printf 'thinking...\\nesc to interrupt\\n'; sleep 0.7; printf '❯\\n'; fi; done",
            Some(90),
        );
        worker.spawned_by_process_id = Some(DELIVERY_ID);
        registry.create(worker)?;
        registry.create(process(STALLED_ID, "stalled", "sleep 30", None))?;
        registry.store().set_process_mcp_token(
            STALLED_ID,
            "stalled-process-token",
            1_700_000_000_000,
        )?;
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
        HeaderName::from_static(WORKMAN_MCP_TOKEN_HEADER),
        HeaderValue::from_str(&delivery_token)?,
    )]);
    let owner_transport = StreamableHttpClientTransport::from_config(
        StreamableHttpClientTransportConfig::with_uri(endpoint.clone())
            .custom_headers(owner_headers.clone()),
    );
    let owner = ClientInfo::default().serve(owner_transport).await?;
    let rotated_owner_transport = StreamableHttpClientTransport::from_config(
        StreamableHttpClientTransportConfig::with_uri(endpoint.clone())
            .custom_headers(owner_headers),
    );
    let rotated_owner = ClientInfo::default().serve(rotated_owner_transport).await?;

    let advertised_tools = owner.list_all_tools().await?;
    let timer_set_tool = advertised_tools
        .iter()
        .find(|tool| tool.name == "timer_set")
        .expect("timer_set tool is present");
    let timer_set_description = timer_set_tool.description.as_deref().unwrap_or_default();
    assert!(timer_set_description.contains("one-shot or repeating"));
    assert!(timer_set_description.contains("delivers to you"));
    assert!(timer_set_description.contains("do not poll while waiting"));
    assert!(
        timer_set_tool.input_schema["properties"]["body"]["description"]
            .as_str()
            .is_some_and(|description| {
                description.contains("submitted unmodified")
                    && description.contains("Keep it on one line")
            })
    );
    for name in ["timer_fire_when_idle_any", "timer_fire_when_idle_all"] {
        let tool = advertised_tools
            .iter()
            .find(|tool| tool.name == name)
            .unwrap_or_else(|| panic!("{name} tool is present"));
        let description = tool.description.as_deref().unwrap_or_default();
        assert!(description.contains("no-poll wake-up"));
        assert!(description.contains("end the current turn"));
        assert!(description.contains("fresh user turn"));
        assert!(description.contains("only when this timer delivers back to you"));
        assert!(description.contains("do not poll while waiting"));
        if name == "timer_fire_when_idle_any" {
            assert!(description.contains("fresh non-idle-to-idle transition"));
        } else {
            assert!(description.contains("idle at arm time or later reaches idle"));
        }
        assert!(
            tool.input_schema["properties"]["body"]["description"]
                .as_str()
                .is_some_and(|description| {
                    description.contains("fresh user turn")
                        && description.contains("submitted unmodified")
                        && description.contains("Keep it on one line")
                        && description.contains("instead of polling")
                })
        );
    }

    let delayed = call(
        &owner,
        "timer_set",
        json!({ "delay_ms": 40, "body": "literal $(wake) [one]" }),
    )
    .await;
    assert_eq!(delayed["timer"]["delivery_process_id"], DELIVERY_ID);
    assert_eq!(delayed["timer"]["owner_process_id"], DELIVERY_ID);
    assert_eq!(delayed["timer"]["owner_process_name"], "orchestrator");
    assert_eq!(delayed["timer"]["owner_label"], "orchestrator");
    assert!(delayed["timer"].get("owner_actor").is_none());
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
    assert!(already["next_action"].as_str().is_some_and(|message| {
        message.contains("Do not create another timer")
            && message.contains("end your current turn now")
    }));
    assert_eq!(already["timer"], Value::Null);
    wait_for_output(&registry, DELIVERY_ID, "received:[already idle]").await?;

    let any = call(
        &owner,
        "timer_fire_when_idle_any",
        json!({
            "processes": [{ "process_name": "worker" }],
            "max_wait_ms": 15_000,
            "body": "fresh idle wake",
        }),
    )
    .await;
    assert_eq!(any["already_satisfied"], false);
    assert_eq!(any["delivered_immediately"], false);
    assert!(any["next_action"].as_str().is_some_and(|message| {
        message.contains("end the current turn now")
            && message.contains("no additional wait call is needed")
            && message.contains("inspect the watched processes before assuming they finished")
            && message.contains("waiting on its own timer")
    }));
    let any_timer_id = any["timer"]["id"].as_i64().unwrap();
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
    {
        let registry = registry.lock().await;
        let timer = registry.store().get_timer(any_timer_id)?.unwrap();
        let child_notifications = registry
            .store()
            .list_notifications(None, 100)?
            .into_iter()
            .filter(|notification| notification.process_id == Some(WORKER_ID))
            .count();
        let consumed_markers: i64 = registry.store().connection().query_row(
            "SELECT COUNT(*) FROM consumed_idle_watches
             WHERE process_id = ?1 AND timer_id = ?2",
            (WORKER_ID, any_timer_id),
            |row| row.get(0),
        )?;
        eprintln!(
            "todo451_event_log parent={DELIVERY_ID} child={WORKER_ID} timer={any_timer_id} fired={} delivered_output=true consumed_markers={consumed_markers} child_notifications={child_notifications}",
            timer.fired
        );
        assert!(timer.fired, "the parent's child watch must fire normally");
        assert_eq!(consumed_markers, 1);
        assert_eq!(
            child_notifications, 0,
            "watched child completion must not reach the shared notification source"
        );
    }

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

    let reconnect_timer = call(
        &owner,
        "timer_set",
        json!({ "delay_ms": 10_000, "body": "cancel after reconnect" }),
    )
    .await;
    let reconnect_timer_id = reconnect_timer["timer"]["id"].as_i64().unwrap();
    registry.lock().await.store().connection().execute(
        "UPDATE processes SET name = 'orchestrator-renamed' WHERE id = ?1",
        [DELIVERY_ID],
    )?;
    let rotated_timers = call(
        &rotated_owner,
        "timer_list",
        json!({ "project_id": PROJECT_ID, "limit": 100 }),
    )
    .await;
    assert!(
        rotated_timers["timers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|timer| {
                timer["id"] == reconnect_timer_id
                    && timer["owner_process_id"] == DELIVERY_ID
                    && timer["owner_process_name"] == "orchestrator-renamed"
            })
    );

    let other_transport = StreamableHttpClientTransport::from_config(
        StreamableHttpClientTransportConfig::with_uri(endpoint)
            .auth_header("stalled-process-token".to_owned()),
    );
    let other = ClientInfo::default().serve(other_transport).await?;
    let other_timers = call(
        &other,
        "timer_list",
        json!({ "project_id": PROJECT_ID, "limit": 100 }),
    )
    .await;
    assert!(
        other_timers["timers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|timer| {
                timer["id"] == reconnect_timer_id && timer["owner_process_id"] == DELIVERY_ID
            })
    );
    let denied = invoke(
        &other,
        "timer_cancel",
        json!({ "project_id": PROJECT_ID, "timer_id": reconnect_timer_id }),
    )
    .await;
    assert_eq!(denied.is_error, Some(true));
    assert_eq!(
        denied.structured_content.unwrap()["code"],
        "timer_not_found"
    );
    call(
        &rotated_owner,
        "timer_cancel",
        json!({ "project_id": PROJECT_ID, "timer_id": reconnect_timer_id }),
    )
    .await;
    assert!(
        registry
            .lock()
            .await
            .store()
            .get_timer(reconnect_timer_id)?
            .is_none()
    );

    let _ = other.cancel().await;
    let _ = rotated_owner.cancel().await;
    let _ = owner.cancel().await;
    let _ = shutdown_tx.send(());
    server_task.await??;
    Ok(())
}
