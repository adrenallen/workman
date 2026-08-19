#![cfg(unix)]

use std::{error::Error, path::Path, process::Command, time::Duration};

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::{sync::oneshot, task::JoinHandle, time::Instant};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
    tungstenite::{Message, client::IntoClientRequest, http::header},
};
use workman_core::Project;
use workmand::{DaemonConfig, DaemonServer, Discovery, SharedProcessRegistry, worktrees};

type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

struct TestServer {
    discovery: Discovery,
    shutdown: Option<oneshot::Sender<()>>,
    task: JoinHandle<std::io::Result<()>>,
    registry: SharedProcessRegistry,
    _temp: TempDir,
    managed: std::path::PathBuf,
}

impl TestServer {
    async fn start() -> Result<Self, Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let main = temp.path().join("project");
        let origin = temp.path().join("origin.git");
        let managed = temp.path().join("managed");
        git(temp.path(), &["init", "--bare", origin.to_str().unwrap()])?;
        git(temp.path(), &["init", "-b", "main", main.to_str().unwrap()])?;
        git(&main, &["config", "user.email", "fixture@example.test"])?;
        git(&main, &["config", "user.name", "Fixture"])?;
        std::fs::write(main.join("README.md"), "fixture\n")?;
        git(&main, &["add", "README.md"])?;
        git(&main, &["commit", "-m", "initial"])?;
        git(
            &main,
            &["remote", "add", "origin", origin.to_str().unwrap()],
        )?;
        git(&main, &["push", "-u", "origin", "main"])?;

        let server = DaemonServer::bind(DaemonConfig {
            data_dir: temp.path().join("isolated-state"),
            port: 0,
        })
        .await?;
        let discovery = server.discovery().clone();
        let registry = server.registry();
        {
            let registry = registry.lock().await;
            registry.store().put_project(&Project {
                id: 1,
                path: workman_core::canonical_path(&main)?
                    .to_string_lossy()
                    .into_owned(),
                name: "fixture".into(),
                display_name: None,
                icon: None,
                selected: true,
                sort_order: 0,
            })?;
            worktrees::reconcile_existing_projects(registry.store())?;
        }

