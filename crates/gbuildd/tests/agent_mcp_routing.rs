use std::{error::Error, path::Path, time::Duration};

use gbuild_core::{AgentTool, AgentToolSource, Project};
use gbuildd::{DaemonConfig, DaemonServer};
use rmcp::{
    ServiceExt,
    model::{CallToolRequestParams, ClientInfo},
    transport::{
        StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use serde_json::{Map, Value, json};

fn arguments(value: Value) -> Map<String, Value> {
    value.as_object().unwrap().clone()
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
    let structured = result
        .structured_content
        .as_ref()
        .unwrap_or_else(|| panic!("{name} returned no structured content"));
    assert!(
        structured.is_object(),
        "{name} returned non-object structured content: {structured}"
    );
    let text = result
        .content
        .iter()
        .find_map(|content| content.as_text())
        .unwrap_or_else(|| panic!("{name} returned no text content"));
    assert_eq!(
        serde_json::from_str::<Value>(&text.text).unwrap(),
        *structured,
        "{name} text content diverged from structured content"
    );
    structured.clone()
}

async fn put_real_agent_tools(server: &DaemonServer) -> Result<(), Box<dyn Error>> {
    let registry = server.registry();
    let registry = registry.lock().await;
    for tool in [
        AgentTool {
            id: 101,
            name: "Real Claude".into(),
            command: "claude --dangerously-skip-permissions".into(),
            tool_type: "claude_code".into(),
            enabled: true,
            source: AgentToolSource::Local,
        },
        AgentTool {
            id: 102,
            name: "Real Codex".into(),
            command: "codex --dangerously-bypass-approvals-and-sandbox".into(),
            tool_type: "codex".into(),
            enabled: true,
            source: AgentToolSource::Local,
        },
    ] {
        registry.store().put_agent_tool(&tool)?;
    }
    Ok(())
}

/// This deliberately exercises the installed, authenticated agent CLIs and is kept ignored for
/// routine/CI runs. Run it explicitly when changing per-launch connector wiring.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires authenticated Claude and Codex CLIs"]
async fn real_claude_and_codex_accept_list_envelopes_and_route_to_spawning_daemon()
-> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let project_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?;
    let server = DaemonServer::bind(DaemonConfig {
        data_dir: temp.path().join("state"),
        port: 0,
    })
    .await?;
    {
        let registry = server.registry();
        let registry = registry.lock().await;
        registry.store().put_project(&Project {
            id: 77,
            path: project_path.to_string_lossy().into_owned(),
            name: "routing-e2e".into(),
            display_name: None,
            icon: None,
            selected: false,
            sort_order: 0,
        })?;
    }
    put_real_agent_tools(&server).await?;

    let discovery = server.discovery().clone();
    let registry = server.registry();
    let endpoint = format!("http://127.0.0.1:{}/mcp", discovery.port);
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let server_task = tokio::spawn(server.serve_until(async move {
        let _ = shutdown_rx.await;
    }));
    let transport = StreamableHttpClientTransport::from_config(
        StreamableHttpClientTransportConfig::with_uri(endpoint).auth_header(discovery.token),
    );
    let parent = ClientInfo::default().serve(transport).await?;
    let prompt = "Use only the MCP server named gbuild. Call whoami, list_agent_tools, and list_processes. Confirm list_agent_tools has an agent_tools array and list_processes has a processes array. After all three calls succeed, print exactly ROUTING_OK and nothing else. Do not use solo or any other MCP server.";

    let claude = call(
        &parent,
        "spawn_agent",
        json!({
            "project_id": 77,
            "agent_tool_id": 101,
            "name": "routing-claude",
            "extra_args": ["--print", "--output-format", "text", prompt]
        }),
    )
    .await;
    let claude_id = claude["process_id"].as_i64().unwrap();

    let codex = call(
        &parent,
        "spawn_agent",
        json!({
            "project_id": 77,
            "agent_tool_id": 102,
            "name": "routing-codex",
            "extra_args": ["exec", "--skip-git-repo-check", prompt]
        }),
    )
    .await;
    let codex_id = codex["process_id"].as_i64().unwrap();

    for process_id in [claude_id, codex_id] {
        for _ in 0..960 {
            let (actor_count, output) = {
                let mut registry = registry.lock().await;
                let _ = registry.get_status(process_id);
                let actor_count = registry.store().connection().query_row(
                    "SELECT COUNT(*) FROM actors WHERE process_id = ?1",
                    [process_id],
                    |row| row.get::<_, i64>(0),
                )?;
                let output = registry.rendered_output(process_id)?.text;
                (actor_count, output)
            };
            if actor_count > 0 && output.contains("ROUTING_OK") {
                break;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        let mut registry = registry.lock().await;
        let actor_count = registry.store().connection().query_row(
            "SELECT COUNT(*) FROM actors WHERE process_id = ?1",
            [process_id],
            |row| row.get::<_, i64>(0),
        )?;
        assert!(actor_count > 0, "isolated daemon did not identify process");
        assert!(
            registry
                .rendered_output(process_id)?
                .text
                .contains("ROUTING_OK"),
            "agent did not report successful isolated whoami"
        );
        drop(registry);
        let closed = call(
            &parent,
            "close_process",
            json!({ "project_id": 77, "process_id": process_id }),
        )
        .await;
        assert_eq!(closed["closed"], true);
    }

    let _ = parent.cancel().await;
    let _ = shutdown_tx.send(());
    server_task.await??;
    Ok(())
}
