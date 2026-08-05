use std::{error::Error, fs};

use awm_core::Project;
use awmd::{DaemonConfig, DaemonServer};
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
    result
        .structured_content
        .unwrap_or_else(|| panic!("{name} returned no structured content"))
}

fn assert_error_code(result: &CallToolResult, expected: &str) {
    assert_eq!(
        result.is_error,
        Some(true),
        "expected error, got {result:?}"
    );
    assert_eq!(
        result.structured_content.as_ref().unwrap()["code"],
        expected
    );
}

async fn connect(endpoint: String, bearer_token: String) -> Result<Client, Box<dyn Error>> {
    let transport = StreamableHttpClientTransport::from_config(
        StreamableHttpClientTransportConfig::with_uri(endpoint).auth_header(bearer_token),
    );
    Ok(ClientInfo::default().serve(transport).await?)
}

#[tokio::test]
async fn rmcp_scratchpads_reject_stale_writes_and_contain_relative_files()
-> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let first_path = temp.path().join("one");
    let second_path = temp.path().join("two");
    let outside_path = temp.path().join("outside");
    fs::create_dir_all(first_path.join("imports"))?;
    fs::create_dir_all(&second_path)?;
    fs::create_dir_all(&outside_path)?;
    fs::write(
        first_path.join("imports/source.md"),
        "# Imported Title\n\nImported body.",
    )?;
    fs::write(outside_path.join("secret.md"), "# Outside\n\nSecret")?;
    fs::write(temp.path().join("outside.md"), "# Escaped\n\nNope")?;

    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside_path, first_path.join("outside-link"))?;

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

    let tools = first.list_all_tools().await?;
    let append_section_tool = tools
        .iter()
        .find(|tool| tool.name == "scratchpad_append_section")
        .expect("scratchpad_append_section tool is registered");
    assert!(
        append_section_tool
            .description
            .as_deref()
            .unwrap_or_default()
            .contains("create_heading=true")
    );
    assert!(
        append_section_tool
            .input_schema
            .get("properties")
            .and_then(Value::as_object)
            .is_some_and(|properties| properties.contains_key("create_heading"))
    );
    let tool_names = tools
        .into_iter()
        .map(|tool| tool.name.into_owned())
        .collect::<Vec<_>>();
    for name in [
        "scratchpad_write",
        "scratchpad_read",
        "scratchpad_append",
        "scratchpad_append_section",
        "scratchpad_edit",
        "scratchpad_find",
        "scratchpad_tail",
        "scratchpad_list",
        "scratchpad_rename",
        "scratchpad_add_tags",
        "scratchpad_remove_tags",
        "scratchpad_tags_list",
        "scratchpad_archive",
        "scratchpad_clear",
        "scratchpad_delete",
        "scratchpad_transfer",
        "scratchpad_save_to_file",
        "scratchpad_load_from_file",
    ] {
        assert!(
            tool_names.iter().any(|candidate| candidate == name),
            "missing {name}; available tools: {tool_names:?}"
        );
    }

    let created = call(
        &first,
        "scratchpad_write",
        json!({
            "name": "ignored by H1",
            "content": "# Shared Plan\n\nIntro\n\n## Next   Steps\n\nAlpha\nBeta\n\n### Detail\n\nGamma",
            "tags": ["Planning", "MCP", "planning"]
        }),
    )
    .await;
    assert_eq!(created["created"], true);
    assert_eq!(created["revision"], 1);
    assert_eq!(created["name"], "Shared Plan");
    let scratchpad_id = created["scratchpad_id"].as_i64().unwrap();

    let headings = call(
        &first,
        "scratchpad_read",
        json!({ "scratchpad_id": scratchpad_id, "mode": "headings" }),
    )
    .await;
    assert_eq!(
        headings["scratchpad"]["content"],
        "# Shared Plan\n## Next Steps\n### Detail"
    );
    let section = call(
        &first,
        "scratchpad_read",
        json!({
            "scratchpad_id": scratchpad_id,
            "mode": "section",
            "section_heading": " next steps "
        }),
    )
    .await;
    assert!(
        section["scratchpad"]["content"]
            .as_str()
            .unwrap()
            .contains("### Detail")
    );
    let slice = call(
        &first,
        "scratchpad_read",
        json!({
            "scratchpad_id": scratchpad_id,
            "mode": "line_slice",
            "offset": 2,
            "limit": 2
        }),
    )
    .await;
    assert_eq!(slice["returned_lines"], 2);
    assert_eq!(slice["has_more"], true);
    let title_section = call(
        &first,
        "scratchpad_read",
        json!({
            "scratchpad_id": scratchpad_id,
            "mode": "section",
            "section_heading": "shared plan"
        }),
    )
    .await;
    assert!(
        title_section["scratchpad"]["content"]
            .as_str()
            .unwrap()
            .starts_with("# Shared Plan\n\nIntro")
    );

    let first_write = invoke(
        &first,
        "scratchpad_write",
        json!({
            "scratchpad_id": scratchpad_id,
            "name": "Shared Plan",
            "content": "## Work\n\nFirst writer",
            "expected_revision": 1
        }),
    );
    let second_write = invoke(
        &second,
        "scratchpad_write",
        json!({
            "scratchpad_id": scratchpad_id,
            "name": "Shared Plan",
            "content": "## Work\n\nSecond writer",
            "expected_revision": 1
        }),
    );
    let (first_result, second_result) = tokio::join!(first_write, second_write);
    let first_succeeded = first_result.is_error != Some(true);
    let second_succeeded = second_result.is_error != Some(true);
    assert_ne!(
        first_succeeded, second_succeeded,
        "exactly one concurrent revision-1 write must succeed"
    );
    let stale = if first_succeeded {
        &second_result
    } else {
        &first_result
    };
    assert_error_code(stale, "scratchpad_revision_conflict");

    let fresh = call(
        &first,
        "scratchpad_write",
        json!({
            "scratchpad_id": scratchpad_id,
            "name": "Shared Plan",
            "content": "## Work\n\nFresh writer\n\n### Child\n\nNested",
            "expected_revision": 2,
            "tags": ["planning", "mcp"]
        }),
    )
    .await;
    assert_eq!(fresh["revision"], 3);

    let appended = call(
        &first,
        "scratchpad_append_section",
        json!({
            "scratchpad_id": scratchpad_id,
            "heading": " WORK ",
            "content": "Appended",
            "expected_revision": 3
        }),
    )
    .await;
    assert_eq!(appended["revision"], 4);
    let appended = call(
        &first,
        "scratchpad_append",
        json!({
            "scratchpad_id": scratchpad_id,
            "content": "Tail marker",
            "expected_revision": 4
        }),
    )
    .await;
    assert_eq!(appended["revision"], 5);
    let edited = call(
        &first,
        "scratchpad_edit",
        json!({
            "scratchpad_id": scratchpad_id,
            "expected_revision": 5,
            "target": { "type": "line_range", "offset": 2, "limit": 1 },
            "content": "Edited line"
        }),
    )
    .await;
    assert_eq!(edited["revision"], 6);
    let edited = call(
        &first,
        "scratchpad_edit",
        json!({
            "scratchpad_id": scratchpad_id,
            "expected_revision": 6,
            "target": { "type": "section", "section_heading": "child" },
            "content": "Replacement child body"
        }),
    )
    .await;
    assert_eq!(edited["revision"], 7);

    let found = call(
        &first,
        "scratchpad_find",
        json!({
            "scratchpad_id": scratchpad_id,
            "query": "replacement",
            "limit": 2,
            "context_lines": 1
        }),
    )
    .await;
    assert_eq!(found["total_matches"], 1);
    assert_eq!(found["matches"][0]["kind"], "content");
    let tail = call(
        &first,
        "scratchpad_tail",
        json!({ "scratchpad_id": scratchpad_id, "lines": 2 }),
    )
    .await;
    assert_eq!(tail["requested_lines"], 2);
    assert_eq!(tail["returned_lines"], 2);
    let listed = call(
        &first,
        "scratchpad_list",
        json!({ "query": "Replacement", "tags": ["MCP"], "limit": 10 }),
    )
    .await;
    assert_eq!(listed["total_count"], 1);
    assert_eq!(listed["scratchpads"][0]["matched_fields"][0], "content");
    assert!(
        listed["scratchpads"][0]["match_snippet"]
            .as_str()
            .unwrap()
            .contains("Replacement")
    );

    let renamed = call(
        &first,
        "scratchpad_rename",
        json!({
            "scratchpad_id": scratchpad_id,
            "name": "Delivery Plan",
            "expected_revision": 7
        }),
    )
    .await;
    assert_eq!(renamed["revision"], 8);
    let tagged = call(
        &first,
        "scratchpad_add_tags",
        json!({
            "scratchpad_id": scratchpad_id,
            "tags": ["Shared", "MCP"],
            "expected_revision": 8
        }),
    )
    .await;
    assert_eq!(tagged["revision"], 9);
    let tagged = call(
        &first,
        "scratchpad_remove_tags",
        json!({
            "scratchpad_id": scratchpad_id,
            "tags": ["planning"],
            "expected_revision": 9
        }),
    )
    .await;
    assert_eq!(tagged["revision"], 10);
    let tags = call(&first, "scratchpad_tags_list", json!({})).await;
    assert_eq!(tags["tags"], json!(["mcp", "shared"]));

    let missing_strict = invoke(
        &first,
        "scratchpad_append_section",
        json!({
            "scratchpad_id": scratchpad_id,
            "heading": "Integration Summary",
            "content": "Joined result",
            "expected_revision": 10
        }),
    )
    .await;
    assert_error_code(&missing_strict, "scratchpad_heading_not_found");
    let stale_create = invoke(
        &first,
        "scratchpad_append_section",
        json!({
            "scratchpad_id": scratchpad_id,
            "heading": "Integration Summary",
            "content": "Stale result",
            "create_heading": true,
            "expected_revision": 9
        }),
    )
    .await;
    assert_error_code(&stale_create, "scratchpad_revision_conflict");
    let created_section = call(
        &first,
        "scratchpad_append_section",
        json!({
            "scratchpad_id": scratchpad_id,
            "heading": "Integration Summary",
            "content": "Joined result",
            "create_heading": true,
            "expected_revision": 10
        }),
    )
    .await;
    assert_eq!(created_section["revision"], 11);
    let created_read = call(
        &first,
        "scratchpad_read",
        json!({ "scratchpad_id": scratchpad_id }),
    )
    .await;
    assert!(
        created_read["scratchpad"]["content"]
            .as_str()
            .unwrap()
            .ends_with("## Integration Summary\n\nJoined result")
    );

    call(
        &first,
        "scratchpad_save_to_file",
        json!({ "scratchpad_id": scratchpad_id, "path": "exports/plan.md" }),
    )
    .await;
    let saved = fs::read_to_string(first_path.join("exports/plan.md"))?;
    assert!(saved.starts_with("# Delivery Plan\n\n"));

    let loaded = call(
        &first,
        "scratchpad_load_from_file",
        json!({ "name": "fallback", "path": "imports/source.md" }),
    )
    .await;
    let imported_id = loaded["scratchpad_id"].as_i64().unwrap();
    assert_eq!(loaded["name"], "Imported Title");
    assert_eq!(loaded["revision"], 1);

    for (tool, args) in [
        (
            "scratchpad_save_to_file",
            json!({ "scratchpad_id": scratchpad_id, "path": "../escaped.md" }),
        ),
        (
            "scratchpad_load_from_file",
            json!({ "name": "escape", "path": "../outside.md" }),
        ),
        (
            "scratchpad_save_to_file",
            json!({ "scratchpad_id": scratchpad_id, "path": "outside-link/new.md" }),
        ),
        (
            "scratchpad_load_from_file",
            json!({ "name": "symlink", "path": "outside-link/secret.md" }),
        ),
    ] {
        let result = invoke(&first, tool, args).await;
        assert_error_code(&result, "scratchpad_path_escape");
    }
    assert!(!temp.path().join("escaped.md").exists());

    let archived = call(
        &first,
        "scratchpad_archive",
        json!({ "scratchpad_id": scratchpad_id, "expected_revision": 11 }),
    )
    .await;
    assert_eq!(archived["revision"], 12);
    assert_eq!(archived["archived"], true);
    let listed = call(&first, "scratchpad_list", json!({})).await;
    assert_eq!(listed["total_count"], 1, "archived scratchpad is hidden");

    let transferred = call(
        &first,
        "scratchpad_transfer",
        json!({
            "scratchpad_id": imported_id,
            "target_project_id": 2,
            "expected_revision": 1
        }),
    )
    .await;
    assert_eq!(transferred["target_project_id"], 2);
    assert_eq!(transferred["revision"], 2);
    let cleared = call(
        &first,
        "scratchpad_clear",
        json!({
            "project_id": 2,
            "scratchpad_id": imported_id,
            "expected_revision": 2
        }),
    )
    .await;
    assert_eq!(cleared["revision"], 3);
    call(
        &first,
        "scratchpad_delete",
        json!({
            "project_id": 2,
            "scratchpad_id": imported_id,
            "expected_revision": 3
        }),
    )
    .await;
    call(
        &first,
        "scratchpad_delete",
        json!({ "scratchpad_id": scratchpad_id, "expected_revision": 12 }),
    )
    .await;

    let _ = first.cancel().await;
    let _ = second.cancel().await;
    let _ = shutdown_tx.send(());
    server_task.await??;
    Ok(())
}
