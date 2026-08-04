use std::{error::Error, time::Duration};

use gbuild_core::Project;
use gbuildd::{DaemonConfig, DaemonServer};
use rmcp::{
    ServiceExt,
    model::{CallToolRequestParams, CallToolResult, ClientInfo},
    transport::{
        StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use serde_json::{Map, Value, json};

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
async fn second_mcp_session_waits_for_release_or_expiry() -> Result<(), Box<dyn Error>> {
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
        registry.lock().await.store().put_project(&Project {
            id: 1,
            path: project_path.to_string_lossy().into_owned(),
            name: "project".into(),
            display_name: None,
            icon: None,
            selected: false,
        })?;
    }

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let server_task = tokio::spawn(server.serve_until(async move {
        let _ = shutdown_rx.await;
    }));
    let endpoint = format!("http://127.0.0.1:{}/mcp", discovery.port);
    let first = connect(endpoint.clone(), discovery.token.clone()).await?;
    let second = connect(endpoint, discovery.token.clone()).await?;
    call(&first, "select_project", json!({ "project_id": 1 })).await;
    call(&second, "select_project", json!({ "project_id": 1 })).await;
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
    assert_eq!(acquired["lease"]["owner_actor_id"], first_actor);
    let denied = invoke(
        &second,
        "lock_acquire",
        json!({ "lock_key": "shared.schema", "lease_ttl_seconds": 60 }),
    )
    .await;
    assert_eq!(denied.is_error, Some(true));
    assert_eq!(denied.structured_content.unwrap()["code"], "lock_held");

    let wrong_release = invoke(
        &second,
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
        &first,
        "lock_release",
        json!({ "lock_key": "shared.schema" }),
    )
    .await;
    assert_eq!(released["released"], true);
    let acquired = call(
        &second,
        "lock_acquire",
        json!({ "lock_key": "shared.schema", "lease_ttl_seconds": 60 }),
    )
    .await;
    assert_eq!(acquired["lease"]["owner_actor_id"], second_actor);
    call(
        &second,
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
        &second,
        "lock_acquire",
        json!({ "lock_key": "short.lease", "lease_ttl_seconds": 60 }),
    )
    .await;
    assert_eq!(
        acquired_after_expiry["lease"]["owner_actor_id"],
        second_actor
    );

    let _ = first.cancel().await;
    let _ = second.cancel().await;
    let _ = shutdown_tx.send(());
    server_task.await??;
    Ok(())
}
