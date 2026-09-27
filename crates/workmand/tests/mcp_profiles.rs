use std::{collections::BTreeMap, error::Error};

use rmcp::{
    ServiceExt,
    model::{CallToolRequestParams, ClientInfo, Implementation},
    transport::{
        StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use serde_json::{Map, Value, json};
use workman_core::{Process, ProcessKind, ProcessSource, ProcessStatus, Project};
use workmand::{DaemonConfig, DaemonServer};

const PROCESS_TOOLS: &[&str] = &[
    "agent_tool_check",
    "clear_output",
    "close_process",
    "commands_control",
    "get_process_output",
    "get_process_status",
    "help",
    "list_agent_tools",
    "list_processes",
    "lock",
    "process_control",
    "project_update",
    "scratchpad_append",
    "scratchpad_comment_create",
    "scratchpad_comment_delete",
    "scratchpad_comment_update",
    "scratchpad_delete",
    "scratchpad_edit",
    "scratchpad_find",
    "scratchpad_list",
    "scratchpad_load_from_file",
    "scratchpad_read",
    "scratchpad_save_to_file",
    "scratchpad_update",
    "scratchpad_write",
    "search_output",
    "send_input",
    "services_list",
    "spawn_agent",
    "spawn_terminal",
    "timer_cancel",
    "timer_fire_when_idle",
    "timer_list",
    "timer_pause",
    "timer_set",
    "todo_comment_create",
    "todo_comment_delete",
    "todo_comment_update",
    "todo_complete",
    "todo_create",
    "todo_delete",
    "todo_get",
    "todo_list",
    "todo_lock",
    "todo_unlock",
    "todo_update",
    "update_process",
    "wait_for_bound_port",
    "whoami",
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

fn process(project_path: &str) -> Process {
    Process {
        id: 42,
        project_id: 1,
        kind: ProcessKind::Agent,
        name: "agent".into(),
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
async fn every_client_gets_one_process_tool_set_and_user_only_calls_are_gated()
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
    let process_token = {
        let mut registry = registry.lock().await;
        registry.store().put_project(&Project {
            id: 1,
            path: project_dir.to_string_lossy().into_owned(),
            name: "tool-set-test".into(),
            display_name: None,
            icon: None,
            selected: true,
            sort_order: 0,
        })?;
        registry
            .store()
            .put_process(&process(&project_dir.to_string_lossy()))?;
        registry.start(42)?;
        registry.store().connection().query_row(
            "SELECT token FROM process_mcp_tokens WHERE process_id = 42",
            [],
            |row| row.get::<_, String>(0),
        )?
    };

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let server_task = tokio::spawn(server.serve_until(async move {
        let _ = shutdown_rx.await;
    }));
    let endpoint = format!("http://127.0.0.1:{}/mcp-stateless", discovery.port);
    let mut expected_process = PROCESS_TOOLS
        .iter()
        .map(|name| (*name).to_owned())
        .collect::<Vec<_>>();
    expected_process.sort();
    assert_eq!(expected_process.len(), 52);

    for client_name in ["Claude Code", "Codex", "Gemini", "OpenCode", "Kimi"] {
        let transport = StreamableHttpClientTransport::from_config(
            StreamableHttpClientTransportConfig::with_uri(endpoint.clone())
                .auth_header(process_token.clone()),
        );
        let client = client_info(client_name).serve(transport).await?;
        assert_eq!(
            tool_names(&client).await?,
            expected_process,
            "{client_name} saw the wrong process tool set"
        );
        let _ = client.cancel().await;
    }

    let process_transport = StreamableHttpClientTransport::from_config(
        StreamableHttpClientTransportConfig::with_uri(endpoint.clone()).auth_header(process_token),
    );
    let process_client = client_info("process-budget")
        .serve(process_transport)
        .await?;
    let process_tools = process_client.list_all_tools().await?;
    assert_eq!(process_tools.len(), 52);
    let process_bytes = serde_json::to_vec(&json!({ "tools": &process_tools }))?.len();
    eprintln!("Process tools/list compact JSON bytes: {process_bytes}");
    const PROCESS_TOOLS_LIST_BUDGET_BYTES: usize = 29_200;
    assert!(
        process_bytes <= PROCESS_TOOLS_LIST_BUDGET_BYTES,
        "process tools/list grew beyond its size budget: {process_bytes} bytes"
    );
    let process_help = process_client
        .call_tool(
            CallToolRequestParams::new("help")
                .with_arguments(arguments(json!({ "topic": "tools" }))),
        )
        .await?
        .structured_content
        .expect("structured process tools help");
    let process_help = process_help["text"].as_str().expect("tools help text");
    assert!(process_help.contains("whoami —"));
    assert!(process_help.contains("spawn_terminal —"));
    assert!(!process_help.contains("Core"));
    assert!(!process_help.contains("Extended"));
    assert!(!process_help.contains("User-only:"));
    assert!(!process_help.contains("agent_tool_configure —"));
    let mut sorted_help_lines = process_help.lines().collect::<Vec<_>>();
    let process_help_lines = sorted_help_lines.clone();
    sorted_help_lines.sort();
    assert_eq!(process_help_lines, sorted_help_lines);

    let denied = process_client
        .call_tool(CallToolRequestParams::new("agent_tool_configure"))
        .await?;
    assert_eq!(denied.is_error, Some(true));
    let denied = denied
        .structured_content
        .expect("structured user-only gate error");
    assert_eq!(denied["code"], "user_session_required");
    assert!(
        denied["message"]
            .as_str()
            .unwrap()
            .contains("unavailable to process identities")
    );
    let _ = process_client.cancel().await;

    let user_transport = StreamableHttpClientTransport::from_config(
        StreamableHttpClientTransportConfig::with_uri(endpoint)
            .auth_header(discovery.token.clone()),
    );
    let user_client = client_info("user-bearer").serve(user_transport).await?;
    let user_tool_list = user_client.list_all_tools().await?;
    let user_bytes = serde_json::to_vec(&json!({ "tools": &user_tool_list }))?.len();
    eprintln!("Bearer tools/list compact JSON bytes: {user_bytes}");
    let mut user_tools = user_tool_list
        .into_iter()
        .map(|tool| tool.name.into_owned())
        .collect::<Vec<_>>();
    user_tools.sort();
    assert_eq!(user_tools.len(), 53);
    assert!(user_tools.iter().any(|name| name == "agent_tool_configure"));
    assert!(
        PROCESS_TOOLS
            .iter()
            .all(|name| user_tools.iter().any(|tool| tool == name))
    );
    let user_help = user_client
        .call_tool(
            CallToolRequestParams::new("help")
                .with_arguments(arguments(json!({ "topic": "tools" }))),
        )
        .await?
        .structured_content
        .expect("structured user tools help");
    let user_help = user_help["text"].as_str().expect("tools help text");
    assert!(user_help.contains("\n\nUser-only:\nagent_tool_configure —"));
    assert!(!user_help.contains("Core"));
    assert!(!user_help.contains("Extended"));
    let _ = user_client.cancel().await;

    registry.lock().await.stop(42)?;
    let _ = shutdown_tx.send(());
    server_task.await??;
    Ok(())
}
