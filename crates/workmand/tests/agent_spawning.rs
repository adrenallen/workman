use std::{
    collections::HashMap, error::Error, os::unix::fs::PermissionsExt, path::Path, time::Duration,
};

use axum::http::{HeaderName, HeaderValue};
use rmcp::{
    ServiceExt,
    model::{CallToolRequestParams, ClientInfo},
    transport::{
        StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use serde_json::{Map, Value, json};
use workman_core::{AgentTool, AgentToolSource, Project};
use workmand::{DaemonConfig, DaemonServer, WORKMAN_MCP_TOKEN_HEADER};

fn arguments(value: Value) -> Map<String, Value> {
    value
        .as_object()
        .expect("tool arguments must be an object")
        .clone()
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

async fn wait_for_fake_agent_context(path: &Path) -> Result<(i64, String, String), Box<dyn Error>> {
    for _ in 0..200 {
        if let Ok(contents) = std::fs::read_to_string(path) {
            let mut lines = contents.lines();
            if let (Some(process_id), Some(token), Some(url)) =
                (lines.next(), lines.next(), lines.next())
                && !token.is_empty()
                && !url.is_empty()
            {
                return Ok((process_id.parse()?, token.to_owned(), url.to_owned()));
            }
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    Err("fake agent did not publish its injected workman context".into())
}

#[tokio::test]
async fn fake_agent_auto_identifies_answers_a_prompt_and_cannot_self_close_unconfirmed()
-> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let project_dir = temp.path().join("workspace");
    std::fs::create_dir_all(&project_dir)?;
    let fake_agent = temp.path().join("fake-agent.sh");
    let context_file = temp.path().join("agent-context.txt");
    std::fs::write(
        &fake_agent,
        "#!/bin/sh\n\
         printf '%s\\n%s\\n%s\\n' \"$WORKMAN_PROCESS_ID\" \"$WORKMAN_MCP_TOKEN\" \"$WORKMAN_MCP_URL\" > \"$1\"\n\
         printf 'ready:%s\\n' \"$WORKMAN_PROCESS_ID\"\n\
         IFS= read -r prompt\n\
         printf 'answer:%s\\n' \"$prompt\"\n\
         sleep 30\n",
    )?;
    let mut permissions = std::fs::metadata(&fake_agent)?.permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&fake_agent, permissions)?;

    let server = DaemonServer::bind(DaemonConfig {
        data_dir: temp.path().join("state"),
        port: 0,
    })
    .await?;
    let discovery = server.discovery().clone();
    let registry = server.registry();
    {
        let registry = registry.lock().await;
        registry.store().put_project(&Project {
            id: 7,
            path: project_dir.to_string_lossy().into_owned(),
            name: "workspace".into(),
            display_name: None,
            icon: None,
            selected: false,
            sort_order: 0,
        })?;
        registry.store().put_agent_tool(&AgentTool {
            id: 99,
            name: "Scripted Claude".into(),
            command: fake_agent.to_string_lossy().into_owned(),
            tool_type: "scripted".into(),
            enabled: true,
            source: AgentToolSource::Local,
        })?;
    }

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let server_task = tokio::spawn(server.serve_until(async move {
        let _ = shutdown_rx.await;
    }));
    let endpoint = format!("http://127.0.0.1:{}/mcp", discovery.port);
    let parent_transport = StreamableHttpClientTransport::from_config(
        StreamableHttpClientTransportConfig::with_uri(endpoint.clone())
            .auth_header(discovery.token.clone()),
    );
    let parent = ClientInfo::default().serve(parent_transport).await?;

    let tools = call(&parent, "list_agent_tools", json!({})).await;
    assert!(tools["agent_tools"].as_array().unwrap().iter().any(|tool| {
        tool["name"] == "Claude"
            && tool["command"]
                .as_str()
                .is_some_and(|command| command.starts_with("claude"))
    }));
    assert!(tools["agent_tools"].as_array().unwrap().iter().any(|tool| {
        tool["id"] == 99 && tool["command"] == fake_agent.to_string_lossy().as_ref()
    }));

    let terminal = call(
        &parent,
        "spawn_process",
        json!({ "project_id": 7, "kind": "terminal" }),
    )
    .await;
    let terminal_id = terminal["process_id"].as_i64().unwrap();
    assert_eq!(terminal["kind"], "terminal");
    assert!(terminal["name"].as_str().unwrap().starts_with("terminal--"));
    assert_eq!(terminal["agent_instructions"], Value::Null);
    assert_eq!(
        call(
            &parent,
            "close_process",
            json!({ "project_id": 7, "process_id": terminal_id }),
        )
        .await["closed"],
        true
    );

    let spawned = call(
        &parent,
        "spawn_agent",
        json!({
            "project_id": 7,
            "agent_tool_id": 99,
            "name": "fake-worker",
            "extra_args": [context_file],
        }),
    )
    .await;
    let process_id = spawned["process_id"].as_i64().unwrap();
    let instructions = spawned["agent_instructions"].as_str().unwrap();
    assert!(instructions.contains(&format!("WORKMAN_PROCESS_ID={process_id}")));
    assert!(instructions.contains(&format!("WORKMAN_MCP_URL={endpoint}")));
    assert!(instructions.contains("${WORKMAN_MCP_TOKEN}"));
    assert!(instructions.contains("This runtime is not auto-wired"));
    assert!(instructions.contains("no registered per-launch Workman MCP adapter"));
    assert!(instructions.contains("Workman MCP identity check is unavailable"));

    let (injected_process_id, injected_token, injected_url) =
        wait_for_fake_agent_context(&context_file).await?;
    assert_eq!(injected_process_id, process_id);
    assert_eq!(injected_url, endpoint);
    let agent_headers = HashMap::from([(
        HeaderName::from_static(WORKMAN_MCP_TOKEN_HEADER),
        HeaderValue::from_str(&injected_token)?,
    )]);
    let agent_transport = StreamableHttpClientTransport::from_config(
        StreamableHttpClientTransportConfig::with_uri(endpoint).custom_headers(agent_headers),
    );
    let agent = ClientInfo::default().serve(agent_transport).await?;
    let identity = call(&agent, "whoami", json!({})).await;
    assert_eq!(identity["process_id"], process_id);
    assert_eq!(identity["effective_project_id"], 7);

    let unconfirmed = agent
        .call_tool(
            CallToolRequestParams::new("close_process")
                .with_arguments(arguments(json!({ "process_id": process_id }))),
        )
        .await?;
    assert_eq!(unconfirmed.is_error, Some(true));
    assert_eq!(
        unconfirmed.structured_content.unwrap()["code"],
        "self_close_confirmation_required"
    );

    let prompted = call(
        &parent,
        "send_input",
        json!({
            "project_id": 7,
            "process_id": process_id,
            "input": "hello from orchestrator",
            "wait_ms": 250,
        }),
    )
    .await;
    assert_eq!(prompted["process_id"], process_id);
    assert!(prompted["bytes_sent"].as_u64().unwrap() > 0);
    let mut answered = false;
    for _ in 0..200 {
        let output = registry.lock().await.rendered_output(process_id)?;
        if output.text.contains("answer:hello from orchestrator") {
            answered = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert!(answered, "scripted agent did not answer its PTY prompt");

    let closed = call(
        &parent,
        "close_process",
        json!({ "project_id": 7, "process_id": process_id }),
    )
    .await;
    assert_eq!(closed["closed"], true);
    assert!(
        registry
            .lock()
            .await
            .store()
            .get_process(process_id)?
            .is_none()
    );

    let _ = agent.cancel().await;
    let _ = parent.cancel().await;
    let _ = shutdown_tx.send(());
    server_task.await??;
    Ok(())
}
