use std::{
    collections::HashMap,
    error::Error,
    os::unix::fs::PermissionsExt,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
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
use tokio::time::Instant;
use workman_core::{AgentTool, AgentToolSource, Project, attention::AttentionState};
use workmand::{DaemonConfig, DaemonServer, WORKMAN_MCP_TOKEN_HEADER};

type Client = rmcp::service::RunningService<rmcp::RoleClient, ClientInfo>;

struct HostLoad {
    keep_running: Arc<AtomicBool>,
    threads: Vec<std::thread::JoinHandle<()>>,
}

impl HostLoad {
    fn start(thread_count: usize) -> Self {
        let keep_running = Arc::new(AtomicBool::new(true));
        let threads = (0..thread_count)
            .map(|_| {
                let keep_running = Arc::clone(&keep_running);
                std::thread::spawn(move || {
                    while keep_running.load(Ordering::Relaxed) {
                        std::hint::spin_loop();
                    }
                })
            })
            .collect();
        Self {
            keep_running,
            threads,
        }
    }
}

impl Drop for HostLoad {
    fn drop(&mut self) {
        self.keep_running.store(false, Ordering::Relaxed);
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}

fn arguments(value: Value) -> Map<String, Value> {
    value.as_object().expect("arguments are an object").clone()
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

fn write_codex_trust_tui(path: &Path) -> Result<(), Box<dyn Error>> {
    std::fs::write(
        path,
        r##"#!/usr/bin/perl
use strict;
use warnings;
$| = 1;
system('stty', 'raw', '-echo');
print "\e[?1049h\e[HYou are in /tmp/new-workspace\r\n";
print "Do you trust the contents of this directory? Working with untrusted contents poses security risks.\r\n";
print "\x{203a} 1. Yes, continue\r\n  2. No, quit\r\nPress enter to continue";
my $ack = '';
my $read = sysread(STDIN, $ack, 1);
if (!defined($read) || $read != 1 || $ack ne "\r") {
    print "\r\nBAD_TRUST_INPUT\r\n";
    sleep 30;
    exit 2;
}
print "\e[2J\e[HAUTO_ACKNOWLEDGED\r\n\x{276f} ";
my $mission = '';
while (1) {
    my $chunk = '';
    my $count = sysread(STDIN, $chunk, 4096);
    exit 3 unless defined($count) && $count > 0;
    for my $character (split //, $chunk) {
        if ($character eq "\r") {
            print "\r\nRECEIVED:$mission\r\n";
            sleep 30;
            exit 0;
        }
        $mission .= $character;
    }
}
"##,
    )?;
    let mut permissions = std::fs::metadata(path)?.permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(path, permissions)?;
    Ok(())
}

async fn wait_for_dialog(
    registry: &workmand::SharedProcessRegistry,
    process_id: i64,
) -> Result<(), Box<dyn Error>> {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if registry.lock().await.pending_dialog(process_id)?.is_some() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err("process did not render a recognized dialog".into());
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

async fn wait_for_idle(
    registry: &workmand::SharedProcessRegistry,
    process_id: i64,
    timeout: Duration,
) -> Result<(), Box<dyn Error>> {
    let deadline = Instant::now() + timeout;
    loop {
        let status = registry.lock().await.get_status(process_id)?;
        if status.agent_state.state == AttentionState::Idle {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "process {process_id} did not become idle; current state is {:?}",
                status.agent_state.state
            )
            .into());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn wait_for_actual_response(
    registry: &workmand::SharedProcessRegistry,
    process_id: i64,
    expected: &str,
    timeout: Duration,
) -> Result<(), Box<dyn Error>> {
    let deadline = Instant::now() + timeout;
    loop {
        let (status, rendered) = {
            let mut registry = registry.lock().await;
            (
                registry.get_status(process_id)?,
                registry.rendered_output(process_id)?.text,
            )
        };
        if rendered.contains(expected) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "Codex left the submitted text as a draft instead of answering {expected:?}; state={:?}, output={rendered:?}",
                status.agent_state.state
            )
            .into());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test]
async fn spawn_auto_acknowledges_trust_before_immediate_mission_and_guard_blocks_when_disabled()
-> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let project_dir = temp.path().join("brand-new-workspace");
    std::fs::create_dir(&project_dir)?;
    let fake_codex = temp.path().join("fake-codex");
    write_codex_trust_tui(&fake_codex)?;

    let server = DaemonServer::bind(DaemonConfig {
        data_dir: temp.path().join("state"),
        port: 0,
    })
    .await?;
    let discovery = server.discovery().clone();
    let registry = server.registry();
    {
        let registry = registry.lock().await;
        registry.store().put_project(&Project {
            id: 7,
            path: project_dir.to_string_lossy().into_owned(),
            name: "new-workspace".into(),
            display_name: None,
            icon: None,
            selected: true,
            sort_order: 0,
        })?;
        registry.store().put_agent_tool(&AgentTool {
            id: 99,
            name: "Codex trust fixture".into(),
            command: fake_codex.to_string_lossy().into_owned(),
            tool_type: "codex".into(),
            enabled: true,
            source: AgentToolSource::Local,
        })?;
    }

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let server_task = tokio::spawn(server.serve_until(async move {
        let _ = shutdown_rx.await;
    }));
    let transport = StreamableHttpClientTransport::from_config(
        StreamableHttpClientTransportConfig::with_uri(format!(
            "http://127.0.0.1:{}/mcp",
            discovery.port
        ))
        .auth_header(discovery.token),
    );
    let client = ClientInfo::default().serve(transport).await?;

    let spawned = call(
        &client,
        "spawn_agent",
        json!({
            "project_id": 7,
            "agent_tool_id": 99,
            "name": "auto-acked-codex"
        }),
    )
    .await;
    let process_id = spawned["process_id"].as_i64().unwrap();
    let preamble = spawned["agent_instructions"].as_str().unwrap();
    assert!(preamble.contains("shared notes, plans, briefs, and hand-offs"));
    assert!(preamble.contains("read it back with scratchpad_read or todo_get"));
    let status = call(
        &client,
        "get_process_status",
        json!({ "project_id": 7, "process_id": process_id }),
    )
    .await;
    assert!(status["events"].as_array().unwrap().iter().any(|event| {
        event["kind"] == "dialog_auto_acknowledged"
            && event["message"]
                .as_str()
                .is_some_and(|message| message.contains("auto-acknowledged"))
    }));

    let mission = "Mission step 2 must arrive intact, including this 2.";
    call(
        &client,
        "send_input",
        json!({
            "process_id": process_id,
            "project_id": 7,
            "input": mission,
            "submit": true
        }),
    )
    .await;
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let output = registry.lock().await.rendered_output(process_id)?;
        if output.text.contains(&format!("RECEIVED:{mission}")) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "mission was not delivered intact after automatic trust acknowledgment: {}",
            output.text
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    let guarded = call(
        &client,
        "spawn_agent",
        json!({
            "project_id": 7,
            "agent_tool_id": 99,
            "name": "guarded-codex",
            "auto_acknowledge_dialogs": false
        }),
    )
    .await;
    let guarded_id = guarded["process_id"].as_i64().unwrap();
    wait_for_dialog(&registry, guarded_id).await?;
    let rejected = call_result(
        &client,
        "send_input",
        json!({
            "process_id": guarded_id,
            "project_id": 7,
            "input": mission,
            "submit": true
        }),
    )
    .await;
    assert_eq!(rejected.is_error, Some(true));
    let error = rejected.structured_content.unwrap();
    assert_eq!(error["code"], "dialog_pending");
    assert_eq!(error["classification"], "permission_dialog");
    assert!(error["dialog"].as_str().unwrap().contains("2. No, quit"));

    // Raw bytes intentionally bypass the text guard, allowing a caller to
    // respond to the dialog explicitly.
    call(
        &client,
        "send_input",
        json!({ "project_id": 7, "process_id": guarded_id, "bytes": [13] }),
    )
    .await;
    call(
        &client,
        "close_process",
        json!({ "project_id": 7, "process_id": process_id }),
    )
    .await;
    call(
        &client,
        "close_process",
        json!({ "project_id": 7, "process_id": guarded_id }),
    )
    .await;

    let _ = client.cancel().await;
    let _ = shutdown_tx.send(());
    server_task.await??;
    Ok(())
}

/// Manual acceptance against the installed Codex CLI. It is ignored in ordinary
/// CI because it requires a local Codex installation and credentials, but it
/// exercises the same brand-new-directory race reported by the user.
#[tokio::test]
#[ignore = "requires an installed Codex CLI"]
async fn installed_codex_timer_delivery_submits_short_and_loaded_large_missions()
-> Result<(), Box<dyn Error>> {
    if std::process::Command::new("codex")
        .arg("--version")
        .output()
        .is_err()
    {
        return Ok(());
    }

    let temp = tempfile::tempdir()?;
    let project_dir = temp.path().join("never-before-trusted");
    std::fs::create_dir(&project_dir)?;
    let server = DaemonServer::bind(DaemonConfig {
        data_dir: temp.path().join("state"),
        port: 0,
    })
    .await?;
    let discovery = server.discovery().clone();
    let registry = server.registry();
    {
        let registry = registry.lock().await;
        registry.store().put_project(&Project {
            id: 8,
            path: project_dir.to_string_lossy().into_owned(),
            name: "codex-new-dir".into(),
            display_name: None,
            icon: None,
            selected: true,
            sort_order: 0,
        })?;
        registry.store().put_agent_tool(&AgentTool {
            id: 100,
            name: "Installed Codex".into(),
            command: "codex --dangerously-bypass-approvals-and-sandbox --no-alt-screen".into(),
            tool_type: "codex".into(),
            enabled: true,
            source: AgentToolSource::Local,
        })?;
    }

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let server_task = tokio::spawn(server.serve_until(async move {
        let _ = shutdown_rx.await;
    }));
    let transport = StreamableHttpClientTransport::from_config(
        StreamableHttpClientTransportConfig::with_uri(format!(
            "http://127.0.0.1:{}/mcp",
            discovery.port
        ))
        .auth_header(discovery.token),
    );
    let client = ClientInfo::default().serve(transport).await?;
    let spawned = call(
        &client,
        "spawn_agent",
        json!({ "project_id": 8, "agent_tool_id": 100 }),
    )
    .await;
    let process_id = spawned["process_id"].as_i64().unwrap();
    let preamble = spawned["agent_instructions"].as_str().unwrap();
    assert!(preamble.contains("shared notes, plans, briefs, and hand-offs"));
    assert!(preamble.contains("read it back with scratchpad_read or todo_get"));
    let status = call(
        &client,
        "get_process_status",
        json!({ "project_id": 8, "process_id": process_id }),
    )
    .await;
    assert!(
        status["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|event| { event["kind"] == "dialog_auto_acknowledged" })
    );

    wait_for_idle(&registry, process_id, Duration::from_secs(10)).await?;
    let process_token = {
        let registry = registry.lock().await;
        registry.store().connection().query_row(
            "SELECT token FROM process_mcp_tokens WHERE process_id = ?1",
            [process_id],
            |row| row.get::<_, String>(0),
        )?
    };
    let timer_transport = StreamableHttpClientTransport::from_config(
        StreamableHttpClientTransportConfig::with_uri(format!(
            "http://127.0.0.1:{}/mcp",
            discovery.port
        ))
        .custom_headers(HashMap::from([(
            HeaderName::from_static(WORKMAN_MCP_TOKEN_HEADER),
            HeaderValue::from_str(&process_token)?,
        )])),
    );
    let timer_client = ClientInfo::default().serve(timer_transport).await?;

    // Scheduled delivery, short enough to reproduce the atomic-write failure.
    call(
        &timer_client,
        "timer_set",
        json!({
            "delay_ms": 20,
            "body": "Decode this ROT13 text and print only the decoded word: SBHE"
        }),
    )
    .await;
    wait_for_actual_response(&registry, process_id, "FOUR", Duration::from_secs(60)).await?;
    wait_for_idle(&registry, process_id, Duration::from_secs(30)).await?;

    // Already-satisfied delivery while the host is busy and the payload is
    // large enough to exercise a different PTY packetization pattern.
    let host_load = HostLoad::start(4);
    let body = format!(
        "Ignore this padding and decode the final ROT13 word, printing only the decoded word. {} RVTUG",
        "padding ".repeat(1_024)
    );
    let immediate = call(
        &timer_client,
        "timer_fire_when_idle_all",
        json!({
            "processes": [process_id],
            "max_wait_ms": 30_000,
            "body": body
        }),
    )
    .await;
    drop(host_load);
    assert_eq!(immediate["already_satisfied"], true);
    assert_eq!(immediate["delivered_immediately"], true);
    wait_for_actual_response(&registry, process_id, "EIGHT", Duration::from_secs(60)).await?;
    assert_eq!(status["status"], "running");

    call(
        &client,
        "close_process",
        json!({ "project_id": 8, "process_id": process_id }),
    )
    .await;
    let _ = timer_client.cancel().await;
    let _ = client.cancel().await;
    let _ = shutdown_tx.send(());
    server_task.await??;
    Ok(())
}
