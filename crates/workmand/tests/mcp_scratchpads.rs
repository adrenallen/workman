use std::{collections::BTreeMap, error::Error, fs};

use rmcp::{
    ServiceExt,
    model::{CallToolRequestParams, CallToolResult, ClientInfo},
    transport::{
        StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use serde_json::{Map, Value, json};
use workman_core::{
    NewScratchpadComment, Process, ProcessKind, ProcessSource, ProcessStatus, Project,
    ScratchpadService,
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
        registry.store().put_project(&Project {
            id: 2,
            path: second_path.to_string_lossy().into_owned(),
            name: "two".into(),
            display_name: None,
            icon: None,
            selected: false,
            sort_order: 0,
        })?;
        registry.store().put_process(&Process {
            id: 1,
            project_id: 1,
            kind: ProcessKind::Agent,
            name: "scratchpad-agent".into(),
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
            project_id: 2,
            kind: ProcessKind::Agent,
            name: "other-project-agent".into(),
            command: Some("true".into()),
            working_dir: second_path.to_string_lossy().into_owned(),
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
            id: 3,
            project_id: 1,
            kind: ProcessKind::Agent,
            name: "second-scratchpad-agent".into(),
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
    }

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let server_task = tokio::spawn(server.serve_until(async move {
        let _ = shutdown_rx.await;
    }));
    let endpoint = format!("http://127.0.0.1:{}/mcp", discovery.port);
    let first = connect(endpoint.clone(), discovery.token.clone()).await?;
    let second = connect(endpoint.clone(), discovery.token.clone()).await?;
    let cross_project = connect(endpoint, discovery.token.clone()).await?;
    call(&first, "identify_session", json!({ "process_id": 1 })).await;
    call(&second, "identify_session", json!({ "process_id": 3 })).await;
    call(
        &cross_project,
        "identify_session",
        json!({ "process_id": 2 }),
    )
    .await;

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
    let read_tool = tools
        .iter()
        .find(|tool| tool.name == "scratchpad_read")
        .expect("scratchpad_read tool is registered");
    assert!(
        read_tool.input_schema["properties"]
            .get("include_comments")
            .is_some()
    );
    let comment_create_tool = tools
        .iter()
        .find(|tool| tool.name == "scratchpad_comment_create")
        .expect("scratchpad_comment_create tool is registered");
    for property in [
        "scratchpad_id",
        "body",
        "quote",
        "anchor_start",
        "anchor_end",
        "anchor_prefix",
        "anchor_suffix",
        "allow_unanchored",
        "expected_revision",
    ] {
        assert!(
            comment_create_tool.input_schema["properties"]
                .get(property)
                .is_some(),
            "scratchpad_comment_create is missing {property}"
        );
    }
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
        "scratchpad_comment_create",
        "scratchpad_comment_list",
        "scratchpad_comment_update",
        "scratchpad_comment_resolve",
        "scratchpad_comment_delete",
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
            "tags": ["Planning", "MCP", "planning"],
            "actor": "Garrett"
        }),
    )
    .await;
    assert_eq!(created["created"], true);
    assert_eq!(created["revision"], 1);
    assert_eq!(created["name"], "Shared Plan");
    let scratchpad_id = created["scratchpad_id"].as_i64().unwrap();
    {
        let registry = registry_handle.lock().await;
        let attribution: (String, String) = registry.store().connection().query_row(
            "SELECT created_by, updated_by FROM scratchpads WHERE id = ?1",
            [scratchpad_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        assert_eq!(
            attribution,
            ("scratchpad-agent".into(), "scratchpad-agent".into())
        );
    }

    let user_comment_id = {
        let registry = registry_handle.lock().await;
        ScratchpadService::attributed(registry.store(), "user")
            .comment_create(
                1,
                scratchpad_id,
                NewScratchpadComment {
                    body: "User review that agents must preserve.".into(),
                    ..NewScratchpadComment::default()
                },
                1,
            )?
            .comment
            .id
    };
    for (tool, args) in [
        (
            "scratchpad_comment_update",
            json!({ "comment_id": user_comment_id, "body": "agent rewrite" }),
        ),
        (
            "scratchpad_comment_resolve",
            json!({ "comment_id": user_comment_id }),
        ),
        (
            "scratchpad_comment_delete",
            json!({ "comment_id": user_comment_id }),
        ),
    ] {
        let denied = invoke(&first, tool, args).await;
        assert_error_code(&denied, "scratchpad_comment_permission_denied");
    }

    let comment = call(
        &first,
        "scratchpad_comment_create",
        json!({
            "scratchpad_id": scratchpad_id,
            "body": "Make this step concrete.",
            "quote": "Alpha"
        }),
    )
    .await;
    assert_eq!(comment["actor"], "scratchpad-agent");
    assert_eq!(comment["can_edit"], true);
    assert_eq!(comment["anchor_state"], "anchored");
    assert_eq!(comment["current_start_line"], 5);
    let comment_id = comment["id"].as_i64().unwrap();

    for tool in [
        (
            "scratchpad_comment_update",
            json!({ "comment_id": comment_id, "body": "not mine" }),
        ),
        (
            "scratchpad_comment_resolve",
            json!({ "comment_id": comment_id }),
        ),
        (
            "scratchpad_comment_delete",
            json!({ "comment_id": comment_id }),
        ),
    ] {
        let denied = invoke(&second, tool.0, tool.1).await;
        assert_error_code(&denied, "scratchpad_comment_permission_denied");
    }
    for (tool, args) in [
        (
            "scratchpad_comment_update",
            json!({ "comment_id": comment_id, "body": "cross project" }),
        ),
        (
            "scratchpad_comment_resolve",
            json!({ "comment_id": comment_id }),
        ),
        (
            "scratchpad_comment_delete",
            json!({ "comment_id": comment_id }),
        ),
    ] {
        let jailed = invoke(&cross_project, tool, args).await;
        assert_error_code(&jailed, "scratchpad_not_found");
    }
    let jailed_create = invoke(
        &cross_project,
        "scratchpad_comment_create",
        json!({ "scratchpad_id": scratchpad_id, "body": "cross project" }),
    )
    .await;
    assert_error_code(&jailed_create, "scratchpad_not_found");

    let commented_read = call(
        &first,
        "scratchpad_read",
        json!({ "scratchpad_id": scratchpad_id, "include_comments": true }),
    )
    .await;
    assert_eq!(commented_read["unresolved_comment_count"], 2);
    assert_eq!(
        commented_read["comments"][1]["body"],
        "Make this step concrete."
    );
    let updated_comment = call(
        &first,
        "scratchpad_comment_update",
        json!({ "comment_id": comment_id, "body": "Concrete now." }),
    )
    .await;
    assert_eq!(updated_comment["body"], "Concrete now.");
    let resolved_comment = call(
        &first,
        "scratchpad_comment_resolve",
        json!({ "comment_id": comment_id }),
    )
    .await;
    assert_eq!(resolved_comment["resolved"], true);
    let default_commented_read = call(
        &first,
        "scratchpad_read",
        json!({ "scratchpad_id": scratchpad_id, "include_comments": true }),
    )
    .await;
    assert_eq!(
        default_commented_read["comments"].as_array().unwrap().len(),
        1
    );
    let resolved_commented_read = call(
        &first,
        "scratchpad_read",
        json!({
            "scratchpad_id": scratchpad_id,
            "include_comments": true,
            "include_resolved": true
        }),
    )
    .await;
    assert_eq!(
        resolved_commented_read["comments"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let unresolved_comments = call(
        &first,
        "scratchpad_comment_list",
        json!({ "scratchpad_id": scratchpad_id }),
    )
    .await;
    assert_eq!(unresolved_comments["comments"].as_array().unwrap().len(), 1);
    let all_comments = call(
        &first,
        "scratchpad_comment_list",
        json!({ "scratchpad_id": scratchpad_id, "include_resolved": true }),
    )
    .await;
    assert_eq!(all_comments["comments"].as_array().unwrap().len(), 2);
    assert_eq!(all_comments["offset"], 0);
    assert_eq!(all_comments["has_more"], false);
    let missing_quote = invoke(
        &first,
        "scratchpad_comment_create",
        json!({ "scratchpad_id": scratchpad_id, "body": "Missing", "quote": "not here" }),
    )
    .await;
    assert_error_code(&missing_quote, "invalid_scratchpad_input");
    let orphaned = call(
        &first,
        "scratchpad_comment_create",
        json!({
            "scratchpad_id": scratchpad_id,
            "body": "Keep this reference.",
            "quote": "not here",
            "allow_unanchored": true
        }),
    )
    .await;
    assert_eq!(orphaned["anchor_state"], "orphaned");
    call(
        &first,
        "scratchpad_comment_delete",
        json!({ "comment_id": comment_id }),
    )
    .await;

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
    assert_eq!(headings["scratchpad"]["created_by"], "scratchpad-agent");
    assert_eq!(headings["scratchpad"]["updated_by"], "scratchpad-agent");
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
    assert_eq!(tail["updated_by"], "scratchpad-agent");
    let listed = call(
        &first,
        "scratchpad_list",
        json!({ "query": "Replacement", "tags": ["MCP"], "limit": 10 }),
    )
    .await;
    assert_eq!(listed["total_count"], 1);
    assert_eq!(listed["scratchpads"][0]["updated_by"], "scratchpad-agent");
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

    let transferred = invoke(
        &first,
        "scratchpad_transfer",
        json!({
            "scratchpad_id": imported_id,
            "target_project_id": 2,
            "expected_revision": 1
        }),
    )
    .await;
    assert_error_code(&transferred, "project_scope_error");
    let cleared = call(
        &first,
        "scratchpad_clear",
        json!({
            "scratchpad_id": imported_id,
            "expected_revision": 1
        }),
    )
    .await;
    assert_eq!(cleared["revision"], 2);
    call(
        &first,
        "scratchpad_delete",
        json!({
            "scratchpad_id": imported_id,
            "expected_revision": 2
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
    let _ = cross_project.cancel().await;
    let _ = shutdown_tx.send(());
    server_task.await??;
    Ok(())
}
