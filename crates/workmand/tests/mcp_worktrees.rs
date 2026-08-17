#![cfg(unix)]

use std::{
    collections::{BTreeMap, HashMap},
    error::Error,
    os::unix::fs::PermissionsExt,
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

async fn call_failure(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ClientInfo>,
    name: &'static str,
    args: Value,
) -> Value {
    let result = client
        .call_tool(CallToolRequestParams::new(name).with_arguments(arguments(args)))
        .await
        .unwrap_or_else(|error| panic!("{name} transport failed: {error}"));
    assert_eq!(result.is_error, Some(true), "{name}: {result:?}");
    result.structured_content.expect("structured tool error")
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
        registry
            .store()
            .set_process_mcp_token(1, "worktree-process-token", 1_700_000_000_000)?;
        workmand::worktrees::reconcile_existing_projects(registry.store())?;
    }
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let server_task = tokio::spawn(server.serve_until(async move {
        let _ = shutdown_rx.await;
    }));

    let endpoint = format!("http://127.0.0.1:{}/mcp", discovery.port);
    let transport = StreamableHttpClientTransport::from_config(
        StreamableHttpClientTransportConfig::with_uri(endpoint)
            .auth_header("worktree-process-token".to_owned()),
    );
    let client = ClientInfo::default().serve(transport).await?;
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
    assert!(
        remove_tool.input_schema["properties"]
            .get("confirm_branch")
            .is_none(),
        "force deletion no longer asks agents to type a branch name"
    );
    let delete_project_tool = tools
        .iter()
        .find(|tool| tool.name == "delete_project")
        .expect("delete_project tool is present");
    assert!(
        delete_project_tool.input_schema["properties"]
            .get("delete_from_disk")
            .is_some(),
        "delete_project advertises local disk deletion for every project type"
    );
    assert!(
        delete_project_tool
            .description
            .as_deref()
            .is_some_and(|description| description.contains("never pushes"))
    );
    assert!(
        delete_project_tool.input_schema["properties"]
            .get("confirm_branch")
            .is_none()
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn desktop_cli_control_and_mcp_delete_share_verified_disk_contract()
-> Result<(), Box<dyn Error>> {
    let temp = tempfile::Builder::new()
        .prefix("com.workman.todo126.surfaces.")
        .tempdir_in("/tmp")?;
    let control_folder = temp.path().join("control-project");
    let locked_parent = temp.path().join("locked-parent");
    let failed_mcp_folder = locked_parent.join("failed-mcp-project");
    let successful_mcp_folder = temp.path().join("successful-mcp-project");
    std::fs::create_dir(&control_folder)?;
    std::fs::create_dir(&locked_parent)?;
    std::fs::create_dir(&failed_mcp_folder)?;
    std::fs::create_dir(&successful_mcp_folder)?;
    std::fs::write(control_folder.join("local.txt"), "control\n")?;
    std::fs::write(failed_mcp_folder.join("local.txt"), "retry\n")?;
    std::fs::write(successful_mcp_folder.join("local.txt"), "mcp\n")?;

    let server = DaemonServer::bind(DaemonConfig {
        data_dir: temp.path().join("state"),
        port: 0,
    })
    .await?;
    let discovery = server.discovery().clone();
    let registry = server.registry();
    {
        let registry = registry.lock().await;
        for (id, path, selected) in [
            (1, &control_folder, true),
            (2, &failed_mcp_folder, false),
            (3, &successful_mcp_folder, false),
        ] {
            registry.store().put_project(&Project {
                id,
                path: std::fs::canonicalize(path)?.to_string_lossy().into_owned(),
                name: path.file_name().unwrap().to_string_lossy().into_owned(),
                display_name: None,
                icon: None,
                selected,
                sort_order: id,
            })?;
        }
        for (id, project_id, path) in [(2, 2, &failed_mcp_folder), (3, 3, &successful_mcp_folder)] {
            registry.store().put_process(&Process {
                id,
                project_id,
                kind: ProcessKind::Agent,
                name: format!("delete-agent-{id}"),
                command: Some("true".into()),
                working_dir: std::fs::canonicalize(path)?.to_string_lossy().into_owned(),
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
        }
        registry.store().set_process_mcp_token(
            2,
            "failed-delete-process-token",
            1_700_000_000_000,
        )?;
        registry.store().set_process_mcp_token(
            3,
            "successful-delete-process-token",
            1_700_000_000_000,
        )?;
    }
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let server_task = tokio::spawn(server.serve_until(async move {
        let _ = shutdown_rx.await;
    }));

    // The desktop and `wrk project remove --delete-local` both use this
    // projects.remove control method.
    let mut request = format!("ws://127.0.0.1:{}/ws", discovery.port).into_client_request()?;
    request.headers_mut().insert(
        axum::http::header::AUTHORIZATION,
        format!("Bearer {}", discovery.token).parse()?,
    );
    let (mut socket, _) = connect_async(request).await?;
    socket
        .send(Message::Text(
            json!({
                "id": 126,
                "method": "projects.remove",
                "params": {
                    "project_id": 1,
                    "confirm_remove": true,
                    "confirm_stop_running": true,
                    "delete_from_disk": true,
                    "force_dirty": false
                }
            })
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
    assert_eq!(response["result"]["deleted_from_disk"], true);
    assert_eq!(response["result"]["project_unregistered"], true);
    assert!(!control_folder.exists());
    socket.close(None).await?;

    let endpoint = format!("http://127.0.0.1:{}/mcp", discovery.port);
    let failed_client = ClientInfo::default()
        .serve(StreamableHttpClientTransport::from_config(
            StreamableHttpClientTransportConfig::with_uri(endpoint.clone())
                .auth_header("failed-delete-process-token".to_owned()),
        ))
        .await?;
    std::fs::set_permissions(&locked_parent, std::fs::Permissions::from_mode(0o500))?;
    let failed = call_failure(
        &failed_client,
        "delete_project",
        json!({
            "project_id": 2,
            "confirm_delete": true,
            "confirm_stop_running": true,
            "delete_from_disk": true
        }),
    )
    .await;
    std::fs::set_permissions(&locked_parent, std::fs::Permissions::from_mode(0o700))?;
    assert_eq!(failed["code"], "invalid_worktree_path");
    assert!(
        failed["message"]
            .as_str()
            .is_some_and(|message| message.contains("remains registered"))
    );
    assert!(failed_mcp_folder.exists());
    assert!(
        registry.lock().await.store().get_project(2)?.is_some(),
        "MCP failure must preserve registration for retry"
    );

    let successful_client = ClientInfo::default()
        .serve(StreamableHttpClientTransport::from_config(
            StreamableHttpClientTransportConfig::with_uri(endpoint)
                .auth_header("successful-delete-process-token".to_owned()),
        ))
        .await?;
    let removed = call(
        &successful_client,
        "delete_project",
        json!({
            "project_id": 3,
            "confirm_delete": true,
            "confirm_stop_running": true,
            "delete_from_disk": true
        }),
    )
    .await;
    assert_eq!(removed["deleted_from_disk"], true);
    assert_eq!(removed["project_unregistered"], true);
    assert!(!successful_mcp_folder.exists());

    let _ = failed_client.cancel().await;
    let _ = successful_client.cancel().await;
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
