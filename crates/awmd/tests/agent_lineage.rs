use std::{
    collections::HashMap, error::Error, os::unix::fs::PermissionsExt, path::Path, time::Duration,
};

use awm_core::{AgentTool, AgentToolSource, Project};
use awmd::{AWM_MCP_TOKEN_HEADER, DaemonConfig, DaemonServer};
use axum::http::{HeaderName, HeaderValue};
use rmcp::{
    ServiceExt,
    model::{CallToolRequestParams, ClientInfo},
    transport::{
        StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use serde_json::{Map, Value, json};

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
    Err("fake agent did not publish its awm process context".into())
}

#[tokio::test]
async fn agent_spawns_record_lineage_and_parent_close_promotes_children()
-> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let project_dir = temp.path().join("workspace");
    std::fs::create_dir_all(&project_dir)?;
    let fake_agent = temp.path().join("fake-agent.sh");
    std::fs::write(
        &fake_agent,
        "#!/bin/sh\n\
         printf '%s\\n%s\\n' \"$AWM_PROCESS_ID\" \"$AWM_MCP_TOKEN\" > \"$1\"\n\
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
        })?;
    }

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let server_task = tokio::spawn(server.serve_until(async move {
        let _ = shutdown_rx.await;
    }));
    let endpoint = format!("http://127.0.0.1:{}/mcp", discovery.port);
    let root_transport = StreamableHttpClientTransport::from_config(
        StreamableHttpClientTransportConfig::with_uri(endpoint.clone())
            .auth_header(discovery.token.clone()),
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
        HeaderName::from_static(AWM_MCP_TOKEN_HEADER),
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

    let child_context = temp.path().join("child-context.txt");
    let child_spawn = call(
        &parent,
        "spawn_agent",
        json!({
            "agent_tool_id": 99,
            "name": "child-agent",
            "extra_args": [child_context],
        }),
    )
    .await;
    let child_id = child_spawn["process_id"].as_i64().unwrap();

    let terminal_spawn = call(
        &parent,
        "spawn_process",
        json!({ "kind": "terminal", "name": "child-terminal" }),
    )
    .await;
    let terminal_id = terminal_spawn["process_id"].as_i64().unwrap();

    let listed = call(&parent, "list_processes", json!({})).await;
    let processes = listed["processes"].as_array().expect("process envelope");
    let parent_view = processes
        .iter()
        .find(|view| view["id"] == parent_id)
        .unwrap();
    let child_view = processes
        .iter()
        .find(|view| view["id"] == child_id)
        .unwrap();
    let terminal_view = processes
        .iter()
        .find(|view| view["id"] == terminal_id)
        .unwrap();
    assert_eq!(parent_view["spawned_by_process_id"], Value::Null);
    assert_eq!(child_view["spawned_by_process_id"], parent_id);
    assert_eq!(terminal_view["spawned_by_process_id"], parent_id);

    let child_status = call(
        &parent,
        "get_process_status",
        json!({ "process_id": child_id }),
    )
    .await;
    assert_eq!(child_status["spawned_by_process_id"], parent_id);

    assert_eq!(
        call(
            &root,
            "close_process",
            json!({ "project_id": 7, "process_id": parent_id }),
        )
        .await["closed"],
        true
    );
    let after_close = call(&root, "list_processes", json!({ "project_id": 7 })).await;
    let child_after_close = after_close["processes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|view| view["id"] == child_id)
        .unwrap();
    assert_eq!(child_after_close["spawned_by_process_id"], Value::Null);

    for process_id in [child_id, terminal_id] {
        let closed = call(
            &root,
            "close_process",
            json!({ "project_id": 7, "process_id": process_id }),
        )
        .await;
        assert_eq!(closed["closed"], true);
    }

    let _ = parent.cancel().await;
    let _ = root.cancel().await;
    let _ = shutdown_tx.send(());
    server_task.await??;
    assert!(registry.lock().await.list(Some(7))?.is_empty());
    Ok(())
}
