#![cfg(unix)]

use std::{
    collections::{BTreeMap, HashMap},
    error::Error,
    path::PathBuf,
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
use tempfile::TempDir;
use tokio::{sync::oneshot, task::JoinHandle};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
    tungstenite::{Message, client::IntoClientRequest, http::header},
};
use workman_core::{
    AgentTool, AgentToolSource, Process, ProcessKind, ProcessSource, ProcessStatus, Project,
    attention::AttentionState,
};
use workmand::{
    DaemonConfig, DaemonServer, Discovery, SharedProcessRegistry, WORKMAN_MCP_TOKEN_HEADER,
};

const PROJECT_ID: i64 = 1;
const DELIVERY_ID: i64 = 10;
const WORKER_ID: i64 = 11;
const STALLED_ID: i64 = 12;

type McpClient = rmcp::service::RunningService<rmcp::RoleClient, ClientInfo>;
type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

struct TestServer {
    discovery: Discovery,
    registry: SharedProcessRegistry,
    owner_token: String,
    shutdown: Option<oneshot::Sender<()>>,
    task: JoinHandle<std::io::Result<()>>,
    _temp: TempDir,
    _project_path: PathBuf,
}

impl TestServer {
    async fn start() -> Result<Self, Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let project_path = temp.path().join("project");
        std::fs::create_dir(&project_path)?;
        let server = DaemonServer::bind(DaemonConfig {
            data_dir: temp.path().join("isolated-state"),
            port: 0,
        })
        .await?;
        let discovery = server.discovery().clone();
        let registry = server.registry();
        let owner_token = {
            let mut registry = registry.lock().await;
            registry.store().put_project(&Project {
                id: PROJECT_ID,
                path: project_path.to_string_lossy().into_owned(),
                name: "timer-events".into(),
                display_name: None,
                icon: None,
                selected: true,
                sort_order: 0,
            })?;
            registry.store().put_agent_tool(&AgentTool {
                id: 90,
                name: "Timer test agent".into(),
                command: "timer-test-agent".into(),
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
            registry.start(DELIVERY_ID)?;
            registry.start(WORKER_ID)?;
            registry.store().connection().query_row(
                "SELECT token FROM process_mcp_tokens WHERE process_id = ?1",
                [DELIVERY_ID],
                |row| row.get(0),
            )?
        };

        let (shutdown, receive_shutdown) = oneshot::channel();
        let task = tokio::spawn(server.serve_until(async move {
            let _ = receive_shutdown.await;
        }));
        Ok(Self {
            discovery,
            registry,
            owner_token,
            shutdown: Some(shutdown),
            task,
            _temp: temp,
            _project_path: project_path,
        })
    }

    fn ws_request(&self) -> tokio_tungstenite::tungstenite::http::Request<()> {
        let mut request = format!("ws://127.0.0.1:{}/ws", self.discovery.port)
            .into_client_request()
            .unwrap();
        request.headers_mut().insert(
            header::AUTHORIZATION,
            format!("Bearer {}", self.discovery.token).parse().unwrap(),
        );
        request
    }

    async fn mcp_client(&self) -> Result<McpClient, Box<dyn Error>> {
        let headers = HashMap::from([(
            HeaderName::from_static(WORKMAN_MCP_TOKEN_HEADER),
            HeaderValue::from_str(&self.owner_token)?,
        )]);
        let transport = StreamableHttpClientTransport::from_config(
            StreamableHttpClientTransportConfig::with_uri(format!(
                "http://127.0.0.1:{}/mcp",
                self.discovery.port
            ))
            .custom_headers(headers),
        );
        Ok(ClientInfo::default().serve(transport).await?)
    }

    async fn stop(mut self) -> Result<(), Box<dyn Error>> {
        {
            let mut registry = self.registry.lock().await;
            let _ = registry.stop(DELIVERY_ID);
            let _ = registry.stop(WORKER_ID);
        }
        let _ = self.shutdown.take().unwrap().send(());
        self.task.await??;
        Ok(())
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

fn arguments(value: Value) -> Map<String, Value> {
    value.as_object().unwrap().clone()
}

async fn call(client: &McpClient, name: &'static str, args: Value) -> Value {
    let result = client
        .call_tool(CallToolRequestParams::new(name).with_arguments(arguments(args)))
        .await
        .unwrap_or_else(|error| panic!("{name} failed: {error}"));
    assert_ne!(result.is_error, Some(true), "{name} returned {result:?}");
    let structured = result.structured_content.as_ref().unwrap();
    assert!(structured.is_object(), "{name} returned a non-object root");
    let text = result
        .content
        .iter()
        .find_map(|content| content.as_text())
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&text.text).unwrap(),
        *structured
    );
    structured.clone()
}

async fn subscribe(socket: &mut Socket) -> Result<(), Box<dyn Error>> {
    socket
        .send(Message::Text(
            json!({ "id": "subscribe", "method": "process.status_subscribe", "params": {} })
                .to_string()
                .into(),
        ))
        .await?;
    loop {
        let message = socket.next().await.ok_or("websocket closed")??;
        let Message::Text(message) = message else {
            continue;
        };
        let response: Value = serde_json::from_str(&message)?;
        if response["id"] == "subscribe" {
            assert_eq!(response["ok"], true);
            return Ok(());
        }
    }
}

async fn next_timer_status(
    socket: &mut Socket,
    predicate: impl Fn(&Value) -> bool,
) -> Result<Value, Box<dyn Error>> {
    let deadline = Instant::now() + Duration::from_secs(6);
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("timed out waiting for timer status event".into());
        }
        let message = tokio::time::timeout(remaining, socket.next())
            .await?
            .ok_or("websocket closed")??;
        let Message::Text(message) = message else {
            continue;
        };
        let event: Value = serde_json::from_str(&message)?;
        if event["event"] == "process.statuses" && predicate(&event) {
            return Ok(event);
        }
    }
}

