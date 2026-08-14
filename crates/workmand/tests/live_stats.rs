// Drives Unix fixtures (POSIX shell one-liners on real PTYs, XDG data layouts);
// Windows fixture parity is tracked as follow-up work.
#![cfg(unix)]

use std::{collections::BTreeMap, error::Error, path::PathBuf, time::Duration};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::{
    sync::oneshot,
    task::JoinHandle,
    time::{Instant, timeout},
};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
    tungstenite::{Message, client::IntoClientRequest, http::header},
};
use workman_core::{
    Process, ProcessKind, ProcessSource, ProcessStatus, Project, ScratchpadService,
};
use workmand::{DaemonConfig, DaemonServer, Discovery, SharedProcessRegistry};

type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

const TEST_PROGRESS_TIMEOUT: Duration = Duration::from_secs(15);
const TEST_POLL_INTERVAL: Duration = Duration::from_millis(10);

struct TestServer {
    discovery: Discovery,
    registry: SharedProcessRegistry,
    shutdown: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<std::io::Result<()>>>,
    _temp: TempDir,
    _project_path: PathBuf,
}

impl TestServer {
    async fn start() -> Result<Self, Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let project_path = temp.path().join("project");
        let process_ready = temp.path().join("process-ready");
        std::fs::create_dir(&project_path)?;
        let server = DaemonServer::bind(DaemonConfig {
            data_dir: temp.path().join("isolated-state"),
            port: 0,
        })
        .await?;
        let discovery = server.discovery().clone();
        let registry = server.registry();
        {
            let mut registry = registry.lock().await;
            registry.store().put_project(&Project {
                id: 1,
                path: project_path.to_string_lossy().into_owned(),
                name: "live-stats".into(),
                display_name: None,
                icon: None,
                selected: true,
                sort_order: 0,
            })?;
            ScratchpadService::new(registry.store()).write(
                1,
                None,
                "Live notes".into(),
                "# Live notes".into(),
                None,
                None,
            )?;
            registry.create(Process {
                id: 101,
                project_id: 1,
                kind: ProcessKind::Terminal,
                name: "shell".into(),
                command: Some(
                    "printf ready > \"$LIVE_STATS_READY_FILE\"; read line; sleep 30 & wait".into(),
                ),
                working_dir: project_path.to_string_lossy().into_owned(),
                env: BTreeMap::from([(
                    "LIVE_STATS_READY_FILE".into(),
                    process_ready.to_string_lossy().into_owned(),
                )]),
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
            registry.start(101)?;
            registry.create(Process {
                id: 102,
                project_id: 1,
                kind: ProcessKind::Terminal,
                name: "interactive-shell".into(),
                command: Some("/bin/sh".into()),
                working_dir: project_path.to_string_lossy().into_owned(),
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
                sort_order: 1,
            })?;
            registry.start(102)?;
        }
        timeout(TEST_PROGRESS_TIMEOUT, async {
            while !process_ready.exists() {
                tokio::time::sleep(TEST_POLL_INTERVAL).await;
            }
        })
        .await
        .map_err(|_| "timed out waiting for the live-stats fixture process to become ready")?;

        let (shutdown, receive_shutdown) = oneshot::channel();
        let task = tokio::spawn(server.serve_until(async move {
            let _ = receive_shutdown.await;
        }));
        Ok(Self {
            discovery,
            registry,
            shutdown: Some(shutdown),
            task: Some(task),
            _temp: temp,
            _project_path: project_path,
        })
    }

    fn request(&self) -> tokio_tungstenite::tungstenite::http::Request<()> {
        let mut request = format!("ws://127.0.0.1:{}/ws", self.discovery.port)
            .into_client_request()
            .unwrap();
        request.headers_mut().insert(
            header::AUTHORIZATION,
            format!("Bearer {}", self.discovery.token).parse().unwrap(),
        );
        request
    }

    async fn stop(mut self) -> Result<(), Box<dyn Error>> {
        {
            let mut registry = self.registry.lock().await;
            let _ = registry.stop(101);
            let _ = registry.stop(102);
        }
        let _ = self.shutdown.take().unwrap().send(());
        self.task.take().unwrap().await??;
        Ok(())
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        if let Some(task) = self.task.take() {
            task.abort();
        }
        if let Ok(mut registry) = self.registry.try_lock() {
            let _ = registry.kill(101);
            let _ = registry.kill(102);
        }
    }
}

#[tokio::test]
async fn status_stream_rolls_up_memory_counts_and_new_descendants() -> Result<(), Box<dyn Error>> {
    let server = TestServer::start().await?;
    let (mut socket, _) = connect_async(server.request()).await?;

    rpc(
        &mut socket,
        "subscribe",
        "process.status_subscribe",
        json!({}),
    )
    .await?;
    let initial = next_stats(&mut socket, "initial process sample", 0, |stats| {
        stats["projects"]["1"]["memory_bytes"]
            .as_u64()
            .is_some_and(|bytes| bytes > 0)
            && stats["processes"]["101"]["descendant_count"] == 0
            && stats["processes"]["102"]["foreground_active"] == false
    })
    .await?;
    assert_eq!(initial["counts"]["1"]["todo_open"], 0);
    assert_eq!(initial["counts"]["1"]["scratchpad_total"], 1);
    assert_eq!(initial["counts"]["1"]["terminal_running"], 2);
    assert_eq!(initial["counts"]["1"]["terminal_total"], 2);
    assert_eq!(
        initial["processes"]["102"]["foreground_process_group"], initial["processes"]["102"]["pid"],
        "idle interactive shell should own its foreground"
    );
    let initial_sample = initial["sampled_at"].as_u64().unwrap();

    rpc(
        &mut socket,
        "create-todo",
        "coordination.todo_create",
        json!({
            "project_id": 1,
            "title": "Count me live",
            "body": "",
            "priority": "medium",
            "tags": []
        }),
    )
    .await?;
    let after_todo = next_stats(&mut socket, "todo count update", initial_sample, |stats| {
        stats["counts"]["1"]["todo_open"] == 1
    })
    .await?;

    rpc(
        &mut socket,
        "spawn-child",
        "process.send_input",
        json!({
            "process_id": 101,
            "data": BASE64.encode("continue"),
            "submit": true
        }),
    )
    .await?;
    let after_child = next_stats(
        &mut socket,
        "descendant process sample",
        after_todo["sampled_at"].as_u64().unwrap(),
        |stats| {
            stats["processes"]["101"]["descendant_count"]
                .as_u64()
                .is_some_and(|count| count > 0)
        },
    )
    .await?;
    assert!(
        after_child["processes"]["101"]["descendants"][0]["memory_bytes"]
            .as_u64()
            .is_some_and(|bytes| bytes > 0)
    );

    rpc(
        &mut socket,
        "run-foreground-job",
        "process.send_input",
        json!({
            "process_id": 102,
            "data": BASE64.encode("sleep 30"),
            "submit": true
        }),
    )
    .await?;
    let foreground = next_stats(
        &mut socket,
        "interactive terminal foreground job",
        after_child["sampled_at"].as_u64().unwrap(),
        |stats| stats["processes"]["102"]["foreground_active"] == true,
    )
    .await?;
    assert_ne!(
        foreground["processes"]["102"]["foreground_process_group"],
        foreground["processes"]["102"]["pid"]
    );

    rpc(
        &mut socket,
        "interrupt-foreground-job",
        "process.send_input",
        json!({
            "process_id": 102,
            "data": BASE64.encode([0x03]),
            "submit": false
        }),
    )
    .await?;
    let idle_again = next_stats(
        &mut socket,
        "interactive terminal idle after foreground job",
        foreground["sampled_at"].as_u64().unwrap(),
        |stats| stats["processes"]["102"]["foreground_active"] == false,
    )
    .await?;
    assert_eq!(
        idle_again["processes"]["102"]["foreground_process_group"],
        idle_again["processes"]["102"]["pid"]
    );

    socket.close(None).await?;
    server.stop().await?;
    Ok(())
}

async fn rpc(
    socket: &mut Socket,
    id: &str,
    method: &str,
    params: Value,
) -> Result<Value, Box<dyn Error>> {
    socket
        .send(Message::Text(
            json!({ "id": id, "method": method, "params": params })
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
        if response["id"] == id {
            if response["ok"] != true {
                return Err(format!("RPC failed: {response}").into());
            }
            return Ok(response["result"].clone());
        }
    }
}

async fn next_stats(
    socket: &mut Socket,
    stage: &str,
    after_sample: u64,
    predicate: impl Fn(&Value) -> bool,
) -> Result<Value, Box<dyn Error>> {
    let deadline = Instant::now() + TEST_PROGRESS_TIMEOUT;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(
                format!("timed out waiting for {stage} after sample {after_sample}").into(),
            );
        }
        let message = tokio::time::timeout(remaining, socket.next())
            .await
            .map_err(|_| format!("timed out waiting for {stage} after sample {after_sample}"))?
            .ok_or("websocket closed")??;
        let Message::Text(message) = message else {
            continue;
        };
        let event: Value = serde_json::from_str(&message)?;
        if event["event"] != "process.statuses" {
            continue;
        }
        let stats = &event["stats"];
        if stats["sampled_at"]
            .as_u64()
            .is_some_and(|sampled_at| sampled_at > after_sample)
            && predicate(stats)
        {
            return Ok(stats.clone());
        }
    }
}
