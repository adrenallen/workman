// Drives Unix fixtures (shebang scripts, permission bits, symlinks); Windows
// fixture parity is tracked as follow-up work.
#![cfg(unix)]

use std::{
    collections::{BTreeMap, HashMap},
    error::Error,
    os::unix::fs::PermissionsExt,
    path::Path,
    time::Duration,
};

use axum::http::{HeaderName, HeaderValue};
use rmcp::{
    ServiceExt,
    model::{CallToolRequestParams, ClientInfo},
    transport::{
        StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use serde_json::{Map, Value, json};
use workman_core::{
    Actor, AgentTool, AgentToolSource, Process, ProcessKind, ProcessSource, ProcessStatus, Project,
    Timer, TimerKind,
};
use workmand::{DaemonConfig, DaemonServer, WORKMAN_MCP_TOKEN_HEADER};

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

async fn wait_for_context(path: &Path) -> Result<(i64, String), Box<dyn Error>> {
    for _ in 0..200 {
        if let Ok(contents) = std::fs::read_to_string(path) {
            let mut lines = contents.lines();
            if let (Some(process_id), Some(token)) = (lines.next(), lines.next())
                && !token.is_empty()
            {
                return Ok((process_id.parse()?, token.to_owned()));
            }
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    Err("fake agent did not publish its workman process context".into())
}

#[tokio::test]
async fn agent_parent_lifecycle_always_cascades_every_registry_descendant()
-> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let project_dir = temp.path().join("workspace");
    std::fs::create_dir_all(&project_dir)?;
    let fake_agent = temp.path().join("fake-agent.sh");
    std::fs::write(
        &fake_agent,
        "#!/bin/sh\n\
         printf '%s\\n%s\\n' \"$WORKMAN_PROCESS_ID\" \"$WORKMAN_MCP_TOKEN\" > \"$1\"\n\
         sleep 30\n",
    )?;
    let mut permissions = std::fs::metadata(&fake_agent)?.permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&fake_agent, permissions)?;

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
            name: "workspace".into(),
            display_name: None,
            icon: None,
            selected: false,
            sort_order: 0,
        })?;
        registry.store().put_agent_tool(&AgentTool {
            id: 99,
            name: "Scripted agent".into(),
            command: fake_agent.to_string_lossy().into_owned(),
            tool_type: "scripted".into(),
            enabled: true,
            source: AgentToolSource::Local,
            resume_args: None,
            continue_args: None,
        })?;
        registry.store().put_process(&Process {
            id: 1,
            project_id: 7,
            kind: ProcessKind::Agent,
            name: "root-agent".into(),
            command: Some("true".into()),
            working_dir: project_dir.to_string_lossy().into_owned(),
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
            .set_process_mcp_token(1, "root-process-token", 1_700_000_000_000)?;
    }

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let server_task = tokio::spawn(server.serve_until(async move {
        let _ = shutdown_rx.await;
    }));
    let endpoint = format!("http://127.0.0.1:{}/mcp", discovery.port);
    let root_transport = StreamableHttpClientTransport::from_config(
        StreamableHttpClientTransportConfig::with_uri(endpoint.clone())
            .auth_header("root-process-token".to_owned()),
    );
    let root = ClientInfo::default().serve(root_transport).await?;

    let parent_context = temp.path().join("parent-context.txt");
    let parent_spawn = call(
        &root,
        "spawn_agent",
        json!({
            "project_id": 7,
            "agent_tool_id": 99,
            "name": "parent-agent",
            "extra_args": [parent_context],
        }),
    )
    .await;
    let parent_id = parent_spawn["process_id"].as_i64().unwrap();
    let (injected_parent_id, parent_token) = wait_for_context(&parent_context).await?;
    assert_eq!(injected_parent_id, parent_id);

    let agent_headers = HashMap::from([(
        HeaderName::from_static(WORKMAN_MCP_TOKEN_HEADER),
        HeaderValue::from_str(&parent_token)?,
    )]);
    let parent_transport = StreamableHttpClientTransport::from_config(
        StreamableHttpClientTransportConfig::with_uri(endpoint.clone())
            .custom_headers(agent_headers),
    );
    let parent = ClientInfo::default().serve(parent_transport).await?;
    assert_eq!(
        call(&parent, "whoami", json!({})).await["process_id"],
        parent_id
    );

    let first_child_context = temp.path().join("first-child-context.txt");
    let first_child_spawn = call(
        &parent,
        "spawn_agent",
        json!({
            "agent_tool_id": 99,
            "name": "first-child",
            "extra_args": [first_child_context],
        }),
    )
    .await;
    let first_child_id = first_child_spawn["process_id"].as_i64().unwrap();
    let (injected_child_id, first_child_token) = wait_for_context(&first_child_context).await?;
    assert_eq!(injected_child_id, first_child_id);

    let first_child_headers = HashMap::from([(
        HeaderName::from_static(WORKMAN_MCP_TOKEN_HEADER),
        HeaderValue::from_str(&first_child_token)?,
    )]);
    let first_child_transport = StreamableHttpClientTransport::from_config(
        StreamableHttpClientTransportConfig::with_uri(endpoint.clone())
            .custom_headers(first_child_headers),
    );
    let first_child = ClientInfo::default().serve(first_child_transport).await?;

    let grandchild_context = temp.path().join("grandchild-context.txt");
    let grandchild_spawn = call(
        &first_child,
        "spawn_agent",
        json!({
            "agent_tool_id": 99,
            "name": "grandchild",
            "extra_args": [grandchild_context],
        }),
    )
    .await;
    let grandchild_id = grandchild_spawn["process_id"].as_i64().unwrap();

    let second_child_context = temp.path().join("second-child-context.txt");
    let second_child_spawn = call(
        &parent,
        "spawn_agent",
        json!({
            "agent_tool_id": 99,
            "name": "second-child",
            "extra_args": [second_child_context],
        }),
    )
    .await;
    let second_child_id = second_child_spawn["process_id"].as_i64().unwrap();

    let terminal_spawn = call(
        &parent,
        "spawn_process",
        json!({ "kind": "terminal", "name": "child-terminal" }),
    )
    .await;
    let terminal_id = terminal_spawn["process_id"].as_i64().unwrap();

    let observer_context = temp.path().join("observer-context.txt");
    let observer_spawn = call(
        &root,
        "spawn_agent",
        json!({
            "project_id": 7,
            "agent_tool_id": 99,
            "name": "independent-observer",
            "extra_args": [observer_context],
        }),
    )
    .await;
    let observer_id = observer_spawn["process_id"].as_i64().unwrap();

    // A surviving observer's watch must remain available to resolve the
    // child's exited edge. Parent-owned/delivered timers are removed below.
    {
        let registry = registry.lock().await;
        registry.store().put_actor(&Actor {
            id: "observer-owner".into(),
            session_id: "observer-owner-session".into(),
            process_id: Some(observer_id),
            selected_project_id: Some(7),
            created_at: 1,
            last_seen_at: 1,
        })?;
        registry.store().put_timer(&Timer {
            id: 900,
            owner_actor: "observer-owner".into(),
            owner_process_id: Some(observer_id),
            delivery_process_id: observer_id,
            body: "child finished".into(),
            kind: TimerKind::IdleAny,
            watch_process_ids: vec![first_child_id],
            interval_ms: None,
            repeating: false,
            max_wait_deadline: Some(i64::MAX),
            paused: false,
            fired: false,
            fired_at: None,
            created_at: 1,
        })?;
    }

    let listed = call(&parent, "list_processes", json!({})).await;
    let processes = listed["processes"].as_array().expect("process envelope");
    let parent_view = processes
        .iter()
        .find(|view| view["id"] == parent_id)
        .unwrap();
    let first_child_view = processes
        .iter()
        .find(|view| view["id"] == first_child_id)
        .unwrap();
    let grandchild_view = processes
        .iter()
        .find(|view| view["id"] == grandchild_id)
        .unwrap();
    let second_child_view = processes
        .iter()
        .find(|view| view["id"] == second_child_id)
        .unwrap();
    let terminal_view = processes
        .iter()
        .find(|view| view["id"] == terminal_id)
        .unwrap();
    assert_eq!(parent_view["spawned_by_process_id"], 1);
    assert_eq!(first_child_view["spawned_by_process_id"], parent_id);
    assert_eq!(second_child_view["spawned_by_process_id"], parent_id);
    assert_eq!(grandchild_view["spawned_by_process_id"], first_child_id);
    assert_eq!(terminal_view["spawned_by_process_id"], parent_id);

    call(
        &parent,
        "timer_set",
        json!({ "delay_ms": 60_000, "body": "must never fire" }),
    )
    .await;

    // An already-exited child remains part of the durable lineage and must be finalized too.
    {
        let mut registry = registry.lock().await;
        registry.stop(second_child_id)?;
        let mut second_child = registry.get(second_child_id)?;
        second_child.status = ProcessStatus::Exited;
        registry.store().put_process(&second_child)?;
    }

    // The legacy false field is deliberately ignored: cascade is the only behavior.
    call(
        &root,
        "stop_process",
        json!({ "project_id": 7, "process_id": parent_id, "cascade": false }),
    )
    .await;
    let after_cascade = call(&root, "list_processes", json!({ "project_id": 7 })).await;
    let after_cascade = after_cascade["processes"].as_array().unwrap();
    for process_id in [
        parent_id,
        first_child_id,
        second_child_id,
        grandchild_id,
        terminal_id,
    ] {
        let process = after_cascade
            .iter()
            .find(|view| view["id"] == process_id)
            .unwrap();
        assert_eq!(
            process["status"], "stopped",
            "process {process_id} was not stopped"
        );
    }
    assert_eq!(
        after_cascade
            .iter()
            .find(|view| view["id"] == terminal_id)
            .unwrap()["spawned_by_process_id"],
        parent_id
    );
    {
        let registry = registry.lock().await;
        let timer_count: i64 =
            registry
                .store()
                .connection()
                .query_row("SELECT COUNT(*) FROM timers", [], |row| row.get(0))?;
        assert_eq!(timer_count, 1, "parent-owned timer survived parent stop");
        let observer_timer = registry.store().get_timer(900)?.unwrap();
        assert_eq!(observer_timer.watch_process_ids, vec![first_child_id]);
    }
    assert!(registry.lock().await.store().get_timer(900)?.is_some());
    assert!(
        registry
            .lock()
            .await
            .store()
            .list_notifications(None, 200)?
            .is_empty(),
        "user-initiated cascade emitted notification spam"
    );

    for process_id in [
        parent_id,
        first_child_id,
        second_child_id,
        grandchild_id,
        terminal_id,
    ] {
        call(
            &root,
            "start_process",
            json!({ "project_id": 7, "process_id": process_id }),
        )
        .await;
    }
    let closed_parent = call(
        &root,
        "close_process",
        json!({ "project_id": 7, "process_id": parent_id, "cascade": false }),
    )
    .await;
    assert_eq!(closed_parent["closed"], true);
    assert_eq!(closed_parent["cascade"], true);
    assert_eq!(
        closed_parent["cascaded_processes"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
    let after_close = call(&root, "list_processes", json!({ "project_id": 7 })).await;
    let after_close = after_close["processes"].as_array().unwrap();
    assert_eq!(after_close.len(), 2);
    assert_eq!(
        after_close
            .iter()
            .find(|view| view["id"] == observer_id)
            .unwrap()["spawned_by_process_id"],
        1
    );

    let closed = call(
        &root,
        "close_process",
        json!({
            "project_id": 7,
            "process_id": 1,
            "confirm_self_close": true
        }),
    )
    .await;
    assert_eq!(closed["closed"], true);
    assert_eq!(closed["cascaded_processes"].as_array().unwrap().len(), 1);

    let _ = first_child.cancel().await;
    let _ = parent.cancel().await;
    let _ = root.cancel().await;
    let _ = shutdown_tx.send(());
    server_task.await??;
    assert!(registry.lock().await.list(Some(7))?.is_empty());
    Ok(())
}
