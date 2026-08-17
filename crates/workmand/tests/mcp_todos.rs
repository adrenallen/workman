use std::{collections::BTreeMap, error::Error};

use rmcp::{
    ServiceExt,
    model::{CallToolRequestParams, CallToolResult, ClientInfo},
    transport::{
        StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use serde_json::{Map, Value, json};
use workman_core::{
    Actor, NotificationType, Process, ProcessKind, ProcessSource, ProcessStatus, Project,
    TodoService,
};
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
async fn todo_lock_ownership_survives_actor_rotation_and_rejects_other_processes()
-> Result<(), Box<dyn Error>> {
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
    let registry_handle = server.registry();
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
        registry.store().put_process(&Process {
            id: 1,
            project_id: 1,
            kind: ProcessKind::Agent,
            name: "todo-agent".into(),
            command: Some("true".into()),
            working_dir: first_path.to_string_lossy().into_owned(),
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
            name: "other-todo-agent".into(),
            command: Some("true".into()),
            working_dir: first_path.to_string_lossy().into_owned(),
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
        registry.store().put_project(&Project {
            id: 2,
            path: second_path.to_string_lossy().into_owned(),
            name: "two".into(),
            display_name: None,
            icon: None,
            selected: false,
            sort_order: 0,
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
    let first_identity = call(&first, "whoami", json!({})).await;
    let second_identity = call(&second, "whoami", json!({})).await;
    assert_ne!(first_identity["actor_id"], second_identity["actor_id"]);

    let tools = first.list_all_tools().await?;
    let tool_names: Vec<_> = tools
        .into_iter()
        .map(|tool| tool.name.into_owned())
        .collect();
    for name in [
        "todo_create",
        "todo_assign",
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
    let handoff_help = call(&first, "help", json!({ "topic": "todos" })).await;
    assert!(
        handoff_help["text"]
            .as_str()
            .unwrap()
            .contains("todo_assign")
    );
    assert!(handoff_help["text"].as_str().unwrap().contains("@user"));

    let created = call(
        &first,
        "todo_create",
        json!({
            "title": "Claim exactly once",
            "body": "Concurrency acceptance test",
            "priority": "high",
            "tags": ["mcp", "coordination"],
            "actor": "Garrett"
        }),
    )
    .await;
    assert_eq!(
        created.as_object().unwrap().len(),
        2,
        "default receipt must stay slim"
    );
    let todo_id = created["todo_id"].as_i64().unwrap();
    let assigned = call(
        &first,
        "todo_assign",
        json!({ "todo_id": todo_id, "assignee": "user", "response_mode": "rich" }),
    )
    .await;
    assert_eq!(assigned["assignee"], "user");
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
        json!({ "todo_id": todo_id, "lease_ttl_seconds": 60, "response_mode": "rich" }),
    );
    let second_lock = invoke(
        &second,
        "todo_lock",
        json!({ "todo_id": todo_id, "lease_ttl_seconds": 60, "response_mode": "rich" }),
    );
    let (first_result, second_result) = tokio::join!(first_lock, second_lock);
    assert_ne!(first_result.is_error, Some(true));
    assert_ne!(second_result.is_error, Some(true));
    assert_eq!(
        second_result.structured_content.as_ref().unwrap()["locked_by"],
        "todo-agent"
    );

    let other_result = invoke(
        &other,
        "todo_lock",
        json!({ "todo_id": todo_id, "lease_ttl_seconds": 60 }),
    )
    .await;
    assert_eq!(other_result.is_error, Some(true));
    assert_eq!(
        other_result.structured_content.as_ref().unwrap()["code"],
        "todo_locked"
    );
    let wrong_unlock = invoke(&other, "todo_unlock", json!({ "todo_id": todo_id })).await;
    assert_eq!(wrong_unlock.is_error, Some(true));
    assert_eq!(
        wrong_unlock.structured_content.as_ref().unwrap()["code"],
        "todo_lock_not_owned"
    );
    call(&second, "todo_unlock", json!({ "todo_id": todo_id })).await;
    call(
        &first,
        "todo_lock",
        json!({ "todo_id": todo_id, "lease_ttl_seconds": 60 }),
    )
    .await;
    let completed = call(
        &second,
        "todo_complete",
        json!({ "todo_id": todo_id, "completed": true }),
    )
    .await;
    assert_eq!(completed["completed"], true);
    call(
        &other,
        "todo_lock",
        json!({ "todo_id": todo_id, "lease_ttl_seconds": 60 }),
    )
    .await;

    let comment = call(
        &first,
        "todo_comment_create",
        json!({
            "todo_id": todo_id,
            "body": "@user, lease verified",
            "actor": "Garrett"
        }),
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
    {
        let registry = registry_handle.lock().await;
        let notifications = registry.store().list_notifications(None, 20)?;
        assert_eq!(notifications.len(), 2);
        assert!(
            notifications
                .iter()
                .any(|row| row.kind == NotificationType::TodoAssignedToYou)
        );
        assert!(notifications.iter().any(|row| {
            row.kind == NotificationType::MentionedInComment && row.comment_id == Some(comment_id)
        }));
    }

    let rich = call(
        &first,
        "todo_get",
        json!({ "todo_id": todo_id, "include_comments": true }),
    )
    .await;
    assert_eq!(rich["todo"]["completed"], true);
    assert_eq!(rich["comments"].as_array().unwrap().len(), 1);
    assert_eq!(
        rich["comments"][0]["actor"], "todo-agent",
        "client-supplied actor names must not cross the server trust boundary"
    );
    {
        let registry = registry_handle.lock().await;
        let activity_actors = registry
            .store()
            .connection()
            .prepare("SELECT actor FROM todo_activity WHERE todo_id = ?1 ORDER BY id")?
            .query_map([todo_id], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        assert!(!activity_actors.is_empty());
        assert!(
            activity_actors
                .iter()
                .all(|actor| actor.starts_with("mcp-"))
        );
        let stored_comment_actor: String = registry.store().connection().query_row(
            "SELECT actor FROM todo_comments WHERE id = ?1",
            [comment_id],
            |row| row.get(0),
        )?;
        assert_eq!(
            stored_comment_actor,
            first_identity["actor_id"].as_str().unwrap()
        );
        registry.store().connection().execute(
            "INSERT INTO todo_activity (todo_id, actor, kind, created_at)
             VALUES (?1, ?2, 'locked', 9999999999999)",
            rusqlite::params![todo_id, first_identity["actor_id"].as_str().unwrap()],
        )?;
        let activity = TodoService::new(registry.store()).activity_list(1, todo_id, 10_000)?;
        assert_eq!(activity.last().unwrap().actor, "todo-agent");
        registry.store().connection().execute(
            "UPDATE processes SET name = 'renamed-todo-agent' WHERE id = 1",
            [],
        )?;
        let activity = TodoService::new(registry.store()).activity_list(1, todo_id, 10_000)?;
        assert_eq!(activity.last().unwrap().actor, "renamed-todo-agent");
        let comments =
            TodoService::new(registry.store()).comment_list(1, todo_id, 0, Some(10), 10_000)?;
        assert_eq!(comments.comments[0].actor, "renamed-todo-agent");
        let external_actor_id = "mcp-0123456789abcdef";
        registry.store().put_actor(&Actor {
            id: external_actor_id.into(),
            session_id: "external-session".into(),
            process_id: None,
            selected_project_id: None,
            created_at: 1,
            last_seen_at: 1,
        })?;
        registry.store().connection().execute(
            "INSERT INTO todo_activity (todo_id, actor, kind, created_at)
             VALUES (?1, ?2, 'locked', 99999999999999)",
            rusqlite::params![todo_id, external_actor_id],
        )?;
        let activity = TodoService::new(registry.store()).activity_list(1, todo_id, 10_000)?;
        assert_eq!(activity.last().unwrap().actor, "session");
        assert!(!activity.last().unwrap().actor.contains(external_actor_id));
    }
    let listed = call(
        &first,
        "todo_list",
        json!({ "completed": true, "assignee": "user", "tags": ["coordination"], "limit": 1 }),
    )
    .await;
    assert_eq!(listed["total_count"], 1);
    assert_eq!(listed["has_more"], false);

    let transfer = invoke(
        &first,
        "todo_transfer",
        json!({ "todo_id": todo_id, "target_project_id": 2 }),
    )
    .await;
    assert_eq!(transfer.is_error, Some(true));
    assert_eq!(
        transfer.structured_content.unwrap()["code"],
        "project_scope_error"
    );

    call(
        &first,
        "todo_comment_delete",
        json!({ "comment_id": comment_id }),
    )
    .await;
    call(&first, "todo_delete", json!({ "todo_id": todo_id })).await;

    let _ = first.cancel().await;
    let _ = second.cancel().await;
    let _ = other.cancel().await;
    let _ = shutdown_tx.send(());
    server_task.await??;
    Ok(())
}
