use std::{collections::BTreeMap, error::Error, time::Duration};

use rmcp::{
    ServiceExt,
    model::{CallToolRequestParams, CallToolResult, ClientInfo},
    transport::{
        StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use serde_json::{Map, Value, json};
use workman_core::{Process, ProcessKind, ProcessSource, ProcessStatus, Project};
use workmand::{DaemonConfig, DaemonServer};

type Client = rmcp::service::RunningService<rmcp::RoleClient, ClientInfo>;

fn arguments(value: Value) -> Map<String, Value> {
    value.as_object().unwrap().clone()
}

async fn invoke(client: &Client, name: &'static str, args: Value) -> CallToolResult {
    client
        .call_tool(CallToolRequestParams::new(name).with_arguments(arguments(args)))
        .await
        .unwrap_or_else(|error| panic!("{name} transport failed: {error}"))
}

async fn call(client: &Client, name: &'static str, args: Value) -> Value {
    let result = invoke(client, name, args).await;
    assert_ne!(result.is_error, Some(true), "{name} returned {result:?}");
    result.structured_content.unwrap()
}

async fn connect(endpoint: String, token: String) -> Result<Client, Box<dyn Error>> {
    let transport = StreamableHttpClientTransport::from_config(
        StreamableHttpClientTransportConfig::with_uri(endpoint).auth_header(token),
    );
    Ok(ClientInfo::default().serve(transport).await?)
}

#[tokio::test]
async fn lock_ownership_survives_actor_rotation_and_rejects_other_processes()
-> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let project_path = temp.path().join("project");
    std::fs::create_dir_all(&project_path)?;
    let server = DaemonServer::bind(DaemonConfig {
        data_dir: temp.path().join("state"),
        port: 0,
    })
    .await?;
    let discovery = server.discovery().clone();
    {
        let registry = server.registry();
        let registry = registry.lock().await;
        let project = Project {
            id: 1,
            path: project_path.to_string_lossy().into_owned(),
            name: "project".into(),
            display_name: None,
            icon: None,
            selected: false,
            sort_order: 0,
        };
        registry.store().put_project(&project)?;
        registry.store().put_process(&Process {
            id: 1,
            project_id: 1,
            kind: ProcessKind::Agent,
            name: "lock-agent".into(),
            command: Some("true".into()),
            working_dir: project.path.clone(),
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
        registry.store().put_process(&Process {
            id: 2,
            project_id: 1,
            kind: ProcessKind::Agent,
            name: "other-lock-agent".into(),
            command: Some("true".into()),
            working_dir: project.path,
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
        registry
            .store()
            .set_process_mcp_token(1, "first-process-token", 1_700_000_000_000)?;
        registry
            .store()
            .set_process_mcp_token(2, "other-process-token", 1_700_000_000_000)?;
    }

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let server_task = tokio::spawn(server.serve_until(async move {
        let _ = shutdown_rx.await;
    }));
    let endpoint = format!("http://127.0.0.1:{}/mcp", discovery.port);
    let first = connect(endpoint.clone(), "first-process-token".into()).await?;
    let second = connect(endpoint.clone(), "first-process-token".into()).await?;
    let other = connect(endpoint, "other-process-token".into()).await?;
    let first_actor = call(&first, "whoami", json!({})).await["actor_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let second_actor = call(&second, "whoami", json!({})).await["actor_id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_ne!(first_actor, second_actor);

    let tool_names: Vec<_> = first
        .list_all_tools()
        .await?
        .into_iter()
        .map(|tool| tool.name.into_owned())
        .collect();
    for name in ["lock_acquire", "lock_release", "lock_status"] {
        assert!(tool_names.iter().any(|candidate| candidate == name));
    }

    let acquired = call(
        &first,
        "lock_acquire",
        json!({ "lock_key": "shared.schema", "lease_ttl_seconds": 60 }),
    )
    .await;
    assert_eq!(acquired["acquired"], true);
    assert_eq!(acquired["lease"]["owner_actor"], "lock-agent");
    assert_eq!(acquired["lease"]["owner_process_id"], 1);
    assert!(acquired["lease"].get("owner_actor_id").is_none());
    assert!(!acquired.to_string().contains(&first_actor));
    let renewed_after_rotation = call(
        &second,
        "lock_acquire",
        json!({ "lock_key": "shared.schema", "lease_ttl_seconds": 60 }),
    )
    .await;
    assert_eq!(renewed_after_rotation["acquired"], true);
    assert_eq!(renewed_after_rotation["lease"]["owner_process_id"], 1);

    let denied = invoke(
        &other,
        "lock_acquire",
        json!({ "lock_key": "shared.schema", "lease_ttl_seconds": 60 }),
    )
    .await;
    assert_eq!(denied.is_error, Some(true));
    assert_eq!(denied.structured_content.unwrap()["code"], "lock_held");

    let wrong_release = invoke(
        &other,
        "lock_release",
        json!({ "lock_key": "shared.schema" }),
    )
    .await;
    assert_eq!(wrong_release.is_error, Some(true));
    assert_eq!(
        wrong_release.structured_content.unwrap()["code"],
        "lock_not_owned"
    );
    let released = call(
        &second,
        "lock_release",
        json!({ "lock_key": "shared.schema" }),
    )
    .await;
    assert_eq!(released["released"], true);
    let acquired = call(
        &other,
        "lock_acquire",
        json!({ "lock_key": "shared.schema", "lease_ttl_seconds": 60 }),
    )
    .await;
    assert_eq!(acquired["lease"]["owner_actor"], "other-lock-agent");
    assert_eq!(acquired["lease"]["owner_process_id"], 2);
    assert!(!acquired.to_string().contains(&second_actor));
    call(
        &other,
        "lock_release",
        json!({ "lock_key": "shared.schema" }),
    )
    .await;
    assert_eq!(
        call(
            &first,
            "lock_status",
            json!({ "lock_key": "shared.schema" }),
        )
        .await["lease"],
        Value::Null
    );

    call(
        &first,
        "lock_acquire",
        json!({ "lock_key": "short.lease", "lease_ttl_seconds": 1 }),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(1_100)).await;
    let acquired_after_expiry = call(
        &other,
        "lock_acquire",
        json!({ "lock_key": "short.lease", "lease_ttl_seconds": 60 }),
    )
    .await;
    assert_eq!(
        acquired_after_expiry["lease"]["owner_actor"],
        "other-lock-agent"
    );

    let _ = first.cancel().await;
    let _ = second.cancel().await;
    let _ = other.cancel().await;
    let _ = shutdown_tx.send(());
    server_task.await??;
    Ok(())
}
