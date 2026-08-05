use std::error::Error;

use rmcp::{
    ServiceExt,
    model::{CallToolRequestParams, CallToolResult, ClientInfo},
    transport::{
        StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use serde_json::{Map, Value, json};
use workman_core::Project;
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
    result
        .structured_content
        .unwrap_or_else(|| panic!("{name} returned no structured content"))
}

async fn connect(endpoint: String, bearer_token: String) -> Result<Client, Box<dyn Error>> {
    let transport = StreamableHttpClientTransport::from_config(
        StreamableHttpClientTransportConfig::with_uri(endpoint).auth_header(bearer_token),
    );
    Ok(ClientInfo::default().serve(transport).await?)
}

#[tokio::test]
async fn concurrent_mcp_sessions_cannot_double_claim_a_todo() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let first_path = temp.path().join("one");
    let second_path = temp.path().join("two");
    std::fs::create_dir_all(&first_path)?;
    std::fs::create_dir_all(&second_path)?;
    let server = DaemonServer::bind(DaemonConfig {
        data_dir: temp.path().join("state"),
        port: 0,
    })
    .await?;
    let discovery = server.discovery().clone();
    {
        let registry = server.registry();
        let registry = registry.lock().await;
        registry.store().put_project(&Project {
            id: 1,
            path: first_path.to_string_lossy().into_owned(),
            name: "one".into(),
            display_name: None,
            icon: None,
            selected: false,
            sort_order: 0,
        })?;
        registry.store().put_project(&Project {
            id: 2,
            path: second_path.to_string_lossy().into_owned(),
            name: "two".into(),
            display_name: None,
            icon: None,
            selected: false,
            sort_order: 0,
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
    let first_identity = call(&first, "whoami", json!({})).await;
    let second_identity = call(&second, "whoami", json!({})).await;
    assert_ne!(first_identity["actor_id"], second_identity["actor_id"]);

    let tool_names: Vec<_> = first
        .list_all_tools()
        .await?
        .into_iter()
        .map(|tool| tool.name.into_owned())
        .collect();
    for name in [
        "todo_create",
        "todo_get",
        "todo_update",
        "todo_delete",
        "todo_list",
        "todo_tags_list",
        "todo_add_tag",
        "todo_remove_tag",
        "todo_set_blockers",
        "todo_add_blocker",
        "todo_remove_blocker",
        "todo_comment_create",
        "todo_comment_update",
        "todo_comment_delete",
        "todo_comment_list",
        "todo_lock",
        "todo_unlock",
        "todo_complete",
        "todo_transfer",
    ] {
        assert!(
            tool_names.iter().any(|candidate| candidate == name),
            "missing {name}"
        );
    }

    let created = call(
        &first,
        "todo_create",
        json!({
            "title": "Claim exactly once",
            "body": "Concurrency acceptance test",
            "priority": "high",
            "tags": ["mcp", "coordination"]
        }),
    )
    .await;
    assert_eq!(
        created.as_object().unwrap().len(),
        2,
        "default receipt must stay slim"
    );
    let todo_id = created["todo_id"].as_i64().unwrap();
    let rich_update = call(
        &first,
        "todo_update",
        json!({
            "todo_id": todo_id,
            "title": "Claim exactly once through MCP",
            "response_mode": "rich"
        }),
    )
    .await;
    assert_eq!(rich_update["id"], todo_id);
    assert_eq!(rich_update["body"], "Concurrency acceptance test");

    let first_lock = invoke(
        &first,
        "todo_lock",
        json!({ "todo_id": todo_id, "lease_ttl_seconds": 60 }),
    );
    let second_lock = invoke(
        &second,
        "todo_lock",
        json!({ "todo_id": todo_id, "lease_ttl_seconds": 60 }),
    );
    let (first_result, second_result) = tokio::join!(first_lock, second_lock);
    let first_won = first_result.is_error != Some(true);
    let second_won = second_result.is_error != Some(true);
    assert_ne!(
        first_won, second_won,
        "exactly one MCP actor must acquire the lease"
    );
    let loser_result = if first_won {
        &second_result
    } else {
        &first_result
    };
    assert_eq!(loser_result.is_error, Some(true));
    assert_eq!(
        loser_result.structured_content.as_ref().unwrap()["code"],
        "todo_locked"
    );

    let (winner, loser) = if first_won {
        (&first, &second)
    } else {
        (&second, &first)
    };
    let completed = call(
        winner,
        "todo_complete",
        json!({ "todo_id": todo_id, "completed": true }),
    )
    .await;
    assert_eq!(completed["completed"], true);
    call(
        loser,
        "todo_lock",
        json!({ "todo_id": todo_id, "lease_ttl_seconds": 60 }),
    )
    .await;

    let comment = call(
        &first,
        "todo_comment_create",
        json!({ "todo_id": todo_id, "body": "lease verified" }),
    )
    .await;
    let comment_id = comment["comment_id"].as_i64().unwrap();
    call(
        &first,
        "todo_comment_update",
        json!({ "comment_id": comment_id, "body": "lease and comments verified" }),
    )
    .await;
    let comments = call(
        &first,
        "todo_comment_list",
        json!({ "todo_id": todo_id, "offset": 0, "limit": 10 }),
    )
    .await;
    assert_eq!(comments["total_count"], 1);

    let rich = call(
        &first,
        "todo_get",
        json!({ "todo_id": todo_id, "include_comments": true }),
    )
    .await;
    assert_eq!(rich["todo"]["completed"], true);
    assert_eq!(rich["comments"].as_array().unwrap().len(), 1);
    let listed = call(
        &first,
        "todo_list",
        json!({ "completed": true, "tags": ["coordination"], "limit": 1 }),
    )
    .await;
    assert_eq!(listed["total_count"], 1);
    assert_eq!(listed["has_more"], false);

    call(
        &first,
        "todo_transfer",
        json!({ "todo_id": todo_id, "target_project_id": 2 }),
    )
    .await;
    let transferred = call(
        &first,
        "todo_get",
        json!({ "project_id": 2, "todo_id": todo_id, "include_comments": true }),
    )
    .await;
    assert_eq!(transferred["todo"]["completed"], true);
    assert_eq!(transferred["todo"]["locked_by"], Value::Null);
    assert_eq!(transferred["comments"].as_array().unwrap().len(), 1);

    call(
        &first,
        "todo_comment_delete",
        json!({ "project_id": 2, "comment_id": comment_id }),
    )
    .await;
    call(
        &first,
        "todo_delete",
        json!({ "project_id": 2, "todo_id": todo_id }),
    )
    .await;

    let _ = first.cancel().await;
    let _ = second.cancel().await;
    let _ = shutdown_tx.send(());
    server_task.await??;
    Ok(())
}
