// Drives Unix fixtures (shebang scripts, permission bits, symlinks); Windows
// fixture parity is tracked as follow-up work.
#![cfg(unix)]

use std::{
    env, error::Error, ffi::OsString, os::unix::fs::PermissionsExt, path::Path, time::Duration,
};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use futures_util::{SinkExt, StreamExt};
use image::{Rgba, RgbaImage};
use rmcp::{
    ServiceExt,
    model::{CallToolRequestParams, ClientInfo},
    transport::{
        StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use serde_json::{Value, json};
use tokio::{net::TcpStream, sync::oneshot, time::Instant};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
    tungstenite::{Message, client::IntoClientRequest, http::header},
};
use workman_core::Project;
use workmand::{DaemonConfig, DaemonServer};

struct ConfigGuard(Option<OsString>);

impl ConfigGuard {
    fn set(path: &Path) -> Self {
        let previous = env::var_os("WORKMAN_CONFIG");
        // SAFETY: this integration binary contains one test, and the variable is
        // restored only after its in-process daemon has stopped.
        unsafe { env::set_var("WORKMAN_CONFIG", path) };
        Self(previous)
    }
}

impl Drop for ConfigGuard {
    fn drop(&mut self) {
        // SAFETY: no sibling test in this integration process reads this value.
        unsafe {
            match self.0.take() {
                Some(value) => env::set_var("WORKMAN_CONFIG", value),
                None => env::remove_var("WORKMAN_CONFIG"),
            }
        }
    }
}

async fn rpc(
    socket: &mut WebSocketStream<MaybeTlsStream<TcpStream>>,
    id: u64,
    method: &str,
    params: Value,
) -> Value {
    socket
        .send(Message::Text(
            json!({ "id": id, "method": method, "params": params })
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    let Message::Text(text) = socket.next().await.unwrap().unwrap() else {
        panic!("expected a JSON control response");
    };
    let response: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(response["id"], id);
    response
}

fn write_fake_agent(path: &Path) -> Result<(), Box<dyn Error>> {
    std::fs::write(
        path,
        r##"#!/usr/bin/perl
use strict;
use warnings;
$| = 1;
system('stty', 'raw', '-echo');
print "agent-args:" . join('|', @ARGV) . "\r\n";
select undef, undef, undef, 0.25;
print "\033[?2004hagent-ready\r\n\$";
my $prompt = '';
while (1) {
    my $chunk = '';
    my $count = sysread(STDIN, $chunk, 4096);
    exit 3 unless defined($count) && $count > 0;
    for my $character (split //, $chunk) {
        if ($character eq "\r") {
            my $bracketed = $prompt =~ /^\e\[200~.*\e\[201~$/s;
            $prompt =~ s/^\e\[200~//;
            $prompt =~ s/\e\[201~$//;
            print "\r\nagent-paste:" . ($bracketed ? "bracketed" : "raw") . "\r\n";
            print "\r\nagent-answer:$prompt\r\n";
            sleep 30;
            exit 0;
        }
        $prompt .= $character;
    }
}
"##,
    )?;
    let mut permissions = std::fs::metadata(path)?.permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(path, permissions)?;
    Ok(())
}

fn write_exiting_agent(path: &Path) -> Result<(), Box<dyn Error>> {
    std::fs::write(path, "#!/bin/sh\nexit 0\n")?;
    let mut permissions = std::fs::metadata(path)?.permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(path, permissions)?;
    Ok(())
}

#[tokio::test]
async fn websocket_manages_tools_spawns_agents_and_submits_prompts() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let _config = ConfigGuard::set(&temp.path().join("config.yml"));
    let project_dir = temp.path().join("workspace");
    std::fs::create_dir(&project_dir)?;
    let fake_agent = temp.path().join("fake-agent.sh");
    write_fake_agent(&fake_agent)?;
    let exiting_agent = temp.path().join("exiting-agent.sh");
    write_exiting_agent(&exiting_agent)?;
    let data_dir = temp.path().join("state");

    let server = DaemonServer::bind(DaemonConfig {
        data_dir: data_dir.clone(),
        port: 0,
    })
    .await?;
    let discovery = server.discovery().clone();
    let registry = server.registry();
    registry.lock().await.store().put_project(&Project {
        id: 7,
        path: project_dir.to_string_lossy().into_owned(),
        name: "workspace".into(),
        display_name: None,
        icon: None,
        selected: true,
        sort_order: 0,
    })?;

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

    let presets = rpc(&mut socket, 1, "agent_tools.list", json!({})).await;
    assert!(presets["ok"].as_bool().unwrap());
    assert!(presets["result"].as_array().unwrap().len() >= 4);

    let created = rpc(
        &mut socket,
        2,
        "agent_tools.save",
        json!({
            "tool": {
                "name": "Scripted agent",
                "command": fake_agent,
                "tool_type": "scripted",
                "enabled": true
            }
        }),
    )
    .await;
    assert!(created["ok"].as_bool().unwrap());
    let tool_id = created["result"]["id"].as_i64().unwrap();

    let override_tool = rpc(
        &mut socket,
        39,
        "agent_tools.save",
        json!({
            "tool": {
                "name": "Override agent",
                "command": fake_agent,
                "tool_type": "scripted",
                "enabled": true
            }
        }),
    )
    .await;
    let override_tool_id = override_tool["result"]["id"].as_i64().unwrap();

    let exiting_tool = rpc(
        &mut socket,
        30,
        "agent_tools.save",
        json!({
            "tool": {
                "name": "Exiting agent",
                "command": exiting_agent,
                "tool_type": "custom",
                "enabled": true
            }
        }),
    )
    .await;
    let exiting_tool_id = exiting_tool["result"]["id"].as_i64().unwrap();
    let exiting_spawn = rpc(
        &mut socket,
        31,
        "agents.spawn",
        json!({
            "project_id": 7,
            "agent_tool_id": exiting_tool_id,
            "prompt": "This prompt must be dropped."
        }),
    )
    .await;
    let exiting_process_id = exiting_spawn["result"]["process_id"].as_i64().unwrap();
    let exit_deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let status = registry.lock().await.get_status(exiting_process_id)?;
        if status.events.iter().any(|event| {
            event.kind == "initial_prompt_dropped" && event.message.contains("reason: exited")
        }) {
            break;
        }
        assert!(
            Instant::now() < exit_deadline,
            "exited agent did not publish an initial-prompt drop event: {:?}",
            status.events
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    let template_prompt = format!(
        "TEMPLATE-BEGIN\n{}TEMPLATE-END",
        "Keep this orchestrator instruction intact across terminal reads.\n".repeat(160)
    );
    let template = rpc(
        &mut socket,
        23,
        "agent_templates.save",
        json!({
            "template": {
                "name": "Review worker",
                "agent_tool_id": tool_id,
                "extra_args": ["--template", "alpha value"],
                "prompt": template_prompt.clone()
            }
        }),
    )
    .await;
    assert!(template["ok"].as_bool().unwrap());
    let template_id = template["result"]["id"].as_i64().unwrap();

    for (id, template, expected) in [
        (
            34,
            json!({
                "name": "x".repeat(121),
                "agent_tool_id": tool_id
            }),
            "agent template name must be 120 characters or fewer",
        ),
        (
            35,
            json!({
                "name": "Large prompt",
                "agent_tool_id": tool_id,
                "prompt": "x".repeat(64 * 1024 + 1)
            }),
            "agent template prompt must be 65536 bytes or fewer",
        ),
        (
            36,
            json!({
                "name": "Many arguments",
                "agent_tool_id": tool_id,
                "extra_args": vec![""; 65]
            }),
            "agent template may have at most 64 arguments",
        ),
        (
            37,
            json!({
                "name": "Large arguments",
                "agent_tool_id": tool_id,
                "extra_args": ["x".repeat(4 * 1024 + 1)]
            }),
            "agent template arguments must total 4096 bytes or fewer",
        ),
    ] {
        let rejected = rpc(
            &mut socket,
            id,
            "agent_templates.save",
            json!({ "template": template }),
        )
        .await;
        assert_eq!(rejected["error"]["code"], "agent_template_error");
        assert_eq!(rejected["error"]["message"], expected);
    }
    let templates = rpc(&mut socket, 24, "agent_templates.list", json!({})).await;
    assert_eq!(templates["result"].as_array().unwrap().len(), 1);
    assert_eq!(templates["result"][0]["name"], "Review worker");
    let reordered = rpc(
        &mut socket,
        25,
        "agent_templates.reorder",
        json!({ "agent_template_ids": [template_id] }),
    )
    .await;
    assert_eq!(reordered["result"][0]["id"], template_id);

    let model_tool = rpc(
        &mut socket,
        70,
        "agent_tools.save",
        json!({
            "tool": {
                "name": "Model test agent",
                "command": "true --model command/provider-model",
                "tool_type": "opencode",
                "enabled": true
            }
        }),
    )
    .await;
    let model_tool_id = model_tool["result"]["id"].as_i64().unwrap();
    let model_template = rpc(
        &mut socket,
        71,
        "agent_templates.save",
        json!({
            "template": {
                "name": "Model reviewer",
                "agent_tool_id": model_tool_id,
                "extra_args": ["--model", "default/provider-model", "--review"],
                "prompt": "Review with the selected model."
            }
        }),
    )
    .await;
    let model_template_id = model_template["result"]["id"].as_i64().unwrap();
    let model_spawn = rpc(
        &mut socket,
        72,
        "agents.spawn",
        json!({
            "project_id": 7,
            "agent_template_id": model_template_id,
            "model": "override/provider-model",
            "name": "model-override-worker"
        }),
    )
    .await;
    assert!(model_spawn["ok"].as_bool().unwrap());
    let model_process_id = model_spawn["result"]["process_id"].as_i64().unwrap();
    let model_command = registry
        .lock()
        .await
        .get_status(model_process_id)?
        .process
        .command
        .unwrap();
    assert_eq!(model_command.matches("--model").count(), 1);
    assert!(model_command.contains("--model override/provider-model"));
    assert!(!model_command.contains("command/provider-model"));
    assert!(!model_command.contains("default/provider-model"));

    let clipboard_dir = temp.path().join("Application Support/terminal-clipboard");
    std::fs::create_dir_all(&clipboard_dir)?;
    let source_icon = clipboard_dir.join("paste-123.png");
    RgbaImage::from_pixel(96, 32, Rgba([19, 42, 77, 255])).save(&source_icon)?;
    let escaped_source_icon = source_icon.to_string_lossy().replace(' ', "\\ ");
    let icon = rpc(
        &mut socket,
        20,
        "agent_tools.set_icon",
        json!({ "agent_tool_id": tool_id, "source_path": escaped_source_icon }),
    )
    .await;
    assert!(icon["ok"].as_bool().unwrap());
    assert!(
        icon["result"]["icon_data_url"]
            .as_str()
            .unwrap()
            .starts_with("data:image/png;base64,")
    );
    assert!(
        data_dir
            .join(format!("agent-icons/{tool_id}.png"))
            .is_file()
    );
    let listed = rpc(&mut socket, 21, "agent_tools.list", json!({})).await;
    assert!(
        listed["result"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["id"] == tool_id)
            .unwrap()["icon_data_url"]
            .is_string()
    );
    let removed_icon = rpc(
        &mut socket,
        22,
        "agent_tools.remove_icon",
        json!({ "agent_tool_id": tool_id }),
    )
    .await;
    assert!(removed_icon["result"]["icon_data_url"].is_null());
    assert!(!data_dir.join(format!("agent-icons/{tool_id}.png")).exists());

    let disabled = rpc(
        &mut socket,
        3,
        "agent_tools.save",
        json!({
            "tool": {
                "id": tool_id,
                "name": "Scripted agent",
                "command": fake_agent,
                "tool_type": "scripted",
                "enabled": false
            }
        }),
    )
    .await;
    assert_eq!(disabled["result"]["enabled"], false);
    let rejected = rpc(
        &mut socket,
        4,
        "agents.spawn",
        json!({ "project_id": 7, "agent_tool_id": tool_id, "extra_args": [] }),
    )
    .await;
    assert_eq!(rejected["error"]["code"], "spawn_failed");

    let enabled = rpc(
        &mut socket,
        5,
        "agent_tools.save",
        json!({
            "tool": {
                "id": tool_id,
                "name": "Scripted agent",
                "command": fake_agent,
                "tool_type": "scripted",
                "enabled": true
            }
        }),
    )
    .await;
    assert_eq!(enabled["result"]["enabled"], true);

    let oversized_prompt = rpc(
        &mut socket,
        38,
        "agents.spawn",
        json!({
            "project_id": 7,
            "agent_tool_id": tool_id,
            "prompt": "x".repeat(64 * 1024 + 1)
        }),
    )
    .await;
    assert_eq!(oversized_prompt["error"]["code"], "invalid_params");
    assert_eq!(
        oversized_prompt["error"]["message"],
        "initial prompt must be 65536 bytes or fewer"
    );

    let spawned = rpc(
        &mut socket,
        6,
        "agents.spawn",
        json!({
            "project_id": 7,
            "agent_tool_id": tool_id,
            "name": "ui-worker",
            "extra_args": []
        }),
    )
    .await;
    assert!(spawned["ok"].as_bool().unwrap());
    assert_eq!(spawned["result"]["name"], "ui-worker");
    assert_eq!(spawned["result"]["kind"], "agent");
    assert!(
        spawned["result"]["agent_instructions"]
            .as_str()
            .unwrap()
            .contains("Workman MCP identity check is unavailable")
    );
    assert!(
        spawned["result"]["agent_instructions"]
            .as_str()
            .unwrap()
            .contains(&format!(
                "WORKMAN_MCP_URL=http://127.0.0.1:{}/mcp",
                discovery.port
            ))
    );
    let process_id = spawned["result"]["process_id"].as_i64().unwrap();
    let process_token = registry.lock().await.store().connection().query_row(
        "SELECT token FROM process_mcp_tokens WHERE process_id = ?1",
        [process_id],
        |row| row.get::<_, String>(0),
    )?;
    let mcp_transport = StreamableHttpClientTransport::from_config(
        StreamableHttpClientTransportConfig::with_uri(format!(
            "http://127.0.0.1:{}/mcp",
            discovery.port
        ))
        .auth_header(process_token),
    );
    let mcp_client = ClientInfo::default().serve(mcp_transport).await?;
    let identity = mcp_client
        .call_tool(CallToolRequestParams::new("whoami").with_arguments(Default::default()))
        .await?;
    assert_ne!(identity.is_error, Some(true), "whoami failed: {identity:?}");
    assert_eq!(
        identity.structured_content.unwrap()["process_id"],
        process_id,
        "an agent spawned through the control API must bind to its own process credential"
    );
    let _ = mcp_client.cancel().await;

    let template_spawn = rpc(
        &mut socket,
        26,
        "agents.spawn",
        json!({
            "project_id": 7,
            "agent_template_id": template_id,
            "agent_tool_id": tool_id,
            "name": "template-worker",
            "extra_args": ["--caller", "beta"],
            "prompt": "Review line one.\nReview line two.",
            "attachments": [source_icon]
        }),
    )
    .await;
    assert!(template_spawn["ok"].as_bool().unwrap());
    let template_process_id = template_spawn["result"]["process_id"].as_i64().unwrap();
    let saved_attachment = data_dir.join(format!("agent-attachments/{template_process_id}/01.png"));
    assert!(saved_attachment.is_file());
    let expected_template_prompt = format!(
        "agent-answer:{template_prompt}\n\nReview line one.\nReview line two.\n\nAttached image files were saved locally at these paths:\n- {}",
        saved_attachment.display()
    );
    let template_deadline = Instant::now() + Duration::from_secs(8);
    loop {
        let raw = registry
            .lock()
            .await
            .raw_output(template_process_id, None, usize::MAX)?;
        let output = String::from_utf8_lossy(&raw.data);
        if output.contains(&expected_template_prompt) {
            assert_eq!(output.matches("agent-answer:").count(), 1);
            assert!(output.contains("agent-paste:bracketed"));
            assert!(output.contains("agent-args:--template|alpha value|--caller|beta"));
            assert!(
                output.find("agent-ready").unwrap() < output.find("agent-answer:").unwrap(),
                "initial prompt arrived before the delayed readiness marker: {output}"
            );
            let status = registry.lock().await.get_status(template_process_id)?;
            assert!(
                status.agent_state.last_output_at.is_some(),
                "readiness delivery must be backed by observed tracker output"
            );
            assert!(status.events.iter().any(|event| {
                event.kind == "initial_prompt_delivered"
                    && event.message.contains("initial prompt delivered")
            }));
            break;
        }
        assert!(
            Instant::now() < template_deadline,
            "template prompt was not delivered after readiness: {}",
            output
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    let overridden = rpc(
        &mut socket,
        27,
        "agents.spawn",
        json!({
            "project_id": 7,
            "agent_template_id": template_id,
            "agent_tool_id": override_tool_id,
            "name": "override-worker",
            "extra_args": ["--caller", "override"],
            "prompt": "Use the selected agent."
        }),
    )
    .await;
    assert!(overridden["ok"].as_bool().unwrap());
    let override_process_id = overridden["result"]["process_id"].as_i64().unwrap();
    let override_deadline = Instant::now() + Duration::from_secs(8);
    loop {
        let raw = registry
            .lock()
            .await
            .raw_output(override_process_id, None, usize::MAX)?;
        let output = String::from_utf8_lossy(&raw.data);
        let expected_override_prompt =
            format!("agent-answer:{template_prompt}\n\nUse the selected agent.");
        if output.contains(&expected_override_prompt) {
            assert_eq!(output.matches("agent-answer:").count(), 1);
            assert!(output.contains("agent-paste:bracketed"));
            assert!(output.contains("agent-args:--caller|override"));
            assert!(!output.contains("--template"));
            assert_eq!(
                registry
                    .lock()
                    .await
                    .get_status(override_process_id)?
                    .process
                    .agent_tool_id,
                Some(override_tool_id)
            );
            break;
        }
        assert!(
            Instant::now() < override_deadline,
            "overridden template prompt was not delivered after readiness: {}",
            output
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    let unknown_override = rpc(
        &mut socket,
        40,
        "agents.spawn",
        json!({
            "project_id": 7,
            "agent_template_id": template_id,
            "agent_tool_id": 999_999
        }),
    )
    .await;
    assert_eq!(unknown_override["error"]["code"], "spawn_failed");
    assert_eq!(
        unknown_override["error"]["message"],
        "agent tool 999999 was not found"
    );

    let disabled_override = rpc(
        &mut socket,
        41,
        "agent_tools.save",
        json!({
            "tool": {
                "id": override_tool_id,
                "name": "Override agent",
                "command": fake_agent,
                "tool_type": "scripted",
                "enabled": false
            }
        }),
    )
    .await;
    assert_eq!(disabled_override["result"]["enabled"], false);
    let disabled_override_spawn = rpc(
        &mut socket,
        42,
        "agents.spawn",
        json!({
            "project_id": 7,
            "agent_template_id": template_id,
            "agent_tool_id": override_tool_id
        }),
    )
    .await;
    assert_eq!(disabled_override_spawn["error"]["code"], "spawn_failed");
    assert!(
        disabled_override_spawn["error"]["message"]
            .as_str()
            .unwrap()
            .contains("is disabled")
    );

    let prompted = rpc(
        &mut socket,
        7,
        "process.send_input",
        json!({
            "process_id": process_id,
            "data": BASE64.encode("hello from the agents UI"),
            "submit": true
        }),
    )
    .await;
    assert_eq!(prompted["result"]["status"], "running");

    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let output = registry.lock().await.rendered_output(process_id)?;
        if output
            .text
            .contains("agent-answer:hello from the agents UI")
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "agent did not receive submitted prompt"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    let statuses = rpc(&mut socket, 8, "process.list", json!({ "project_id": 7 })).await;
    let spawned_status = statuses["result"]
        .as_array()
        .unwrap()
        .iter()
        .find(|status| status["id"] == process_id)
        .unwrap();
    assert_eq!(spawned_status["agent_tool_id"], tool_id);
    assert!(spawned_status["agent_state"]["state"].is_string());

    let closed = rpc(
        &mut socket,
        9,
        "process.close",
        json!({ "process_id": process_id }),
    )
    .await;
    assert_eq!(closed["result"]["id"], process_id);
    assert_eq!(closed["result"]["status"], "stopped");
    let closed_template = rpc(
        &mut socket,
        28,
        "process.close",
        json!({ "process_id": template_process_id }),
    )
    .await;
    assert_eq!(closed_template["result"]["status"], "stopped");
    assert!(!saved_attachment.parent().unwrap().exists());
    let closed_override = rpc(
        &mut socket,
        43,
        "process.close",
        json!({ "process_id": override_process_id }),
    )
    .await;
    assert_eq!(closed_override["result"]["status"], "stopped");
    let closed_exiting = rpc(
        &mut socket,
        33,
        "process.close",
        json!({ "process_id": exiting_process_id }),
    )
    .await;
    assert_eq!(closed_exiting["result"]["status"], "exited");
    let closed_model = rpc(
        &mut socket,
        73,
        "process.close",
        json!({ "process_id": model_process_id }),
    )
    .await;
    assert_eq!(closed_model["result"]["status"], "exited");
    let deleted_model_template = rpc(
        &mut socket,
        74,
        "agent_templates.delete",
        json!({ "agent_template_id": model_template_id }),
    )
    .await;
    assert_eq!(deleted_model_template["result"]["deleted"], true);
    let deleted_template = rpc(
        &mut socket,
        29,
        "agent_templates.delete",
        json!({ "agent_template_id": template_id }),
    )
    .await;
    assert_eq!(deleted_template["result"]["deleted"], true);
    let deleted = rpc(
        &mut socket,
        10,
        "agent_tools.delete",
        json!({ "agent_tool_id": tool_id }),
    )
    .await;
    assert_eq!(deleted["result"]["deleted"], true);
    let deleted_override_tool = rpc(
        &mut socket,
        44,
        "agent_tools.delete",
        json!({ "agent_tool_id": override_tool_id }),
    )
    .await;
    assert_eq!(deleted_override_tool["result"]["deleted"], true);
    let deleted_exiting_tool = rpc(
        &mut socket,
        32,
        "agent_tools.delete",
        json!({ "agent_tool_id": exiting_tool_id }),
    )
    .await;
    assert_eq!(deleted_exiting_tool["result"]["deleted"], true);
    let deleted_model_tool = rpc(
        &mut socket,
        75,
        "agent_tools.delete",
        json!({ "agent_tool_id": model_tool_id }),
    )
    .await;
    assert_eq!(deleted_model_tool["result"]["deleted"], true);

    socket.close(None).await?;
    let _ = shutdown_tx.send(());
    server_task.await??;
    Ok(())
}
