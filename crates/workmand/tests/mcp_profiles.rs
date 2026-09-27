use std::{collections::BTreeMap, error::Error};

use rmcp::{
    ServiceExt,
    model::{CallToolRequestParams, ClientInfo, Implementation},
    transport::{
        StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use serde_json::{Map, Value, json};
use workman_core::{McpToolsProfile, Process, ProcessKind, ProcessSource, ProcessStatus, Project};
use workmand::{DaemonConfig, DaemonServer};

const CORE_TOOLS: &[&str] = &[
    "close_process",
    "get_process_output",
    "get_process_status",
    "help",
    "list_agent_tools",
    "list_processes",
    "process_control",
    "scratchpad_append",
    "scratchpad_comment_create",
    "scratchpad_comment_update",
    "scratchpad_edit",
    "scratchpad_find",
    "scratchpad_list",
    "scratchpad_read",
    "scratchpad_update",
    "scratchpad_write",
    "search_output",
    "send_input",
    "spawn_agent",
    "timer_cancel",
    "timer_fire_when_idle",
    "timer_list",
    "timer_set",
    "todo_comment_create",
    "todo_complete",
    "todo_create",
    "todo_delete",
    "todo_get",
    "todo_list",
    "todo_lock",
    "todo_unlock",
    "todo_update",
    "whoami",
];

const EXTENDED_TOOLS: &[&str] = &[
    "agent_tool_check",
    "clear_output",
    "commands_control",
    "lock",
    "project_update",
    "scratchpad_comment_delete",
    "scratchpad_delete",
    "scratchpad_load_from_file",
    "scratchpad_save_to_file",
    "services_list",
    "spawn_terminal",
    "timer_pause",
    "todo_comment_delete",
    "todo_comment_update",
    "update_process",
    "wait_for_bound_port",
    "worktree_env_forget",
    "worktree_health",
    "worktree_list",
];

fn arguments(value: Value) -> Map<String, Value> {
    value
        .as_object()
        .expect("tool arguments must be an object")
        .clone()
}

fn client_info(name: &str) -> ClientInfo {
    let mut info = ClientInfo::default();
    info.client_info = Implementation::new(name, "test");
    info
}

fn process(id: i64, project_path: &str, name: &str) -> Process {
    Process {
        id,
        project_id: 1,
        kind: ProcessKind::Agent,
        name: name.into(),
        command: Some("sleep 30".into()),
        working_dir: project_path.into(),
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
    }
}

async fn tool_names(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ClientInfo>,
) -> Result<Vec<String>, Box<dyn Error>> {
    let mut names = client
        .list_all_tools()
        .await?
        .into_iter()
        .map(|tool| tool.name.into_owned())
        .collect::<Vec<_>>();
    names.sort();
    Ok(names)
}

#[tokio::test]
async fn profiles_filter_every_client_handshake_and_keep_user_surface_full()
-> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let project_dir = temp.path().join("project");
    std::fs::create_dir_all(&project_dir)?;
    let server = DaemonServer::bind(DaemonConfig {
        data_dir: temp.path().join("state"),
        port: 0,
    })
    .await?;
    let discovery = server.discovery().clone();
    let registry = server.registry();
    let (core_token, extended_token) = {
        let mut registry = registry.lock().await;
        registry.store().put_project(&Project {
            id: 1,
            path: project_dir.to_string_lossy().into_owned(),
            name: "profile-test".into(),
            display_name: None,
            icon: None,
            selected: true,
            sort_order: 0,
        })?;
        registry
            .store()
            .put_process(&process(41, &project_dir.to_string_lossy(), "core-agent"))?;
        registry.store().put_process(&process(
            42,
            &project_dir.to_string_lossy(),
            "extended-agent",
        ))?;
        registry
            .store()
            .set_process_mcp_tools_profile(42, McpToolsProfile::Extended)?;
        registry.start(41)?;
        registry.start(42)?;
        let token = |process_id| {
            registry.store().connection().query_row(
                "SELECT token FROM process_mcp_tokens WHERE process_id = ?1",
                [process_id],
                |row| row.get::<_, String>(0),
            )
        };
        (token(41)?, token(42)?)
    };

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let server_task = tokio::spawn(server.serve_until(async move {
        let _ = shutdown_rx.await;
    }));
    let stateful = format!("http://127.0.0.1:{}/mcp", discovery.port);
    let stateless = format!("http://127.0.0.1:{}/mcp-stateless", discovery.port);
    let expected_core = CORE_TOOLS
        .iter()
        .map(|name| (*name).to_owned())
        .collect::<Vec<_>>();

    for client_name in [
        "claude-code",
        "codex",
        "gemini-cli",
        "opencode",
        "kimi-code",
    ] {
        let endpoint = if client_name == "kimi-code" {
            stateless.clone()
        } else {
            stateful.clone()
        };
        let transport = StreamableHttpClientTransport::from_config(
            StreamableHttpClientTransportConfig::with_uri(endpoint).auth_header(core_token.clone()),
        );
        let client = client_info(client_name).serve(transport).await?;
        assert_eq!(
            tool_names(&client).await?,
            expected_core,
            "{client_name} saw the wrong Core profile"
        );
        let _ = client.cancel().await;
    }

    let core_transport = StreamableHttpClientTransport::from_config(
        StreamableHttpClientTransportConfig::with_uri(stateful.clone())
            .auth_header(core_token.clone()),
    );
    let core_client = client_info("core-budget").serve(core_transport).await?;
    let core_tools = core_client.list_all_tools().await?;
    let core_bytes = serde_json::to_vec(&json!({ "tools": &core_tools }))?.len();
    eprintln!("Core tools/list compact JSON bytes: {core_bytes}");
    // 21,309 bytes after the todo 632 review-fix merge, retaining about 8.9% headroom.
    const CORE_TOOLS_LIST_BUDGET_BYTES: usize = 23_200;
    assert!(
        core_bytes <= CORE_TOOLS_LIST_BUDGET_BYTES,
        "Core tools/list grew beyond its size budget: {core_bytes} bytes"
    );
    let core_help = core_client
        .call_tool(
            CallToolRequestParams::new("help")
                .with_arguments(arguments(json!({ "topic": "tools" }))),
        )
        .await?;
    assert_ne!(core_help.is_error, Some(true));
    let core_help = core_help.structured_content.expect("structured tools help");
    let core_help = core_help["text"].as_str().expect("tools help text");
    assert!(core_help.contains("fixed when an agent launches"));
    assert!(core_help.contains("Core:\nwhoami —"));
    assert!(core_help.contains("Extended (requires the Extended profile):"));
    assert!(core_help.contains("spawn_terminal —"));
    assert!(!core_help.contains("User-only:"));
    assert!(!core_help.contains("agent_tool_configure —"));
    let denied = core_client
        .call_tool(
            CallToolRequestParams::new("spawn_terminal").with_arguments(arguments(json!({}))),
        )
        .await?;
    assert_eq!(denied.is_error, Some(true));
    let denied = denied.structured_content.expect("structured profile error");
    assert_eq!(denied["code"], "mcp_tools_profile_required");
    assert!(
        denied["message"]
            .as_str()
            .unwrap()
            .contains("requires the Extended")
    );
    assert!(
        denied["message"]
            .as_str()
            .unwrap()
            .contains("fixed for its lifetime")
    );
    let _ = core_client.cancel().await;

    let extended_transport = StreamableHttpClientTransport::from_config(
        StreamableHttpClientTransportConfig::with_uri(stateful.clone()).auth_header(extended_token),
    );
    let extended_client = client_info("extended").serve(extended_transport).await?;
    let mut expected_extended = CORE_TOOLS
        .iter()
        .chain(EXTENDED_TOOLS.iter())
        .map(|name| (*name).to_owned())
        .collect::<Vec<_>>();
    expected_extended.sort();
    assert_eq!(tool_names(&extended_client).await?, expected_extended);
    let _ = extended_client.cancel().await;

    let user_transport = StreamableHttpClientTransport::from_config(
        StreamableHttpClientTransportConfig::with_uri(stateful)
            .auth_header(discovery.token.clone()),
    );
    let user_client = client_info("desktop-user").serve(user_transport).await?;
    let user_tools = tool_names(&user_client).await?;
    assert_eq!(
        user_tools.len(),
        CORE_TOOLS.len() + EXTENDED_TOOLS.len() + 1
    );
    assert!(user_tools.iter().any(|name| name == "agent_tool_configure"));
    let user_help = user_client
        .call_tool(
            CallToolRequestParams::new("help")
                .with_arguments(arguments(json!({ "topic": "tools" }))),
        )
        .await?;
    assert_ne!(user_help.is_error, Some(true));
    let user_help = user_help.structured_content.expect("structured tools help");
    let user_help = user_help["text"].as_str().expect("tools help text");
    assert!(user_help.contains("Core:\nwhoami —"));
    assert!(user_help.contains("Extended (requires the Extended profile):"));
    assert!(user_help.contains("User-only:\nagent_tool_configure —"));
    let _ = user_client.cancel().await;

    {
        let mut registry = registry.lock().await;
        registry.stop(41)?;
        registry.stop(42)?;
    }
    let _ = shutdown_tx.send(());
    server_task.await??;
    Ok(())
}