fn has_timer(event: &Value, timer_id: i64) -> bool {
    event["timers"]
        .as_array()
        .is_some_and(|timers| timers.iter().any(|timer| timer["id"] == timer_id))
}

fn process_status(event: &Value, process_id: i64) -> Option<&Value> {
    event["processes"]
        .as_array()?
        .iter()
        .find(|process| process["id"] == process_id)
}

fn lifecycle(event: &Value, kind: &str, timer_id: Option<i64>, reason: Option<&str>) -> bool {
    event["timer_events"].as_array().is_some_and(|events| {
        events.iter().any(|candidate| {
            candidate["kind"] == kind
                && timer_id.is_none_or(|timer_id| candidate["timer_id"] == timer_id)
                && reason.is_none_or(|reason| candidate["reason"] == reason)
        })
    })
}

async fn wait_for_state(
    registry: &SharedProcessRegistry,
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
            return Err(format!("process {process_id} did not reach {expected:?}").into());
        }
        tokio::time::sleep(Duration::from_millis(15)).await;
    }
}

#[tokio::test]
async fn parent_waiting_on_working_child_reaches_every_status_consumer()
-> Result<(), Box<dyn Error>> {
    let server = TestServer::start().await?;
    wait_for_state(&server.registry, DELIVERY_ID, AttentionState::Idle).await?;
    wait_for_state(&server.registry, WORKER_ID, AttentionState::Idle).await?;
    let (mut socket, _) = connect_async(server.ws_request()).await?;
    subscribe(&mut socket).await?;
    let client = server.mcp_client().await?;

    call(
        &client,
        "send_input",
        json!({ "process_id": WORKER_ID, "input": "go" }),
    )
    .await;
    wait_for_state(&server.registry, WORKER_ID, AttentionState::Working).await?;

    let idle = call(
        &client,
        "timer_fire_when_idle_any",
        json!({
            "processes": [WORKER_ID],
            "max_wait_ms": 12_000,
            "body": "child finished",
        }),
    )
    .await;
    let timer_id = idle["timer"]["id"].as_i64().unwrap();
    let waiting = next_timer_status(&mut socket, |event| {
        has_timer(event, timer_id)
            && process_status(event, DELIVERY_ID)
                .is_some_and(|process| process["agent_state"]["state"] == "waiting")
            && process_status(event, WORKER_ID)
                .is_some_and(|process| process["agent_state"]["state"] == "working")
    })
    .await?;

    let parent = process_status(&waiting, DELIVERY_ID).unwrap();
    let child = process_status(&waiting, WORKER_ID).unwrap();
    assert_eq!(parent["agent_state"]["waiting"], true);
    assert_eq!(parent["agent_state"]["waiting_on"][0]["timer_id"], timer_id);
    assert_eq!(
        parent["agent_state"]["waiting_on"][0]["max_wait_ms"],
        12_000
    );
    assert_eq!(
        parent["agent_state"]["waiting_on"][0]["watch_processes"][0]["process_name"],
        "worker"
    );
    assert_eq!(child["agent_state"]["watched"], true);
    assert_ne!(child["agent_state"]["state"], "waiting");

    tokio::time::sleep(Duration::from_millis(1_500)).await;
    {
        let mut registry = server.registry.lock().await;
        assert_eq!(
            registry.get_status(WORKER_ID)?.agent_state.state,
            AttentionState::Working,
            "the child's transient prompt must remain debounced"
        );
        assert!(
            registry
                .store()
                .get_timer(timer_id)?
                .is_some_and(|timer| !timer.fired),
            "the parent's timer must remain pending during the transient prompt"
        );
    }

    wait_for_state(&server.registry, WORKER_ID, AttentionState::Idle).await?;
    let delivered = next_timer_status(&mut socket, |event| {
        !has_timer(event, timer_id)
            && lifecycle(event, "fired", Some(timer_id), Some("idle_transition"))
            && process_status(event, DELIVERY_ID).is_some_and(|process| {
                process["agent_state"]["state"] != "waiting"
                    && process["agent_state"]["waiting_on"]
                        .as_array()
                        .is_some_and(Vec::is_empty)
            })
    })
    .await?;
    assert_eq!(
        process_status(&delivered, WORKER_ID).unwrap()["agent_state"]["state"],
        "idle"
    );

    client.cancel().await?;
    server.stop().await?;
    Ok(())
}

