#![cfg(unix)]

use std::{
    collections::{BTreeMap, HashMap},
    error::Error,
    path::Path,
    process::Command,
    time::Duration,
};

use futures_util::{SinkExt, StreamExt};
use rmcp::{
    ServiceExt,
    model::{CallToolRequestParams, ClientInfo},
    transport::{
        StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use serde_json::{Map, Value, json};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{Message, client::IntoClientRequest},
};
use workman_core::{Process, ProcessKind, ProcessSource, ProcessStatus, Project};
use workmand::{DaemonConfig, DaemonServer};

fn arguments(value: Value) -> Map<String, Value> {
    value.as_object().unwrap().clone()
}

async fn call(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ClientInfo>,
    name: &'static str,
    args: Value,
) -> Value {
    let result = client
        .call_tool(CallToolRequestParams::new(name).with_arguments(arguments(args)))
        .await
        .unwrap_or_else(|error| panic!("{name} failed: {error}"));
    assert_ne!(result.is_error, Some(true), "{name}: {result:?}");
    result.structured_content.expect("structured tool result")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mcp_agent_sees_only_its_worktree_and_ws_exposes_the_full_repository()
-> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let main = temp.path().join("rpc-repo");
    let external = temp.path().join("external");
    git(temp.path(), &["init", "-b", "main", main.to_str().unwrap()])?;
    git(&main, &["config", "user.email", "fixture@example.test"])?;
    git(&main, &["config", "user.name", "Fixture"])?;
    std::fs::write(main.join("README.md"), "fixture\n")?;
    git(&main, &["add", "."])?;
    git(&main, &["commit", "-m", "initial"])?;
    git(
        &main,
        &[
            "worktree",
            "add",
            "-b",
            "external",
            external.to_str().unwrap(),
            "main",
        ],
    )?;

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
            id: 1,
            path: std::fs::canonicalize(&main)?.to_string_lossy().into_owned(),
            name: "rpc-repo".into(),
            display_name: None,
            icon: None,
            selected: true,
            sort_order: 0,
        })?;
        registry.store().put_process(&Process {
            id: 1,
            project_id: 1,
            kind: ProcessKind::Agent,
            name: "worktree-agent".into(),
            command: Some("true".into()),
            working_dir: std::fs::canonicalize(&main)?.to_string_lossy().into_owned(),
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
        workmand::worktrees::reconcile_existing_projects(registry.store())?;
    }
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let server_task = tokio::spawn(server.serve_until(async move {
        let _ = shutdown_rx.await;
    }));

    let endpoint = format!("http://127.0.0.1:{}/mcp", discovery.port);
    let transport = StreamableHttpClientTransport::from_config(
        StreamableHttpClientTransportConfig::with_uri(endpoint)
            .auth_header(discovery.token.clone()),
    );
    let client = ClientInfo::default().serve(transport).await?;
    call(&client, "identify_session", json!({ "process_id": 1 })).await;
    let tools = client.list_all_tools().await?;
    let remove_tool = tools
        .iter()
        .find(|tool| tool.name == "worktree_remove")
        .expect("worktree_remove tool is present");
    assert!(
        remove_tool.input_schema["properties"]
            .get("delete_from_disk")
            .is_some(),
        "worktree_remove advertises the explicit disk-deletion flag"
    );
    assert_eq!(
        remove_tool.input_schema["properties"]["delete_from_disk"]["default"],
        false
    );
    let names = tools
        .into_iter()
        .map(|tool| tool.name.into_owned())
        .collect::<Vec<_>>();
    for expected in [
        "worktree_list",
        "worktree_create",
        "worktree_fork",
        "worktree_env_forget",
        "worktree_health",
        "worktree_adopt",
        "worktree_remove",
    ] {
        assert!(
            names.iter().any(|name| name == expected),
            "missing {expected}"
        );
    }

    let listed = call(&client, "worktree_list", json!({ "project_id": 1 })).await;
    assert_eq!(listed["repository"]["name"], "rpc-repo");
    assert_eq!(listed["worktrees"].as_array().unwrap().len(), 1);
    assert_eq!(listed["worktrees"][0]["project_id"], 1);
    assert!(listed["repository"]["herd"]["available"].is_boolean());
    assert!(listed["pull_requests"]["available"].is_boolean());
    assert!(listed["pull_requests"]["checked_at"].is_number());

    let health = call(&client, "worktree_health", json!({})).await;
    assert!(health["all_required_ready"].is_boolean());
    assert_eq!(health["checks"].as_array().unwrap().len(), 4);
    assert!(health["checks"].as_array().unwrap().iter().all(|check| {
        check["detail"].is_string() && check["fix_hint"].is_string() || check["fix_hint"].is_null()
    }));

    for (tool, args) in [
        (
            "worktree_adopt",
            json!({ "path": external, "preferences": { "env_policy": "link" } }),
        ),
        (
            "worktree_create",
            json!({ "project_id": 1, "branch": "rpc-created", "from_ref": "main" }),
        ),
        (
            "worktree_fork",
            json!({ "project_id": 1, "branch": "rpc-forked" }),
        ),
    ] {
        let rejected = client
            .call_tool(CallToolRequestParams::new(tool).with_arguments(arguments(args)))
            .await?;
        assert_eq!(rejected.is_error, Some(true));
        assert_eq!(
            rejected.structured_content.unwrap()["code"],
            "project_scope_error"
        );
    }

    // The WebSocket control plane exposes canonical dotted names and returns
    // the same project-parent/branch-enriched model.
    let mut request = format!("ws://127.0.0.1:{}/ws", discovery.port).into_client_request()?;
    request.headers_mut().insert(
        axum::http::header::AUTHORIZATION,
        format!("Bearer {}", discovery.token).parse()?,
    );
    let (mut socket, _) = connect_async(request).await?;
    socket
        .send(Message::Text(
            json!({ "id": 7, "method": "worktree.list", "params": { "project_id": 1 } })
                .to_string()
                .into(),
        ))
        .await?;
    let response = tokio::time::timeout(Duration::from_secs(10), socket.next())
        .await?
        .ok_or("socket closed")??;
    let Message::Text(response) = response else {
        return Err("expected text response".into());
    };
    let response: Value = serde_json::from_str(&response)?;
    assert_eq!(response["ok"], true, "{response}");
    assert_eq!(response["result"]["repository"]["name"], "rpc-repo");
    assert!(
        response["result"]["worktrees"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| { row["branch"] == "external" && row["parent_project_id"] == 1 })
    );
    socket.close(None).await?;

    let _ = client.cancel().await;
    let _ = shutdown_tx.send(());
    server_task.await??;
    Ok(())
}

fn git(directory: &Path, args: &[&str]) -> Result<(), Box<dyn Error>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(directory)
        .args(args)
        .envs(HashMap::from([("GIT_TERMINAL_PROMPT", "0")]))
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "git -C {} {} failed: {}",
            directory.display(),
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(())
}
