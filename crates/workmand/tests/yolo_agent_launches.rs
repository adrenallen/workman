use std::{
    env, error::Error, ffi::OsString, fs, os::unix::fs::PermissionsExt, path::Path, time::Duration,
};

use rmcp::{
    ServiceExt,
    model::{CallToolRequestParams, ClientInfo},
    transport::{
        StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use serde_json::{Map, Value, json};
use workman_core::{AgentTool, AgentToolSource, Project};
use workmand::{DaemonConfig, DaemonServer, WORKMAN_CONFIG_ENV};

struct EnvGuard {
    name: &'static str,
    previous: Option<OsString>,
}

impl EnvGuard {
    fn set(name: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
        let previous = env::var_os(name);
        // SAFETY: tests using this guard run alone, and the environment is restored only after
        // the isolated daemon and all of its child processes stop.
        unsafe { env::set_var(name, value) };
        Self { name, previous }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        // SAFETY: see EnvGuard::set; no sibling test observes this variable while the guard lives.
        unsafe {
            match self.previous.take() {
                Some(previous) => env::set_var(self.name, previous),
                None => env::remove_var(self.name),
            }
        }
    }
}

fn arguments(value: Value) -> Map<String, Value> {
    value.as_object().expect("tool arguments object").clone()
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
    assert_ne!(result.is_error, Some(true), "{name} returned {result:?}");
    result
        .structured_content
        .unwrap_or_else(|| panic!("{name} returned no structured content"))
}

fn write_fixture_runtime(path: &Path) -> Result<(), Box<dyn Error>> {
    fs::write(
        path,
        r##"#!/usr/bin/python3
import json
import os
import sys
import urllib.request

args = sys.argv[1:]
if "--version" in args:
    print("workman-yolo-fixture 1.0")
    raise SystemExit(0)

runtime = next((arg.split("=", 1)[1] for arg in args if arg.startswith("--fixture-runtime=")), None)
required = {
    "claude": "--dangerously-skip-permissions",
    "codex": "--dangerously-bypass-approvals-and-sandbox",
}.get(runtime)
if required is None or required not in args:
    print("PERMISSION_PROMPT: configured yolo flag missing", flush=True)
    raise SystemExit(40)

claude_deep = runtime == "claude" and "--allowedTools" in args
codex_deep = runtime == "codex" and any("approval_mode=\"approve\"" in arg for arg in args)
deep_check = claude_deep or codex_deep
if not deep_check:
    if "--allowedTools" in args or any("approval_mode=" in arg for arg in args):
        print("PERMISSION_PROMPT: deep-check authorization leaked into normal launch", flush=True)
        raise SystemExit(41)
    print("WORKMAN_YOLO_LAUNCH_OK", flush=True)
    raise SystemExit(0)

url = os.environ["WORKMAN_MCP_URL"]
token = os.environ["WORKMAN_MCP_TOKEN"]
session = None

def post(payload):
    global session
    headers = {
        "Accept": "application/json, text/event-stream",
        "Content-Type": "application/json",
        "MCP-Protocol-Version": "2025-03-26",
        "x-workman-mcp-token": token,
    }
    if session:
        headers["Mcp-Session-Id"] = session
    request = urllib.request.Request(
        url,
        data=json.dumps(payload).encode("utf-8"),
        headers=headers,
        method="POST",
    )
    with urllib.request.urlopen(request, timeout=5) as response:
        session = response.headers.get("Mcp-Session-Id", session)
        body = response.read().decode("utf-8")
    if not body:
        return None
    data_lines = [line[5:].strip() for line in body.splitlines() if line.startswith("data:")]
    return json.loads(data_lines[-1] if data_lines else body)

initialized = post({
    "jsonrpc": "2.0",
    "id": 1,
    "method": "initialize",
    "params": {
        "protocolVersion": "2025-03-26",
        "capabilities": {},
        "clientInfo": {"name": "workman-yolo-fixture", "version": "1.0"},
    },
})
if not initialized or "result" not in initialized or not session:
    raise RuntimeError("MCP initialize did not return a result and session")
post({"jsonrpc": "2.0", "method": "notifications/initialized", "params": {}})
called = post({
    "jsonrpc": "2.0",
    "id": 2,
    "method": "tools/call",
    "params": {"name": "whoami", "arguments": {}},
})
if not called or "result" not in called:
    raise RuntimeError("MCP whoami did not return a result")
print("WORKMAN_DEEP_CHECK_OK", flush=True)
"##,
    )?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

async fn wait_for_output(
    registry: &workmand::SharedProcessRegistry,
    process_id: i64,
    expected: &str,
) -> Result<String, Box<dyn Error>> {
    for _ in 0..100 {
        let output = {
            let mut registry = registry.lock().await;
            let _ = registry.get_status(process_id);
            registry.rendered_output(process_id)?.text
        };
        if output.contains(expected) || output.contains("PERMISSION_PROMPT") {
            return Ok(output);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Err(format!("process {process_id} did not emit {expected:?}").into())
}

async fn wait_for_identity_and_output(
    registry: &workmand::SharedProcessRegistry,
    process_id: i64,
    expected: &str,
) -> Result<String, Box<dyn Error>> {
    for _ in 0..480 {
        let (identified, output, finished) = {
            let mut registry = registry.lock().await;
            let status = registry.get_status(process_id)?.process.status;
            let identified = registry.store().connection().query_row(
                "SELECT EXISTS(SELECT 1 FROM actors WHERE process_id = ?1)",
                [process_id],
                |row| row.get::<_, bool>(0),
            )?;
            let output = registry.rendered_output(process_id)?.text;
            (
                identified,
                output,
                matches!(
                    status,
                    workman_core::ProcessStatus::Exited | workman_core::ProcessStatus::Crashed
                ),
            )
        };
        if identified && output.contains(expected) {
            return Ok(output);
        }
        if finished {
            return Err(format!(
                "process {process_id} exited before identity + {expected:?}: {output}"
            )
            .into());
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    Err(format!("process {process_id} timed out before identity + {expected:?}").into())
}

#[tokio::test(flavor = "multi_thread")]
async fn isolated_normal_yolo_launches_and_deep_checks_both_succeed() -> Result<(), Box<dyn Error>>
{
    let temp = tempfile::tempdir()?;
    let config_path = temp.path().join("config.yml");
    let _config_guard = EnvGuard::set(WORKMAN_CONFIG_ENV, &config_path);
    let fixture = temp.path().join("fixture-agent");
    write_fixture_runtime(&fixture)?;
    let project_path = temp.path().join("project");
    fs::create_dir(&project_path)?;

    let server = DaemonServer::bind(DaemonConfig {
        data_dir: temp.path().join("state"),
        port: 0,
    })
    .await?;
    let registry = server.registry();
    let (claude_id, codex_id) = {
        let registry = registry.lock().await;
        let defaults = registry.store().list_agent_tools()?;
        let expected = [
            ("Claude", "claude --dangerously-skip-permissions"),
            ("Codex", "codex --dangerously-bypass-approvals-and-sandbox"),
            ("Gemini", "gemini --approval-mode=yolo"),
            ("OpenCode", "opencode --auto"),
            ("Kimi", "kimi --yolo"),
            (
                "DeepSeek v4 flash",
                "opencode --auto --model deepseek/deepseek-v4-flash",
            ),
        ];
        assert_eq!(defaults.len(), expected.len());
        for (name, command) in expected {
            assert_eq!(
                defaults
                    .iter()
                    .find(|tool| tool.name == name)
                    .map(|tool| tool.command.as_str()),
                Some(command),
                "fresh isolated {name} default"
            );
        }
        registry.store().put_project(&Project {
            id: 77,
            path: project_path.to_string_lossy().into_owned(),
            name: "yolo-isolated".into(),
            display_name: None,
            icon: None,
            selected: false,
            sort_order: 0,
        })?;
        let claude_id = defaults
            .iter()
            .find(|tool| tool.name == "Claude")
            .unwrap()
            .id;
        let codex_id = defaults
            .iter()
            .find(|tool| tool.name == "Codex")
            .unwrap()
            .id;
        registry.store().put_agent_tool(&AgentTool {
            id: claude_id,
            name: "Claude".into(),
            command: format!(
                "{} --fixture-runtime=claude --dangerously-skip-permissions",
                fixture.display()
            ),
            tool_type: "claude_code".into(),
            enabled: true,
            source: AgentToolSource::Local,
        })?;
        registry.store().put_agent_tool(&AgentTool {
            id: codex_id,
            name: "Codex".into(),
            command: format!(
                "{} --fixture-runtime=codex --dangerously-bypass-approvals-and-sandbox",
                fixture.display()
            ),
            tool_type: "codex".into(),
            enabled: true,
            source: AgentToolSource::Local,
        })?;
        (claude_id, codex_id)
    };

    let discovery = server.discovery().clone();
    let endpoint = format!("http://127.0.0.1:{}/mcp", discovery.port);
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let server_task = tokio::spawn(server.serve_until(async move {
        let _ = shutdown_rx.await;
    }));
    let transport = StreamableHttpClientTransport::from_config(
        StreamableHttpClientTransportConfig::with_uri(endpoint).auth_header(discovery.token),
    );
    let parent = ClientInfo::default().serve(transport).await?;

    for (name, tool_id) in [("claude", claude_id), ("codex", codex_id)] {
        let launched = call(
            &parent,
            "spawn_agent",
            json!({
                "project_id": 77,
                "agent_tool_id": tool_id,
                "name": format!("normal-{name}"),
                "extra_args": [],
                "auto_acknowledge_dialogs": false,
            }),
        )
        .await;
        let process_id = launched["process_id"].as_i64().unwrap();
        let output = wait_for_output(&registry, process_id, "WORKMAN_YOLO_LAUNCH_OK").await?;
        assert!(
            output.contains("WORKMAN_YOLO_LAUNCH_OK"),
            "{name}: {output}"
        );
        assert!(!output.contains("PERMISSION_PROMPT"), "{name}: {output}");
        call(
            &parent,
            "close_process",
            json!({ "project_id": 77, "process_id": process_id }),
        )
        .await;

        let deep = call(
            &parent,
            "agent_tool_deep_check",
            json!({
                "project_id": 77,
                "agent_tool_id": tool_id,
                "timeout_ms": 10_000,
            }),
        )
        .await;
        assert_eq!(deep["success"], true, "{name}: {deep}");
        assert!(
            deep["message"]
                .as_str()
                .is_some_and(|message| message.contains("confirmed the roundtrip")),
            "{name}: {deep}"
        );
    }

    let _ = parent.cancel().await;
    let _ = shutdown_tx.send(());
    server_task.await??;
    Ok(())
}

/// Explicit local acceptance against the installed authenticated Claude and Codex CLIs. This is
/// ignored for routine runs and serialized when invoked because it temporarily points this test
/// process at a per-todo config path.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires authenticated Claude and Codex CLIs; run alone with an isolated todo identity"]
async fn real_claude_and_codex_yolo_launches_and_deep_checks_succeed() -> Result<(), Box<dyn Error>>
{
    let temp = tempfile::Builder::new()
        .prefix("workman-todo424-")
        .tempdir_in("/tmp")?;
    let config_path = temp.path().join("config.yml");
    fs::write(
        &config_path,
        "terminal:\n  shell: /bin/zsh\nagent_tools:\n  - name: Claude\n    command: claude --dangerously-skip-permissions\n    tool_type: claude_code\n  - name: Codex\n    command: codex --dangerously-bypass-approvals-and-sandbox\n    tool_type: codex\n",
    )?;
    let _config_guard = EnvGuard::set(WORKMAN_CONFIG_ENV, &config_path);
    let project_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?;
    let server = DaemonServer::bind(DaemonConfig {
        data_dir: temp.path().join("data"),
        port: 0,
    })
    .await?;
    let registry = server.registry();
    let (claude_id, codex_id) = {
        let registry = registry.lock().await;
        registry.store().put_project(&Project {
            id: 424,
            path: project_path.to_string_lossy().into_owned(),
            name: "todo424-real-yolo-qa".into(),
            display_name: None,
            icon: None,
            selected: false,
            sort_order: 0,
        })?;
        let tools = registry.store().list_agent_tools()?;
        (
            tools.iter().find(|tool| tool.name == "Claude").unwrap().id,
            tools.iter().find(|tool| tool.name == "Codex").unwrap().id,
        )
    };

    let discovery = server.discovery().clone();
    let endpoint = format!("http://127.0.0.1:{}/mcp", discovery.port);
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let server_task = tokio::spawn(server.serve_until(async move {
        let _ = shutdown_rx.await;
    }));
    let transport = StreamableHttpClientTransport::from_config(
        StreamableHttpClientTransportConfig::with_uri(endpoint).auth_header(discovery.token),
    );
    let parent = ClientInfo::default().serve(transport).await?;
    let prompt = "Use only the MCP server named workman. Call whoami once. When it identifies you, print exactly WORKMAN_REAL_YOLO_OK and exit.";

    for (name, tool_id, extra_args) in [
        (
            "claude",
            claude_id,
            json!(["--print", "--output-format", "text", prompt]),
        ),
        (
            "codex",
            codex_id,
            json!(["exec", "--skip-git-repo-check", prompt]),
        ),
    ] {
        let launched = call(
            &parent,
            "spawn_agent",
            json!({
                "project_id": 424,
                "agent_tool_id": tool_id,
                "name": format!("todo424-real-{name}"),
                "extra_args": extra_args,
                "auto_acknowledge_dialogs": false,
            }),
        )
        .await;
        let process_id = launched["process_id"].as_i64().unwrap();
        let output =
            wait_for_identity_and_output(&registry, process_id, "WORKMAN_REAL_YOLO_OK").await?;
        assert!(
            !output.to_ascii_lowercase().contains("permission prompt"),
            "{name}: {output}"
        );
        call(
            &parent,
            "close_process",
            json!({ "project_id": 424, "process_id": process_id }),
        )
        .await;

        let deep = call(
            &parent,
            "agent_tool_deep_check",
            json!({
                "project_id": 424,
                "agent_tool_id": tool_id,
                "timeout_ms": 60_000,
            }),
        )
        .await;
        assert_eq!(deep["success"], true, "{name}: {deep}");
        assert!(
            deep["message"]
                .as_str()
                .is_some_and(|message| message.contains("called whoami through this daemon")),
            "{name}: {deep}"
        );
    }

    let _ = parent.cancel().await;
    let _ = shutdown_tx.send(());
    server_task.await??;
    Ok(())
}