#[tokio::test]
async fn timer_status_stream_reconciles_every_lifecycle_path() -> Result<(), Box<dyn Error>> {
    let server = TestServer::start().await?;
    wait_for_state(&server.registry, WORKER_ID, AttentionState::Idle).await?;
    let (mut socket, _) = connect_async(server.ws_request()).await?;
    subscribe(&mut socket).await?;
    let client = server.mcp_client().await?;

    let delayed = call(
        &client,
        "timer_set",
        json!({ "delay_ms": 1_200, "body": "one shot" }),
    )
    .await;
    let delayed_id = delayed["timer"]["id"].as_i64().unwrap();
    next_timer_status(&mut socket, |event| {
        has_timer(event, delayed_id) && lifecycle(event, "created", Some(delayed_id), None)
    })
    .await?;
    let one_shot_started = Instant::now();
    next_timer_status(&mut socket, |event| {
        !has_timer(event, delayed_id)
            && lifecycle(event, "fired", Some(delayed_id), Some("delay"))
            && lifecycle(event, "delivered", Some(delayed_id), Some("delay"))
    })
    .await?;
    assert!(one_shot_started.elapsed() < Duration::from_secs(2));

    let idle = call(
        &client,
        "timer_fire_when_idle_any",
        json!({
            "processes": [WORKER_ID],
            "max_wait_ms": 12_000,
            "body": "early idle",
        }),
    )
    .await;
    let idle_id = idle["timer"]["id"].as_i64().unwrap();
    let idle_deadline = idle["timer"]["due_at"].as_i64().unwrap();
    next_timer_status(&mut socket, |event| {
        has_timer(event, idle_id) && lifecycle(event, "created", Some(idle_id), None)
    })
    .await?;
    call(
        &client,
        "send_input",
        json!({ "process_id": WORKER_ID, "input": "go" }),
    )
    .await;
    wait_for_state(&server.registry, WORKER_ID, AttentionState::Working).await?;
    wait_for_state(&server.registry, WORKER_ID, AttentionState::Idle).await?;
    let early_idle = next_timer_status(&mut socket, |event| {
        !has_timer(event, idle_id)
            && lifecycle(event, "fired", Some(idle_id), Some("idle_transition"))
    })
    .await?;
    let fired_at = early_idle["timer_events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|event| event["timer_id"] == idle_id && event["kind"] == "fired")
        .unwrap()["at"]
        .as_i64()
        .unwrap();
    assert!(
        fired_at < idle_deadline,
        "idle timer did not fire before max_wait"
    );

    let max_wait = call(
        &client,
        "timer_fire_when_idle_any",
        json!({
            "processes": [STALLED_ID],
            "max_wait_ms": 200,
            "body": "max wait",
        }),
    )
    .await;
    let max_wait_id = max_wait["timer"]["id"].as_i64().unwrap();
    next_timer_status(&mut socket, |event| {
        !has_timer(event, max_wait_id)
            && lifecycle(event, "fired", Some(max_wait_id), Some("max_wait"))
    })
    .await?;

    let repeating = call(
        &client,
        "timer_set",
        json!({ "delay_ms": 900, "repeat_every_ms": 900, "body": "repeat" }),
    )
    .await;
    let repeating_id = repeating["timer"]["id"].as_i64().unwrap();
    let first_due = repeating["timer"]["due_at"].as_i64().unwrap();
    next_timer_status(&mut socket, |event| {
        has_timer(event, repeating_id) && lifecycle(event, "created", Some(repeating_id), None)
    })
    .await?;
    let rearmed = next_timer_status(&mut socket, |event| {
        lifecycle(event, "delivered", Some(repeating_id), Some("delay"))
            && event["timers"].as_array().is_some_and(|timers| {
                timers.iter().any(|timer| {
                    timer["id"] == repeating_id
                        && timer["repeating"] == true
                        && timer["fired"] == false
                        && timer["due_at"].as_i64().is_some_and(|due| due > first_due)
                })
            })
    })
    .await?;
    assert!(has_timer(&rearmed, repeating_id));
    call(&client, "timer_cancel", json!({ "timer_id": repeating_id })).await;
    next_timer_status(&mut socket, |event| {
        !has_timer(event, repeating_id) && lifecycle(event, "cancelled", Some(repeating_id), None)
    })
    .await?;

    let controlled = call(
        &client,
        "timer_set",
        json!({ "delay_ms": 4_000, "body": "controlled" }),
    )
    .await;
    let controlled_id = controlled["timer"]["id"].as_i64().unwrap();
    call(&client, "timer_pause", json!({ "timer_id": controlled_id })).await;
    next_timer_status(&mut socket, |event| {
        lifecycle(event, "paused", Some(controlled_id), None)
            && event["timers"].as_array().is_some_and(|timers| {
                timers
                    .iter()
                    .any(|timer| timer["id"] == controlled_id && timer["paused"] == true)
            })
    })
    .await?;
    call(
        &client,
        "timer_resume",
        json!({ "timer_id": controlled_id }),
    )
    .await;
    next_timer_status(&mut socket, |event| {
        lifecycle(event, "resumed", Some(controlled_id), None)
            && event["timers"].as_array().is_some_and(|timers| {
                timers
                    .iter()
                    .any(|timer| timer["id"] == controlled_id && timer["paused"] == false)
            })
    })
    .await?;
    call(
        &client,
        "timer_cancel",
        json!({ "timer_id": controlled_id }),
    )
    .await;

    let immediate = call(
        &client,
        "timer_fire_when_idle_all",
        json!({
            "processes": [WORKER_ID],
            "max_wait_ms": 5_000,
            "body": "already idle",
        }),
    )
    .await;
    assert_eq!(immediate["delivered_immediately"], true);
    next_timer_status(&mut socket, |event| {
        event["timer_events"].as_array().is_some_and(|events| {
            let immediate = |kind: &str| {
                events.iter().any(|event| {
                    event["timer_id"].is_null()
                        && event["delivery_process_id"] == DELIVERY_ID
                        && event["reason"] == "already_satisfied"
                        && event["kind"] == kind
                })
            };
            immediate("fired") && immediate("delivered")
        })
    })
    .await?;

    let _ = client.cancel().await;
    socket.close(None).await?;
    server.stop().await?;
    Ok(())
}
