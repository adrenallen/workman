use std::{env, error::Error, ffi::OsString, fs, os::unix::fs::PermissionsExt, path::Path};

use futures_util::{SinkExt, StreamExt};
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
use tokio::{net::TcpStream, sync::oneshot};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
    tungstenite::{Message, client::IntoClientRequest, http::header},
};

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

struct EnvGuard(Vec<(&'static str, Option<OsString>)>);

impl EnvGuard {
    fn set(values: [(&'static str, OsString); 3]) -> Self {
        let previous = values
            .iter()
            .map(|(name, _)| (*name, env::var_os(name)))
            .collect();
        for (name, value) in values {
            // SAFETY: this integration binary has one test; no sibling thread reads
            // these variables until after all three fixture values are installed.
            unsafe { env::set_var(name, value) };
        }
        Self(previous)
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (name, value) in self.0.drain(..) {
            // SAFETY: this integration binary has one test and the daemon task is
            // stopped before the guard is dropped.
            unsafe {
                match value {
                    Some(value) => env::set_var(name, value),
                    None => env::remove_var(name),
                }
            }
        }
    }
}

async fn rpc(socket: &mut Socket, id: u64, method: &str, params: Value) -> Value {
    socket
        .send(Message::Text(
            json!({ "id": id, "method": method, "params": params })
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    let Message::Text(text) = socket.next().await.unwrap().unwrap() else {
        panic!("expected JSON response");
    };
    let response: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(response["id"], id);
    response
}

fn arguments(value: Value) -> Map<String, Value> {
    value.as_object().unwrap().clone()
}

async fn mcp_call(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ClientInfo>,
    name: &'static str,
    args: Value,
) -> Value {
    let result = client
        .call_tool(CallToolRequestParams::new(name).with_arguments(arguments(args)))
        .await
        .unwrap();
    assert_ne!(result.is_error, Some(true), "{name} returned {result:?}");
    result.structured_content.unwrap()
}

fn write_runtime(path: &Path, version: &str) -> Result<(), Box<dyn Error>> {
    fs::write(path, format!("#!/bin/sh\nprintf '%s\\n' '{version}'\n"))?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

fn write_user_config(path: &Path) -> Result<(), Box<dyn Error>> {
    fs::write(
        path,
        "agent_tools:\n\
         \x20 - name: Gemini\n\
         \x20   command: gemini\n\
         \x20   tool_type: gemini\n\
         \x20   enabled: true\n\
         \x20 - name: OpenCode\n\
         \x20   command: opencode\n\
         \x20   tool_type: opencode\n\
         \x20   enabled: true\n\
         \x20 - name: Kimi\n\
         \x20   command: kimi --yolo\n\
         \x20   tool_type: kimi\n\
         \x20   enabled: true\n\
         \x20 - name: Claude\n\
         \x20   command: claude --dangerously-skip-permissions\n\
         \x20   tool_type: claude\n\
         \x20   enabled: true\n\
         \x20 - name: Codex\n\
         \x20   command: codex --dangerously-bypass-approvals-and-sandbox\n\
         \x20   tool_type: codex\n\
         \x20   enabled: true\n\
         \x20 - name: DeepSeek v4 flash\n\
         \x20   command: opencode --model deepseek/deepseek-v4-flash\n\
         \x20   tool_type: opencode\n\
         \x20   enabled: true\n",
    )?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn isolated_doctor_reports_refreshes_and_configures_without_real_user_files()
-> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let home = temp.path().join("home");
    let bin = temp.path().join("bin");
    let workspace = temp.path().join("workspace");
    let user_config = temp.path().join("gbuild-config.yml");
    fs::create_dir_all(home.join(".config/opencode"))?;
    fs::create_dir_all(&bin)?;
    fs::create_dir_all(&workspace)?;
    fs::write(
        home.join(".config/opencode/opencode.json"),
        "{\"theme\":\"fixture-night\"}\n",
    )?;
    write_user_config(&user_config)?;
    for (binary, version) in [
        ("gemini", "gemini 1.2.3-test"),
        ("opencode", "opencode 2.3.4-test"),
        ("kimi", "kimi 3.4.5-test"),
        ("claude", "claude 4.5.6-test"),
        ("codex", "codex 5.6.7-test"),
    ] {
        write_runtime(&bin.join(binary), version)?;
    }
    let _environment = EnvGuard::set([
        ("HOME", home.clone().into_os_string()),
        ("PATH", env::join_paths([&bin])?),
        ("GBUILD_CONFIG", user_config.clone().into_os_string()),
    ]);

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
            id: 77,
            path: workspace.to_string_lossy().into_owned(),
            name: "doctor-fixture".to_owned(),
            display_name: None,
            icon: None,
            selected: true,
            sort_order: 0,
        })?;
        registry.store().put_agent_tool(&AgentTool {
            id: 99,
            name: "Missing fixture".to_owned(),
            command: "definitely-missing-agent".to_owned(),
            tool_type: "custom".to_owned(),
            enabled: true,
            source: AgentToolSource::Local,
        })?;
    }

    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let server_task = tokio::spawn(server.serve_until(async move {
        let _ = shutdown_rx.await;
    }));
    let mut request = format!("ws://127.0.0.1:{}/ws", discovery.port).into_client_request()?;
    request.headers_mut().insert(
        header::AUTHORIZATION,
        format!("Bearer {}", discovery.token).parse()?,
    );
    let (mut socket, _) = connect_async(request).await?;

    let health = rpc(&mut socket, 1, "agent_tools.health", json!({})).await;
    assert!(health["ok"].as_bool().unwrap());
    assert_eq!(
        health["result"]["summary"],
        "6 of 7 runtime targets can launch"
    );
    assert_eq!(health["result"]["tools"].as_array().unwrap().len(), 7);
    let missing = health["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "Missing fixture")
        .unwrap();
    assert_eq!(missing["found_on_path"], false);
    let codex = health["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "Codex")
        .unwrap();
    assert_eq!(codex["version"], "codex 5.6.7-test");
    assert_eq!(codex["configuration_mode"], "per_launch");

    let endpoint = format!("http://127.0.0.1:{}/mcp", discovery.port);
    let transport = StreamableHttpClientTransport::from_config(
        StreamableHttpClientTransportConfig::with_uri(endpoint.clone())
            .auth_header(discovery.token.clone()),
    );
    let mcp = ClientInfo::default().serve(transport).await?;
    let mcp_health = mcp_call(&mcp, "agent_tools_health", json!({})).await;
    assert_eq!(mcp_health["summary"], "6 of 7 runtime targets can launch");
    assert!(mcp_health["tools"][0]["found_on_path"].is_boolean());
    assert!(mcp_health["tools"][0]["config_path"].is_string());

    let missing_deep_check = rpc(
        &mut socket,
        10,
        "agent_tools.deep_check",
        json!({ "project_id": 77, "agent_tool_id": 99, "timeout_ms": 1_000 }),
    )
    .await;
    assert_eq!(missing_deep_check["result"]["success"], false);
    assert_eq!(missing_deep_check["result"]["process_id"], Value::Null);

    write_runtime(
        &bin.join("definitely-missing-agent"),
        "missing-agent 0.1-test",
    )?;
    let refreshed = rpc(&mut socket, 2, "agent_tools.health", json!({})).await;
    assert_eq!(
        refreshed["result"]["summary"],
        "7 of 7 runtime targets can launch"
    );

    let opencode_id = health["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "OpenCode")
        .unwrap()["id"]
        .as_i64()
        .unwrap();
    let preview = rpc(
        &mut socket,
        3,
        "agent_tools.configure_preview",
        json!({ "agent_tool_id": opencode_id }),
    )
    .await;
    assert_eq!(preview["result"]["requires_consent"], true);
    assert!(
        preview["result"]["preview"]
            .as_str()
            .unwrap()
            .contains("fixture-night")
    );
    assert!(
        preview["result"]["preview"]
            .as_str()
            .unwrap()
            .contains("{env:GBUILD_MCP_TOKEN}")
    );
    let rejected = rpc(
        &mut socket,
        4,
        "agent_tools.configure",
        json!({
            "agent_tool_id": opencode_id,
            "confirm_write": false,
            "expected_preview_sha256": preview["result"]["preview_sha256"]
        }),
    )
    .await;
    assert_eq!(rejected["error"]["code"], "agent_config_error");
    let applied = rpc(
        &mut socket,
        5,
        "agent_tools.configure",
        json!({
            "agent_tool_id": opencode_id,
            "confirm_write": true,
            "expected_preview_sha256": preview["result"]["preview_sha256"]
        }),
    )
    .await;
    assert_eq!(applied["result"]["written"], true);
    let written = fs::read_to_string(home.join(".config/opencode/opencode.json"))?;
    assert!(written.contains("fixture-night"));
    assert!(written.contains(&endpoint));
    assert!(written.contains("{env:GBUILD_MCP_TOKEN}"));

    let toggled = rpc(
        &mut socket,
        6,
        "agent_tools.save",
        json!({
            "tool": {
                "id": opencode_id,
                "name": "OpenCode",
                "command": "opencode",
                "tool_type": "opencode",
                "enabled": false
            }
        }),
    )
    .await;
    assert_eq!(toggled["result"]["enabled"], false);
    assert!(fs::read_to_string(&user_config)?.contains("enabled: false"));

    let _ = mcp.cancel().await;
    socket.close(None).await?;
    let _ = shutdown_tx.send(());
    server_task.await??;
    Ok(())
}
