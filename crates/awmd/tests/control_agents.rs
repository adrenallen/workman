use std::{error::Error, os::unix::fs::PermissionsExt, path::Path, time::Duration};

use awm_core::Project;
use awmd::{DaemonConfig, DaemonServer};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::{net::TcpStream, sync::oneshot, time::Instant};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
    tungstenite::{Message, client::IntoClientRequest, http::header},
};

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
        "#!/bin/sh\nprintf 'agent-ready\\n'\nIFS= read -r prompt\nprintf 'agent-answer:%s\\n' \"$prompt\"\nsleep 30\n",
    )?;
    let mut permissions = std::fs::metadata(path)?.permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(path, permissions)?;
    Ok(())
}

#[tokio::test]
async fn websocket_manages_tools_spawns_agents_and_submits_prompts() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let project_dir = temp.path().join("workspace");
    std::fs::create_dir(&project_dir)?;
    let fake_agent = temp.path().join("fake-agent.sh");
    write_fake_agent(&fake_agent)?;

    let server = DaemonServer::bind(DaemonConfig {
        data_dir: temp.path().join("state"),
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
            .contains("Call whoami() through awm first")
    );
    assert!(
        spawned["result"]["agent_instructions"]
            .as_str()
            .unwrap()
            .contains(&format!(
                "AWM_MCP_URL=http://127.0.0.1:{}/mcp",
                discovery.port
            ))
    );
    let process_id = spawned["result"]["process_id"].as_i64().unwrap();

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
    assert_eq!(statuses["result"][0]["agent_tool_id"], tool_id);
    assert!(statuses["result"][0]["agent_state"]["state"].is_string());

    let closed = rpc(
        &mut socket,
        9,
        "process.close",
        json!({ "process_id": process_id }),
    )
    .await;
    assert_eq!(closed["result"]["id"], process_id);
    assert_eq!(closed["result"]["status"], "stopped");
    let deleted = rpc(
        &mut socket,
        10,
        "agent_tools.delete",
        json!({ "agent_tool_id": tool_id }),
    )
    .await;
    assert_eq!(deleted["result"]["deleted"], true);

    socket.close(None).await?;
    let _ = shutdown_tx.send(());
    server_task.await??;
    Ok(())
}
