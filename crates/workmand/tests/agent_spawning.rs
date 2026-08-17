use std::{
    collections::{BTreeMap, HashMap},
    error::Error,
    os::unix::fs::PermissionsExt,
    path::Path,
    time::Duration,
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
use workman_core::{
    AgentTemplate, AgentTool, AgentToolSource, Process, ProcessKind, ProcessSource, ProcessStatus,
    Project,
};
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
            resume_args: None,
            continue_args: None,
        })?;
        registry.store().put_agent_tool(&AgentTool {
            id: 100,
            name: "Override agent".into(),
            command: fake_agent.to_string_lossy().into_owned(),
            tool_type: "scripted".into(),
            enabled: true,
            source: AgentToolSource::Local,
            resume_args: None,
            continue_args: None,
        })?;
        registry.store().put_agent_tool(&AgentTool {
            id: 101,
            name: "Disabled override".into(),
            command: fake_agent.to_string_lossy().into_owned(),
            tool_type: "scripted".into(),
            enabled: false,
            source: AgentToolSource::Local,
            resume_args: None,
            continue_args: None,
        })?;
        registry.store().put_agent_tool(&AgentTool {
            id: 102,
            name: "Model capture agent".into(),
            command: "true".into(),
            tool_type: "codex".into(),
            enabled: true,
            source: AgentToolSource::Local,
            resume_args: None,
            continue_args: None,
        })?;
        registry.store().put_agent_template(&AgentTemplate {
            id: 300,
            profile_id: 1,
            name: "Scripted worker".into(),
            agent_tool_id: 99,
            extra_args: Vec::new(),
            prompt: String::new(),
            sort_order: 0,
            created_at: 0,
            updated_at: 0,
        })?;
        registry.store().put_agent_template(&AgentTemplate {
            id: 301,
            profile_id: 1,
            name: "Reviewer".into(),
            agent_tool_id: 102,
            extra_args: vec![
                "--model".into(),
                "reviewer-default".into(),
                "--review".into(),
            ],
            prompt: "Review the implementation carefully and report concrete findings. ".repeat(3),
            sort_order: 1,
            created_at: 0,
            updated_at: 0,
        })?;
        registry.store().connection().execute_batch(
            "INSERT INTO profiles (id, name, active) VALUES (2, 'Other profile', 0);
             INSERT INTO agent_tools (
                id, name, display_name, command, tool_type, enabled, source, sort_order, profile_id
             ) VALUES (199, 'other-profile-tool', 'Other tool', 'true', 'custom', 1, 'local', 0, 2);
             INSERT INTO agent_templates (
                id, profile_id, name, agent_tool_id, extra_args, prompt, sort_order
             ) VALUES (299, 2, 'Other template', 199, '[]', '', 0);",
        )?;
        registry.store().put_process(&Process {
            id: 1,
            project_id: 7,
            kind: ProcessKind::Agent,
            name: "parent-agent".into(),
            command: Some("true".into()),
            working_dir: project_dir.to_string_lossy().into_owned(),
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
    call(&parent, "identify_session", json!({ "process_id": 1 })).await;

    let advertised_tools = parent.list_all_tools().await?;
    let spawn_tool = advertised_tools
        .iter()
        .find(|tool| tool.name == "spawn_agent")
        .expect("spawn_agent tool is present");
    for parameter in ["agent_template_id", "model"] {
        assert!(
            spawn_tool.input_schema["properties"]
                .get(parameter)
                .is_some(),
            "spawn_agent schema advertises {parameter}"
        );
    }
    assert!(
        spawn_tool.input_schema["properties"]
            .get("agent_template")
            .is_none(),
        "spawn_agent keeps template selection unambiguous and ID-only"
    );
    assert!(
        spawn_tool
            .description
            .as_deref()
            .is_some_and(|description| description.contains("plain agent by default"))
    );
    assert!(
        spawn_tool.input_schema["properties"]["agent_tool_id"]["description"]
            .as_str()
            .is_some_and(|description| description.contains("default plain-agent path"))
    );
    assert!(
        spawn_tool.input_schema["properties"]["agent_template_id"]["description"]
            .as_str()
            .is_some_and(|description| description.contains("only when the user names one"))
    );
    assert!(
        spawn_tool.input_schema["properties"]["model"]["description"]
            .as_str()
            .is_some_and(|description| description.contains("unsupported custom tool types"))
    );

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
    let templates = call(&parent, "list_agent_templates", json!({})).await;
    assert_eq!(templates["agent_templates"].as_array().unwrap().len(), 2);
    assert_eq!(templates["agent_templates"][0]["id"], 300);
    assert_eq!(templates["agent_templates"][0]["model"], Value::Null);
    assert_eq!(templates["agent_templates"][1]["id"], 301);
    assert_eq!(templates["agent_templates"][1]["name"], "Reviewer");
    assert_eq!(
        templates["agent_templates"][1]["default_agent"],
        json!({
            "agent_tool_id": 102,
            "name": "Model capture agent",
            "tool_type": "codex"
        })
    );
    assert_eq!(templates["agent_templates"][1]["model"], "reviewer-default");
    assert_eq!(
        templates["agent_templates"][1]["extra_args"],
        json!(["--model", "reviewer-default", "--review"])
    );
    assert!(
        templates["agent_templates"][1]["prompt_preview"]
            .as_str()
            .unwrap()
            .starts_with("Review the implementation")
    );
    assert_eq!(
        templates["agent_templates"][1]["prompt_preview"]
            .as_str()
            .unwrap()
            .chars()
            .count(),
        120
    );
    assert!(templates["agent_templates"][1].get("prompt").is_none());

    let default_model_spawn = call(
        &parent,
        "spawn_agent",
        json!({
            "project_id": 7,
            "agent_template_id": 301,
            "name": "reviewer-default-model"
        }),
    )
    .await;
    let default_model_process_id = default_model_spawn["process_id"].as_i64().unwrap();
    let default_model_command = registry
        .lock()
        .await
        .get_status(default_model_process_id)?
        .process
        .command
        .unwrap();
    assert_eq!(default_model_command.matches("--model").count(), 1);
    assert!(default_model_command.contains("--model reviewer-default"));
    call(
        &parent,
        "close_process",
        json!({ "project_id": 7, "process_id": default_model_process_id }),
    )
    .await;

    let override_model_spawn = call(
        &parent,
        "spawn_agent",
        json!({
            "project_id": 7,
            "agent_template_id": 301,
            "model": "override/provider-model",
            "name": "reviewer-override-model"
        }),
    )
    .await;
    let override_model_process_id = override_model_spawn["process_id"].as_i64().unwrap();
    let override_model_command = registry
        .lock()
        .await
        .get_status(override_model_process_id)?
        .process
        .command
        .unwrap();
    assert_eq!(override_model_command.matches("--model").count(), 1);
    assert!(override_model_command.contains("--model override/provider-model"));
    assert!(!override_model_command.contains("reviewer-default"));
    call(
        &parent,
        "close_process",
        json!({ "project_id": 7, "process_id": override_model_process_id }),
    )
    .await;

    let plain_spawn = call(
        &parent,
        "spawn_agent",
        json!({
            "project_id": 7,
            "agent_tool_id": 102,
            "model": "plain-model",
            "name": "plain-model-agent"
        }),
    )
    .await;
    let plain_process_id = plain_spawn["process_id"].as_i64().unwrap();
    let plain_command = registry
        .lock()
        .await
        .get_status(plain_process_id)?
        .process
        .command
        .unwrap();
    assert_eq!(plain_command.matches("--model").count(), 1);
    assert!(plain_command.contains("--model plain-model"));
    call(
        &parent,
        "close_process",
        json!({ "project_id": 7, "process_id": plain_process_id }),
    )
    .await;

    let cross_profile = parent
        .call_tool(
            CallToolRequestParams::new("spawn_agent").with_arguments(arguments(json!({
                "project_id": 7,
                "agent_template_id": 299
            }))),
        )
        .await?;
    assert_eq!(cross_profile.is_error, Some(true));
    assert_eq!(
        cross_profile.structured_content.unwrap()["code"],
        "spawn_failed"
    );

    let oversized_prompt = parent
        .call_tool(
            CallToolRequestParams::new("spawn_agent").with_arguments(arguments(json!({
                "project_id": 7,
                "agent_tool_id": 99,
                "initial_prompt": "x".repeat(64 * 1024 + 1)
            }))),
        )
        .await?;
    assert_eq!(oversized_prompt.is_error, Some(true));
    assert_eq!(
        oversized_prompt.structured_content.unwrap()["code"],
        "invalid_params"
    );

    for (agent_tool_id, expected) in [
        (999, "agent tool 999 was not found"),
        (101, "agent tool 101 (Disabled override) is disabled"),
    ] {
        let rejected = parent
            .call_tool(
                CallToolRequestParams::new("spawn_agent").with_arguments(arguments(json!({
                    "project_id": 7,
                    "agent_template_id": 300,
                    "agent_tool_id": agent_tool_id
                }))),
            )
            .await?;
        assert_eq!(rejected.is_error, Some(true));
        let details = rejected.structured_content.unwrap();
        assert_eq!(details["code"], "spawn_failed");
        assert_eq!(details["message"], expected);
    }

    let override_context_file = temp.path().join("override-context.txt");
    let overridden = call(
        &parent,
        "spawn_agent",
        json!({
            "project_id": 7,
            "agent_template_id": 300,
            "agent_tool_id": 100,
            "name": "override-worker",
            "extra_args": [override_context_file]
        }),
    )
    .await;
    let override_process_id = overridden["process_id"].as_i64().unwrap();
    let (injected_override_id, _, _) = wait_for_fake_agent_context(&override_context_file).await?;
    assert_eq!(injected_override_id, override_process_id);
    assert_eq!(
        registry
            .lock()
            .await
            .get_status(override_process_id)?
            .process
            .agent_tool_id,
        Some(100)
    );
    assert_eq!(
        call(
            &parent,
            "close_process",
            json!({ "project_id": 7, "process_id": override_process_id }),
        )
        .await["closed"],
        true
    );

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
            "agent_template_id": 300,
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
