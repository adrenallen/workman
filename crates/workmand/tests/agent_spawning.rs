// Drives Unix fixtures (shebang scripts, permission bits, symlinks); Windows
// fixture parity is tracked as follow-up work.
#![cfg(unix)]

use std::{
    collections::BTreeMap, error::Error, os::unix::fs::PermissionsExt, path::Path, time::Duration,
};

use rmcp::{
    ServiceExt,
    model::{CallToolRequestParams, ClientInfo},
    transport::{
        StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use serde_json::{Map, Value, json};
use workman_core::{
    AgentTemplate, AgentTool, AgentToolSource, McpToolsProfile, Process, ProcessKind,
    ProcessSource, ProcessStatus, Project,
};
use workmand::{DaemonConfig, DaemonServer};

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
            mcp_tools_profile: Default::default(),
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
            mcp_tools_profile: Default::default(),
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
            mcp_tools_profile: Default::default(),
        })?;
        registry.store().put_agent_tool(&AgentTool {
            id: 102,
            name: "Model capture agent".into(),
            command: "true --model command-default".into(),
            tool_type: "codex".into(),
            enabled: true,
            source: AgentToolSource::Local,
            resume_args: None,
            continue_args: None,
            mcp_tools_profile: Default::default(),
        })?;
        registry.store().put_agent_tool(&AgentTool {
            id: 103,
            name: "Swap model agent".into(),
            command: "true -m swap-default".into(),
            tool_type: "opencode".into(),
            enabled: true,
            source: AgentToolSource::Local,
            resume_args: None,
            continue_args: None,
            mcp_tools_profile: Default::default(),
        })?;
        registry.store().put_agent_tool(&AgentTool {
            id: 104,
            name: "Portable Codex agent".into(),
            command: "true --model portable-default".into(),
            tool_type: "codex".into(),
            enabled: true,
            source: AgentToolSource::Local,
            resume_args: None,
            continue_args: None,
            mcp_tools_profile: Default::default(),
        })?;
        registry.store().put_agent_tool(&AgentTool {
            id: 105,
            name: "Custom agent".into(),
            command: "true".into(),
            tool_type: "custom".into(),
            enabled: true,
            source: AgentToolSource::Local,
            resume_args: None,
            continue_args: None,
            mcp_tools_profile: Default::default(),
        })?;
        registry.store().put_agent_tool(&AgentTool {
            id: 106,
            name: "Shell-composed Claude".into(),
            command: "export WORKMAN_TEMPLATE_PROBE=1 && true".into(),
            tool_type: "claude_code".into(),
            enabled: true,
            source: AgentToolSource::Local,
            resume_args: None,
            continue_args: None,
            mcp_tools_profile: Default::default(),
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
            mcp_tools_profile: Default::default(),
        })?;
        registry.store().put_agent_template(&AgentTemplate {
            id: 301,
            profile_id: 1,
            name: "Reviewer".into(),
            agent_tool_id: 102,
            extra_args: vec!["--review".into()],
            prompt: "🧪 Review the implementation carefully and report concrete findings. "
                .repeat(3),
            sort_order: 1,
            created_at: 0,
            updated_at: 0,
            mcp_tools_profile: Default::default(),
        })?;
        registry.store().put_agent_template(&AgentTemplate {
            id: 302,
            profile_id: 1,
            name: "Configured reviewer".into(),
            agent_tool_id: 102,
            extra_args: vec![
                "--model".into(),
                "template-model".into(),
                "-c".into(),
                "model_reasoning_effort=\"high\"".into(),
                "--review".into(),
            ],
            prompt: "Review the change.".into(),
            sort_order: 2,
            created_at: 0,
            updated_at: 0,
            mcp_tools_profile: Default::default(),
        })?;
        registry.store().put_agent_template(&AgentTemplate {
            id: 303,
            profile_id: 1,
            name: "Shell-composed reviewer".into(),
            agent_tool_id: 106,
            extra_args: vec!["--model".into(), "fable".into()],
            prompt: String::new(),
            sort_order: 3,
            created_at: 0,
            updated_at: 0,
            mcp_tools_profile: Default::default(),
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
        registry
            .store()
            .set_process_mcp_tools_profile(1, McpToolsProfile::Extended)?;
        registry
            .store()
            .set_process_mcp_token(1, "parent-process-token", 1_700_000_000_000)?;
    }

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let server_task = tokio::spawn(server.serve_until(async move {
        let _ = shutdown_rx.await;
    }));
    let endpoint = format!("http://127.0.0.1:{}/mcp", discovery.port);
    let parent_transport = StreamableHttpClientTransport::from_config(
        StreamableHttpClientTransportConfig::with_uri(endpoint.clone())
            .auth_header("parent-process-token".to_owned()),
    );
    let parent = ClientInfo::default().serve(parent_transport).await?;

    let advertised_tools = parent.list_all_tools().await?;
    let spawn_tool = advertised_tools
        .iter()
        .find(|tool| tool.name == "spawn_agent")
        .expect("spawn_agent tool is present");
    for parameter in ["agent_template_id", "model", "notify_spawner_on_idle"] {
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
            .is_some_and(|description| {
                description.contains("Template: set agent_template_id only")
                    && description
                        .contains("supplies its agent tool, model, effort, launch args and prompt")
                    && description.contains("initial_prompt is appended")
                    && description.contains("Pass model or agent_tool_id only to override")
                    && description.contains(
                        "override keeps the template's model only for the same agent type",
                    )
                    && description.contains("mcp_wired")
                    && !description.contains("preferred")
            })
    );
    assert!(
        spawn_tool.input_schema["properties"]["agent_tool_id"]["description"]
            .as_str()
            .is_some_and(|description| {
                description.contains("Required unless agent_template_id is set")
                    && description.contains("model only within the same agent type")
            })
    );
    assert!(
        spawn_tool.input_schema["properties"]["agent_template_id"]["description"]
            .as_str()
            .is_some_and(|description| description.contains("list_agent_tools.agent_templates"))
    );
    assert!(
        spawn_tool.input_schema["properties"]["model"]["description"]
            .as_str()
            .is_some_and(|description| {
                description.contains("tool_type")
                    && description.contains("registered command")
                    && description.contains("Omit it to use the template or agent default")
                    && !description.contains("Prefer")
            })
    );
    assert!(
        spawn_tool
            .description
            .as_deref()
            .is_some_and(|description| {
                description.contains("avoids an idle timer unless a deadline matters")
            })
    );
    assert!(
        spawn_tool.input_schema["properties"]["notify_spawner_on_idle"]["description"]
            .as_str()
            .is_some_and(|description| {
                description.contains("coalesced Workman turn")
                    && description.contains("defaults to false")
            })
    );
    let update_tool = advertised_tools
        .iter()
        .find(|tool| tool.name == "update_process")
        .expect("update_process tool is present");
    assert!(
        update_tool
            .description
            .as_deref()
            .is_some_and(|description| description.contains("notifications"))
    );

    let tools = call(&parent, "list_agent_tools", json!({})).await;
    assert!(tools["agent_tools"].as_array().unwrap().iter().any(|tool| {
        tool["name"] == "Claude"
            && tool["mcp_wired"] == true
            && tool["command"]
                .as_str()
                .is_some_and(|command| command.starts_with("claude"))
    }));
    assert!(tools["agent_tools"].as_array().unwrap().iter().any(|tool| {
        tool["id"] == 99
            && tool["mcp_wired"] == false
            && tool["command"] == fake_agent.to_string_lossy().as_ref()
    }));
    assert!(
        tools["agent_tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| { tool["id"] == 105 && tool["mcp_wired"] == false })
    );
    let templates = &tools;
    assert_eq!(templates["agent_templates"].as_array().unwrap().len(), 4);
    assert_eq!(templates["agent_templates"][0]["id"], 300);
    assert_eq!(
        templates["agent_templates"][0]["launch"],
        json!({
            "agent_tool_id": 99,
            "agent_tool_name": "Scripted Claude",
            "model": "agent default",
            "effort": "agent default"
        })
    );
    assert!(templates["agent_templates"][0].get("model").is_none());
    assert_eq!(templates["agent_templates"][1]["id"], 301);
    assert_eq!(templates["agent_templates"][1]["name"], "Reviewer");
    assert_eq!(
        templates["agent_templates"][1]["default_agent"],
        json!({
            "agent_tool_id": 102,
            "name": "Model capture agent",
            "tool_type": "codex",
            "enabled": true
        })
    );
    assert_eq!(
        templates["agent_templates"][1]["launch"],
        json!({
            "agent_tool_id": 102,
            "agent_tool_name": "Model capture agent",
            "model": "command-default",
            "effort": "agent default"
        })
    );
    assert_eq!(
        templates["agent_templates"][1]["extra_args"],
        json!(["--review"])
    );
    assert_eq!(
        templates["agent_templates"][2]["launch"],
        json!({
            "agent_tool_id": 102,
            "agent_tool_name": "Model capture agent",
            "model": "template-model",
            "effort": "high"
        })
    );
    assert!(
        templates["agent_templates"][1]["prompt_preview"]
            .as_str()
            .unwrap()
            .starts_with("🧪 Review the implementation")
    );
    assert_eq!(
        templates["agent_templates"][1]["prompt_preview"]
            .as_str()
            .unwrap()
            .chars()
            .count(),
        120
    );
    assert!(
        templates["agent_templates"][1]["prompt_preview"]
            .as_str()
            .unwrap()
            .ends_with('…')
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
    assert_eq!(
        default_model_spawn["resolved"],
        json!({
            "agent_tool_id": 102,
            "agent_tool_name": "Model capture agent",
            "model": "command-default",
            "effort": "agent default",
            "mcp_tools_profile": "core",
            "mcp_wired": true,
            "template_args_skipped": []
        })
    );
    let default_model_process_id = default_model_spawn["process_id"].as_i64().unwrap();
    let default_model_command = registry
        .lock()
        .await
        .get_status(default_model_process_id)?
        .process
        .command
        .unwrap();
    assert_eq!(default_model_command.matches("--model").count(), 1);
    assert!(default_model_command.contains("--model command-default"));
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
            "extra_args": ["-m", "caller-default"],
            "name": "reviewer-override-model"
        }),
    )
    .await;
    assert_eq!(
        override_model_spawn["resolved"]["model"],
        "override/provider-model"
    );
    assert_eq!(override_model_spawn["resolved"]["effort"], "agent default");
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
    assert!(!override_model_command.contains("command-default"));
    assert!(!override_model_command.contains("caller-default"));
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
    assert_eq!(plain_spawn["resolved"]["agent_tool_id"], 102);
    assert_eq!(plain_spawn["resolved"]["model"], "plain-model");
    assert_eq!(plain_spawn["resolved"]["effort"], "agent default");
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
    assert!(!plain_command.contains("command-default"));
    call(
        &parent,
        "close_process",
        json!({ "project_id": 7, "process_id": plain_process_id }),
    )
    .await;

    let swapped_model_spawn = call(
        &parent,
        "spawn_agent",
        json!({
            "project_id": 7,
            "agent_template_id": 301,
            "agent_tool_id": 103,
            "model": "swapped/provider-model",
            "name": "reviewer-swapped-model"
        }),
    )
    .await;
    assert_eq!(
        swapped_model_spawn["resolved"],
        json!({
            "agent_tool_id": 103,
            "agent_tool_name": "Swap model agent",
            "model": "swapped/provider-model",
            "effort": "agent default",
            "mcp_tools_profile": "core",
            "mcp_wired": true,
            "template_args_skipped": ["--review"]
        })
    );
    let swapped_model_process_id = swapped_model_spawn["process_id"].as_i64().unwrap();
    let swapped_model_command = registry
        .lock()
        .await
        .get_status(swapped_model_process_id)?
        .process
        .command
        .unwrap();
    assert_eq!(swapped_model_command.matches("--model").count(), 1);
    assert!(swapped_model_command.contains("--model swapped/provider-model"));
    assert!(!swapped_model_command.contains("swap-default"));
    assert!(!swapped_model_command.contains("--review"));
    call(
        &parent,
        "close_process",
        json!({ "project_id": 7, "process_id": swapped_model_process_id }),
    )
    .await;

    let portable_spawn = call(
        &parent,
        "spawn_agent",
        json!({
            "project_id": 7,
            "agent_template_id": 302,
            "agent_tool_id": 104,
            "name": "portable-template-settings"
        }),
    )
    .await;
    assert_eq!(
        portable_spawn["resolved"],
        json!({
            "agent_tool_id": 104,
            "agent_tool_name": "Portable Codex agent",
            "model": "template-model",
            "effort": "high",
            "mcp_tools_profile": "core",
            "mcp_wired": true,
            "template_args_skipped": ["--review"]
        })
    );
    let portable_process_id = portable_spawn["process_id"].as_i64().unwrap();
    let portable_command = registry
        .lock()
        .await
        .get_status(portable_process_id)?
        .process
        .command
        .unwrap();
    assert_eq!(portable_command.matches("--model").count(), 1);
    assert!(portable_command.contains("--model template-model"));
    assert!(!portable_command.contains("portable-default"));
    assert!(portable_command.contains("model_reasoning_effort"));
    call(
        &parent,
        "close_process",
        json!({ "project_id": 7, "process_id": portable_process_id }),
    )
    .await;

    let own_tool_model_spawn = call(
        &parent,
        "spawn_agent",
        json!({
            "project_id": 7,
            "agent_template_id": 302,
            "name": "template-model-on-own-tool"
        }),
    )
    .await;
    assert_eq!(
        own_tool_model_spawn["resolved"],
        json!({
            "agent_tool_id": 102,
            "agent_tool_name": "Model capture agent",
            "model": "template-model",
            "effort": "high",
            "mcp_tools_profile": "core",
            "mcp_wired": true,
            "template_args_skipped": []
        })
    );
    let own_tool_model_process_id = own_tool_model_spawn["process_id"].as_i64().unwrap();
    let own_tool_model_command = registry
        .lock()
        .await
        .get_status(own_tool_model_process_id)?
        .process
        .command
        .unwrap();
    assert_eq!(own_tool_model_command.matches("--model").count(), 1);
    assert!(own_tool_model_command.contains("--model template-model"));
    assert!(!own_tool_model_command.contains("command-default"));
    call(
        &parent,
        "close_process",
        json!({ "project_id": 7, "process_id": own_tool_model_process_id }),
    )
    .await;

    let unsupported_spawn = call(
        &parent,
        "spawn_agent",
        json!({
            "project_id": 7,
            "agent_template_id": 302,
            "agent_tool_id": 105,
            "name": "unsupported-template-settings"
        }),
    )
    .await;
    assert_eq!(
        unsupported_spawn["resolved"],
        json!({
            "agent_tool_id": 105,
            "agent_tool_name": "Custom agent",
            "model": "agent default",
            "effort": "agent default",
            "mcp_tools_profile": "core",
            "mcp_wired": false,
            "template_args_skipped": [
                "--model",
                "template-model",
                "-c",
                "model_reasoning_effort=\"high\"",
                "--review"
            ]
        })
    );
    let unsupported_process_id = unsupported_spawn["process_id"].as_i64().unwrap();
    call(
        &parent,
        "close_process",
        json!({ "project_id": 7, "process_id": unsupported_process_id }),
    )
    .await;

    let shell_composed_spawn = call(
        &parent,
        "spawn_agent",
        json!({
            "project_id": 7,
            "agent_template_id": 303,
            "name": "shell-composed-template-model"
        }),
    )
    .await;
    assert_eq!(
        shell_composed_spawn["resolved"],
        json!({
            "agent_tool_id": 106,
            "agent_tool_name": "Shell-composed Claude",
            "model": "fable",
            "effort": "agent default",
            "mcp_tools_profile": "core",
            "mcp_wired": true,
            "template_args_skipped": []
        })
    );
    let shell_composed_process_id = shell_composed_spawn["process_id"].as_i64().unwrap();
    let shell_composed_command = registry
        .lock()
        .await
        .get_status(shell_composed_process_id)?
        .process
        .command
        .unwrap();
    assert!(shell_composed_command.contains("export WORKMAN_TEMPLATE_PROBE=1 && true"));
    assert!(shell_composed_command.contains("--model fable"));
    call(
        &parent,
        "close_process",
        json!({ "project_id": 7, "process_id": shell_composed_process_id }),
    )
    .await;

    for (model, expected_code, expected_message) in [
        (
            "",
            "invalid_params",
            "model must not be empty when provided",
        ),
        (
            "   ",
            "invalid_params",
            "model must not be empty when provided",
        ),
        (
            "unsupported-model",
            "spawn_failed",
            "this agent tool has no known model flag",
        ),
    ] {
        let rejected = parent
            .call_tool(
                CallToolRequestParams::new("spawn_agent").with_arguments(arguments(json!({
                    "project_id": 7,
                    "agent_tool_id": 99,
                    "model": model
                }))),
            )
            .await?;
        assert_eq!(rejected.is_error, Some(true));
        let details = rejected.structured_content.unwrap();
        assert_eq!(details["code"], expected_code);
        assert!(
            details["message"]
                .as_str()
                .unwrap()
                .contains(expected_message)
        );
    }

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

    let terminal = call(&parent, "spawn_terminal", json!({ "project_id": 7 })).await;
    let terminal_id = terminal["process_id"].as_i64().unwrap();
    assert_eq!(terminal["kind"], "terminal");
    assert!(terminal["name"].as_str().unwrap().starts_with("terminal--"));
    assert!(terminal.get("agent_instructions").is_none());
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
    assert!(spawned.get("agent_instructions").is_none());

    let (injected_process_id, injected_token, injected_url) =
        wait_for_fake_agent_context(&context_file).await?;
    assert_eq!(injected_process_id, process_id);
    assert_eq!(injected_url, endpoint);
    let agent_transport = StreamableHttpClientTransport::from_config(
        StreamableHttpClientTransportConfig::with_uri(endpoint).auth_header(injected_token),
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
