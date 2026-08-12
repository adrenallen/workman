use std::{collections::HashMap, error::Error, process::Stdio, time::Duration};

use futures_util::{SinkExt, StreamExt};
use rmcp::{
    ServiceExt,
    model::{CallToolRequestParams, ClientInfo},
    transport::{
        StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use serde_json::{Map, Value, json};
use tokio::process::{Child, Command};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{Message, client::IntoClientRequest, http::header},
};
use workmand::{Discovery, discovery_path};

const SIX_AGENTS: &str = r#"agent_tools:
  - name: Gemini
    command: gemini --approval-mode=yolo
    tool_type: gemini
  - name: OpenCode
    command: opencode --auto
    tool_type: opencode
  - name: Kimi
    command: kimi --yolo
    tool_type: kimi
  - name: Claude
    command: claude --dangerously-skip-permissions
    tool_type: claude
  - name: Codex
    command: codex --dangerously-bypass-approvals-and-sandbox
    tool_type: codex
  - name: DeepSeek v4 flash
    command: opencode --auto --model deepseek/deepseek-v4-flash
    tool_type: opencode
"#;

fn spawn_daemon(
    data_dir: &std::path::Path,
    config_path: &std::path::Path,
) -> Result<Child, Box<dyn Error>> {
    Ok(Command::new(env!("CARGO_BIN_EXE_workmand"))
        .env("WORKMAN_DATA_DIR", data_dir)
        .env("WORKMAN_CONFIG", config_path)
        .arg("--port")
        .arg("0")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?)
}

async fn wait_for_discovery(
    data_dir: &std::path::Path,
    expected_pid: u32,
) -> Result<Discovery, Box<dyn Error>> {
    for _ in 0..250 {
        if let Ok(discovery) = Discovery::read(data_dir)
            && discovery.pid == expected_pid
        {
            return Ok(discovery);
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    Err("isolated daemon did not publish discovery metadata".into())
}

async fn stop_daemon(mut child: Child, data_dir: &std::path::Path) -> Result<(), Box<dyn Error>> {
    let pid = child.id().ok_or("daemon child had no pid")?;
    let status = std::process::Command::new("kill")
        .arg("-TERM")
        .arg(pid.to_string())
        .status()?;
    if !status.success() {
        return Err(format!("failed to terminate isolated daemon {pid}").into());
    }
    tokio::time::timeout(Duration::from_secs(3), child.wait()).await??;
    for _ in 0..100 {
        if !discovery_path(data_dir).exists() {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    Err("isolated daemon did not remove discovery metadata".into())
}

fn arguments(value: Value) -> Map<String, Value> {
    value.as_object().unwrap().clone()
}

async fn list_agent_tools(discovery: &Discovery) -> Result<Vec<Value>, Box<dyn Error>> {
    let transport = StreamableHttpClientTransport::from_config(
        StreamableHttpClientTransportConfig::with_uri(format!(
            "http://127.0.0.1:{}/mcp",
            discovery.port
        ))
        .auth_header(discovery.token.clone()),
    );
    let client = ClientInfo::default().serve(transport).await?;
    let result = client
        .call_tool(
            CallToolRequestParams::new("list_agent_tools").with_arguments(arguments(json!({}))),
        )
        .await?;
    assert_ne!(result.is_error, Some(true));
    let structured = result
        .structured_content
        .as_ref()
        .ok_or("list_agent_tools returned no structured content")?;
    let text = result
        .content
        .iter()
        .find_map(|content| content.as_text())
        .ok_or("list_agent_tools returned no text content")?;
    assert_eq!(serde_json::from_str::<Value>(&text.text)?, *structured);
    let tools = structured
        .get("agent_tools")
        .ok_or("list_agent_tools response omitted agent_tools")?
        .as_array()
        .ok_or("list_agent_tools agent_tools was not an array")?
        .clone();
    let _ = client.cancel().await;
    Ok(tools)
}

async fn control_call(
    discovery: &Discovery,
    id: i64,
    method: &str,
    params: Value,
) -> Result<Value, Box<dyn Error>> {
    let mut request = format!("ws://127.0.0.1:{}/ws", discovery.port).into_client_request()?;
    request.headers_mut().insert(
        header::AUTHORIZATION,
        format!("Bearer {}", discovery.token).parse()?,
    );
    let (mut socket, _) = connect_async(request).await?;
    socket
        .send(Message::Text(
            json!({
                "id": id,
                "method": method,
                "params": params,
            })
            .to_string()
            .into(),
        ))
        .await?;
    let response = socket.next().await.ok_or("missing control response")??;
    let Message::Text(response) = response else {
        return Err("control response was not JSON text".into());
    };
    let response: Value = serde_json::from_str(&response)?;
    assert_eq!(response["ok"], true, "{response}");
    socket.close(None).await?;
    Ok(response["result"].clone())
}

#[tokio::test]
async fn settings_mutations_persist_to_active_profile_and_survive_isolated_daemon_restart()
-> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let data_dir = temp.path().join("state");
    let config_path = temp.path().join("config.yml");
    std::fs::write(&config_path, SIX_AGENTS)?;

    let child = spawn_daemon(&data_dir, &config_path)?;
    let discovery = wait_for_discovery(&data_dir, child.id().unwrap()).await?;
    let tools = list_agent_tools(&discovery).await?;
    assert_eq!(tools.len(), 6);
    let actual = tools
        .iter()
        .map(|tool| {
            (
                tool["name"].as_str().unwrap().to_owned(),
                (
                    tool["command"].as_str().unwrap().to_owned(),
                    tool["tool_type"].as_str().unwrap().to_owned(),
                    tool["source"].as_str().unwrap().to_owned(),
                ),
            )
        })
        .collect::<HashMap<_, _>>();
    for (name, command, tool_type) in [
        ("Gemini", "gemini --approval-mode=yolo", "gemini"),
        ("OpenCode", "opencode --auto", "opencode"),
        ("Kimi", "kimi --yolo", "kimi"),
        ("Claude", "claude --dangerously-skip-permissions", "claude"),
        (
            "Codex",
            "codex --dangerously-bypass-approvals-and-sandbox",
            "codex",
        ),
        (
            "DeepSeek v4 flash",
            "opencode --auto --model deepseek/deepseek-v4-flash",
            "opencode",
        ),
    ] {
        assert_eq!(
            actual.get(name),
            Some(&(
                command.to_owned(),
                tool_type.to_owned(),
                "config".to_owned()
            ))
        );
    }
    let custom = control_call(
        &discovery,
        1,
        "agent_tools.save",
        json!({
            "tool": {
                "name": "UI custom",
                "command": "custom-agent --interactive",
                "tool_type": "future_unknown_type",
                "enabled": true
            }
        }),
    )
    .await?;
    assert_eq!(custom["source"], "config");
    let codex = tools.iter().find(|tool| tool["name"] == "Codex").unwrap();
    let edited = control_call(
        &discovery,
        2,
        "agent_tools.save",
        json!({
            "tool": {
                "id": codex["id"],
                "name": "Codex QA",
                "command": "codex --model qa-persisted",
                "tool_type": "codex",
                "enabled": true
            }
        }),
    )
    .await?;
    assert_eq!(edited["name"], "Codex QA");
    let tools = list_agent_tools(&discovery).await?;
    let reversed_ids = tools
        .iter()
        .rev()
        .map(|tool| tool["id"].as_i64().unwrap())
        .collect::<Vec<_>>();
    let reordered = control_call(
        &discovery,
        3,
        "agent_tools.reorder",
        json!({ "agent_tool_ids": reversed_ids }),
    )
    .await?;
    assert_eq!(reordered[0]["name"], "UI custom");
    let kimi_id = tools.iter().find(|tool| tool["name"] == "Kimi").unwrap()["id"]
        .as_i64()
        .unwrap();
    let deleted = control_call(
        &discovery,
        4,
        "agent_tools.delete",
        json!({ "agent_tool_id": kimi_id }),
    )
    .await?;
    assert_eq!(deleted["deleted"], true);
    stop_daemon(child, &data_dir).await?;

    // YAML is a one-time migration source. Settings writes belong to the
    // active profile and must not rewrite the legacy/global config file.
    assert_eq!(std::fs::read_to_string(&config_path)?, SIX_AGENTS);

    let child = spawn_daemon(&data_dir, &config_path)?;
    let discovery = wait_for_discovery(&data_dir, child.id().unwrap()).await?;
    let tools = list_agent_tools(&discovery).await?;
    assert_eq!(tools.len(), 6);
    assert!(!tools.iter().any(|tool| tool["name"] == "Kimi"));
    assert_eq!(tools[0]["name"], "UI custom");
    let custom = tools
        .iter()
        .find(|tool| tool["name"] == "UI custom")
        .unwrap();
    assert_eq!(custom["command"], "custom-agent --interactive");
    assert_eq!(custom["tool_type"], "future_unknown_type");
    assert_eq!(custom["source"], "config");
    let codex = tools
        .iter()
        .find(|tool| tool["name"] == "Codex QA")
        .unwrap();
    assert_eq!(codex["command"], "codex --model qa-persisted");
    stop_daemon(child, &data_dir).await?;
    Ok(())
}