        let (shutdown, receive_shutdown) = oneshot::channel();
        let task = tokio::spawn(server.serve_until(async move {
            let _ = receive_shutdown.await;
        }));
        Ok(Self {
            discovery,
            shutdown: Some(shutdown),
            task,
            registry,
            _temp: temp,
            managed,
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
        let _ = self.shutdown.take().unwrap().send(());
        self.task.await??;
        Ok(())
    }

    async fn put_project(&self, project: &Project) -> Result<(), Box<dyn Error>> {
        self.registry.lock().await.store().put_project(project)?;
        Ok(())
    }
}

#[tokio::test]
async fn async_worktree_rpc_streams_success_and_bad_branch_failure() -> Result<(), Box<dyn Error>> {
    let server = TestServer::start().await?;
    let (mut socket, _) = connect_async(server.request()).await?;
    let validated = rpc(
        &mut socket,
        "validate-ref",
        "worktree.ref_validate",
        json!({ "project_id": 1, "ref": "HEAD" }),
    )
    .await?;
    assert_eq!(validated["requested_ref"], "HEAD");
    assert_eq!(validated["resolved_ref"], "HEAD");
    assert!(
        validated["commit"]
            .as_str()
            .is_some_and(|value| value.len() == 40)
    );
    rpc(
        &mut socket,
        "subscribe",
        "process.status_subscribe",
        json!({}),
    )
    .await?;

    let started = Instant::now();
    let ack = rpc(
        &mut socket,
        "create",
        "worktree.create_async",
        json!({
            "operation_id": "fixture-create",
            "project_id": 1,
            "branch": "feature/optimistic",
            "from_ref": "main",
            "managed_root": server.managed.to_string_lossy(),
            "env_policy": "skip",
            "preferences": { "herd_enabled": "no" }
        }),
    )
    .await?;
    assert_eq!(ack["operation_id"], "fixture-create");
    assert_eq!(ack["accepted"], true);
    assert!(started.elapsed() < Duration::from_secs(1));

    let completed = next_operation(&mut socket, "fixture-create", |operation| {
        operation["status"] == "completed"
    })
    .await?;
    assert!(completed["project"]["id"].as_i64().is_some());
    assert!(completed["steps"].as_array().is_some_and(|steps| {
        steps
            .iter()
            .all(|step| matches!(step["status"].as_str(), Some("completed" | "skipped")))
    }));
    assert!(server.managed.join("feature-optimistic").is_dir());
    let created_project_id = completed["project"]["id"]
        .as_i64()
        .expect("created project id");

    let failed_ack = rpc(
        &mut socket,
        "bad-branch",
        "worktree.create_async",
        json!({
            "operation_id": "fixture-failure",
            "project_id": 1,
            "branch": "bad..branch",
            "managed_root": server.managed.to_string_lossy(),
            "env_policy": "skip",
            "preferences": { "herd_enabled": "no" }
        }),
    )
    .await?;
    assert_eq!(failed_ack["accepted"], true);
    let failed = next_operation(&mut socket, "fixture-failure", |operation| {
        operation["status"] == "failed"
    })
    .await?;
    assert_eq!(failed["steps"][0]["status"], "failed");
    assert!(
        failed["error"]
            .as_str()
            .is_some_and(|error| !error.is_empty())
    );
    let dismissed = rpc(
        &mut socket,
        "dismiss-failure",
        "worktree.operation_dismiss",
        json!({ "operation_id": "fixture-failure" }),
    )
    .await?;
    assert_eq!(dismissed["operation_id"], "fixture-failure");
    assert_eq!(dismissed["dismissed"], true);
    next_snapshot_without(&mut socket, "fixture-failure").await?;

    let remove_started = Instant::now();
    let remove_ack = rpc(
        &mut socket,
        "remove",
        "worktree.remove_async",
        json!({
            "operation_id": "fixture-remove",
            "project_id": created_project_id,
            "confirm_remove": true,
            "confirm_stop_running": true,
            "delete_from_disk": false,
            "force_dirty": false
        }),
    )
    .await?;
    assert_eq!(remove_ack["accepted"], true);
    assert!(remove_started.elapsed() < Duration::from_secs(1));
    let removed = next_operation(&mut socket, "fixture-remove", |operation| {
        operation["status"] == "completed"
    })
    .await?;
    assert_eq!(removed["mode"], "remove");
    assert_eq!(removed["removal"]["project_id"], created_project_id);
    assert_eq!(removed["removal"]["files_untouched"], true);
    assert_eq!(removed["removal"]["registration_issue"], Value::Null);
    assert!(removed["steps"].as_array().is_some_and(|steps| {
        steps
            .iter()
            .all(|step| matches!(step["status"].as_str(), Some("completed" | "skipped")))
    }));
    let projects = rpc(&mut socket, "projects", "projects.list", json!({})).await?;
    assert!(projects.as_array().is_some_and(|projects| {
        projects
            .iter()
            .all(|project| project["id"] != created_project_id)
    }));
    let remove_dismissed = rpc(
        &mut socket,
        "dismiss-remove",
        "worktree.operation_dismiss",
        json!({ "operation_id": "fixture-remove" }),
    )
    .await?;
    assert_eq!(remove_dismissed["dismissed"], true);
    next_snapshot_without(&mut socket, "fixture-remove").await?;

    let missing = server.managed.join("missing-registration");
    server
        .put_project(&Project {
            id: 99,
            path: missing.to_string_lossy().into_owned(),
            name: "missing-registration".into(),
            display_name: None,
            icon: None,
            selected: false,
            sort_order: 99,
        })
        .await?;
    rpc(
        &mut socket,
        "remove-missing",
        "worktree.remove_async",
        json!({
            "operation_id": "fixture-remove-missing",
            "project_id": 99,
            "confirm_remove": true,
            "confirm_stop_running": true,
            "delete_from_disk": true,
            "force_dirty": true
        }),
    )
    .await?;
    let removed_missing = next_operation(&mut socket, "fixture-remove-missing", |operation| {
        operation["status"] == "completed"
    })
    .await?;
    assert_eq!(removed_missing["removal"]["files_untouched"], true);
    assert!(
        removed_missing["removal"]["registration_issue"]
            .as_str()
            .is_some_and(|issue| issue.contains("project path is missing"))
    );

    rpc(
        &mut socket,
        "remove-invalid",
        "worktree.remove_async",
        json!({
            "operation_id": "fixture-remove-failure",
            "project_id": 404,
            "confirm_remove": true
        }),
    )
    .await?;
    let remove_failure = next_operation(&mut socket, "fixture-remove-failure", |operation| {
        operation["status"] == "failed"
    })
    .await?;
    assert_eq!(remove_failure["error_code"], "project_not_found");
    assert!(
        remove_failure["error"]
            .as_str()
            .is_some_and(|error| error.contains("project 404 was not found"))
    );

    socket.close(None).await?;
    server.stop().await?;
    Ok(())
}

async fn next_snapshot_without(
    socket: &mut Socket,
    operation_id: &str,
) -> Result<(), Box<dyn Error>> {
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(format!("timed out waiting for dismissal of {operation_id}").into());
        }
        let message = tokio::time::timeout(remaining, socket.next())
            .await?
            .ok_or("websocket closed")??;
        let Message::Text(message) = message else {
            continue;
        };
        let event: Value = serde_json::from_str(&message)?;
        if event["event"] != "process.statuses" {
            continue;
        }
        let operations = event["worktree_operations"]
            .as_array()
            .ok_or("worktree operation snapshot missing")?;
        if operations
            .iter()
            .all(|operation| operation["id"] != operation_id)
        {
            return Ok(());
        }
    }
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
        if response["id"] != id {
            continue;
        }
        if response["ok"] != true {
            return Err(format!("RPC failed: {response}").into());
        }
        return Ok(response["result"].clone());
    }
}

async fn next_operation(
    socket: &mut Socket,
    operation_id: &str,
    predicate: impl Fn(&Value) -> bool,
) -> Result<Value, Box<dyn Error>> {
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(format!("timed out waiting for {operation_id}").into());
        }
        let message = tokio::time::timeout(remaining, socket.next())
            .await?
            .ok_or("websocket closed")??;
        let Message::Text(message) = message else {
            continue;
        };
        let event: Value = serde_json::from_str(&message)?;
        if event["event"] != "process.statuses" {
            continue;
        }
        let Some(operation) = event["worktree_operations"]
            .as_array()
            .and_then(|operations| {
                operations
                    .iter()
                    .find(|operation| operation["id"] == operation_id)
            })
        else {
            continue;
        };
        if predicate(operation) {
            return Ok(operation.clone());
        }
    }
}

fn git(cwd: &Path, args: &[&str]) -> Result<String, Box<dyn Error>> {
    let output = Command::new("git").args(args).current_dir(cwd).output()?;
    if !output.status.success() {
        return Err(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(String::from_utf8(output.stdout)?)
}
