use std::{error::Error, path::Path, time::Duration};

use rmcp::{
    ServiceExt,
    model::{CallToolRequestParams, ClientInfo},
    transport::{
        StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use serde_json::{Map, Value, json};
use workman_core::{AgentTool, AgentToolSource, ProcessStatus, Project};
use workmand::{DaemonConfig, DaemonServer};

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
            name: "Real Gemini".into(),
            command: "gemini".into(),
            tool_type: "gemini".into(),
            enabled: true,
            source: AgentToolSource::Local,
        },
        AgentTool {
            id: 102,
            name: "Real OpenCode".into(),
            command: "opencode".into(),
            tool_type: "opencode".into(),
            enabled: true,
            source: AgentToolSource::Local,
        },
        AgentTool {
            id: 103,
            name: "Real Kimi".into(),
            command: "kimi --yolo".into(),
            tool_type: "kimi".into(),
            enabled: true,
            source: AgentToolSource::Local,
        },
        AgentTool {
            id: 104,
            name: "Real Claude".into(),
            command: "claude --dangerously-skip-permissions".into(),
            tool_type: "claude_code".into(),
            enabled: true,
            source: AgentToolSource::Local,
        },
        AgentTool {
            id: 105,
            name: "Real Codex".into(),
            command: "codex --dangerously-bypass-approvals-and-sandbox".into(),
            tool_type: "codex".into(),
            enabled: true,
            source: AgentToolSource::Local,
        },
        AgentTool {
            id: 106,
            name: "Real DeepSeek v4 flash".into(),
            command: "opencode --model deepseek/deepseek-v4-flash".into(),
            tool_type: "opencode".into(),
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
#[ignore = "requires all six authenticated configured agent CLIs"]
async fn configured_agent_tools_route_to_an_isolated_spawning_daemon() -> Result<(), Box<dyn Error>>
{
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
    let prompt = "Use only the MCP server named workman. Call whoami, list_agent_tools, and list_processes. Confirm list_agent_tools has an agent_tools array and list_processes has a processes array. After all three calls succeed, print exactly ROUTING_OK and nothing else. Do not use solo or any other MCP server.";

    let launches = [
        (
            101,
            "routing-gemini",
            json!(["--skip-trust", "--prompt", prompt]),
            Some("IneligibleTierError"),
        ),
        (
            102,
            "routing-opencode",
            json!(["--model", "openai/gpt-5.6-terra", "run", prompt]),
            None,
        ),
        (
            104,
            "routing-claude",
            json!(["--print", "--output-format", "text", prompt]),
            None,
        ),
        (
            105,
            "routing-codex",
            json!(["exec", "--skip-git-repo-check", prompt]),
            None,
        ),
        (
            106,
            "routing-deepseek",
            json!(["run", prompt]),
            Some("Insufficient Balance"),
        ),
    ];
    let mut processes = Vec::new();
    for (agent_tool_id, name, extra_args, expected_external_block) in launches {
        let spawned = call(
            &parent,
            "spawn_agent",
            json!({
                "project_id": 77,
                "agent_tool_id": agent_tool_id,
                "name": name,
                "extra_args": extra_args,
            }),
        )
        .await;
        processes.push((
            name,
            spawned["process_id"].as_i64().unwrap(),
            expected_external_block,
        ));
    }

    let mut failures = Vec::new();
    for (name, process_id, expected_external_block) in processes {
        for _ in 0..960 {
            let (status, actor_count, output) = {
                let mut registry = registry.lock().await;
                let status = registry.get_status(process_id)?.process.status;
                let actor_count = registry.store().connection().query_row(
                    "SELECT COUNT(*) FROM actors WHERE process_id = ?1",
                    [process_id],
                    |row| row.get::<_, i64>(0),
                )?;
                let output = registry.rendered_output(process_id)?.text;
                (status, actor_count, output)
            };
            if (actor_count > 0 && output.contains("ROUTING_OK"))
                || matches!(status, ProcessStatus::Exited | ProcessStatus::Crashed)
            {
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
        let routed = registry
            .rendered_output(process_id)?
            .text
            .contains("ROUTING_OK");
        let output = registry.rendered_output(process_id)?.text;
        match expected_external_block {
            Some(expected) if output.contains(expected) => {
                eprintln!("{name}: launch reached external account/provider block: {expected}");
            }
            Some(expected) => failures.push(format!(
                "{name}: expected external block {expected:?}, got {output:?}"
            )),
            None => {
                if actor_count == 0 {
                    failures.push(format!("{name}: isolated daemon did not identify process"));
                }
                if !routed {
                    failures.push(format!(
                        "{name}: agent did not report successful isolated MCP calls: {output:?}"
                    ));
                }
            }
        }
        drop(registry);
        let closed = call(
            &parent,
            "close_process",
            json!({ "project_id": 77, "process_id": process_id }),
        )
        .await;
        if closed["closed"] != true {
            failures.push(format!("{name}: failed to close launched process"));
        }
        eprintln!("{name}: closed managed process");
    }

    let kimi = call(
        &parent,
        "agent_tool_deep_check",
        json!({ "project_id": 77, "agent_tool_id": 103, "timeout_ms": 1000 }),
    )
    .await;
    if kimi["success"] != false
        || kimi["process_id"] != Value::Null
        || !kimi["message"].as_str().is_some_and(|message| {
            message.contains("no documented safe per-launch MCP config override")
        })
    {
        failures.push("routing-kimi: unsupported capability check was not explicit".to_owned());
    }
    eprintln!("routing-kimi: reported unsupported without launching a process");

    let _ = parent.cancel().await;
    let _ = shutdown_tx.send(());
    server_task.await??;
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    Ok(())
}
