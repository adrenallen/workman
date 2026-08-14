// Drives Unix fixtures (shebang scripts, permission bits, symlinks); Windows
// fixture parity is tracked as follow-up work.
#![cfg(unix)]

#![cfg(unix)]

use std::{
    collections::HashSet,
    env,
    error::Error,
    ffi::OsString,
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::Duration,
};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::{net::TcpStream, sync::oneshot, time::Instant};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
    tungstenite::{Message, client::IntoClientRequest, http::header},
};
use workman_core::Project;
use workmand::{DaemonConfig, DaemonServer};

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

struct EnvironmentGuard {
    name: &'static str,
    previous: Option<OsString>,
}

impl EnvironmentGuard {
    fn set(name: &'static str, value: &Path) -> Self {
        let previous = env::var_os(name);
        // SAFETY: this integration binary contains one test and restores the variable on drop.
        unsafe { env::set_var(name, value) };
        Self { name, previous }
    }
}

impl Drop for EnvironmentGuard {
    fn drop(&mut self) {
        // SAFETY: no sibling test in this integration process reads this variable.
        unsafe {
            match self.previous.take() {
                Some(value) => env::set_var(self.name, value),
                None => env::remove_var(self.name),
            }
        }
    }
}

fn write_executable(path: &Path, body: &str) -> Result<(), Box<dyn Error>> {
    fs::write(path, body)?;
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

fn request(discovery: &workmand::Discovery) -> Result<axum::http::Request<()>, Box<dyn Error>> {
    let mut request = format!("ws://127.0.0.1:{}/ws", discovery.port).into_client_request()?;
    request.headers_mut().insert(
        header::AUTHORIZATION,
        format!("Bearer {}", discovery.token).parse()?,
    );
    Ok(request)
}

async fn rpc(socket: &mut Socket, id: &str, method: &str, params: Value) -> Value {
    socket
        .send(Message::Text(
            json!({ "id": id, "method": method, "params": params })
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    loop {
        let message = socket.next().await.unwrap().unwrap();
        let Message::Text(text) = message else {
            continue;
        };
        let response: Value = serde_json::from_str(&text).unwrap();
        if response["id"] == id {
            assert_eq!(response["ok"], true, "{response}");
            return response["result"].clone();
        }
    }
}

fn private_agent_homes(prefix: &str) -> HashSet<PathBuf> {
    let prefix = format!("workman-{prefix}-mcp.");
    fs::read_dir(env::temp_dir())
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(&prefix))
        })
        .collect()
}

async fn wait_for_private_home(prefix: &str, previous: &HashSet<PathBuf>) -> PathBuf {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(home) = private_agent_homes(prefix)
                .into_iter()
                .find(|home| !previous.contains(home))
            {
                return home;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .expect("agent home preparation did not begin")
}

async fn wait_for_terminal_bytes(socket: &mut Socket, needle: &[u8]) {
    let mut observed = Vec::new();
    loop {
        let message = socket.next().await.unwrap().unwrap();
        let Message::Binary(frame) = message else {
            continue;
        };
        if frame.len() < 21 || &frame[..4] != b"WRK1" {
            continue;
        }
        observed.extend_from_slice(&frame[21..]);
        if observed
            .windows(needle.len())
            .any(|window| window == needle)
        {
            return;
        }
    }
}

async fn wait_for_response(socket: &mut Socket, id: &str) -> Value {
    loop {
        let message = socket.next().await.unwrap().unwrap();
        let Message::Text(text) = message else {
            continue;
        };
        let response: Value = serde_json::from_str(&text).unwrap();
        if response["id"] == id {
            return response;
        }
    }
}

async fn measure_spawn_contention(
    control: &mut Socket,
    terminal_stream: &mut Socket,
    terminal_id: i64,
    tool_id: i64,
    adapter: &str,
    spawn_id: &str,
    marker: &[u8],
) -> Duration {
    let previous_homes = private_agent_homes(adapter);
    control
        .send(Message::Text(
            json!({
                "id": spawn_id,
                "method": "agents.spawn",
                "params": {
                    "project_id": 104,
                    "agent_tool_id": tool_id,
                    "name": format!("spawn-contention-{adapter}"),
                    "extra_args": [],
                    "auto_acknowledge_dialogs": false
                }
            })
            .to_string()
            .into(),
        ))
        .await
        .unwrap();
    let private_home = wait_for_private_home(adapter, &previous_homes).await;
    assert!(private_home.is_dir());

    let started = Instant::now();
    control
        .send(Message::Text(
            json!({
                "id": format!("latency-input-{adapter}"),
                "method": "process.send_input",
                "params": {
                    "process_id": terminal_id,
                    "data": BASE64.encode(marker)
                }
            })
            .to_string()
            .into(),
        ))
        .await
        .unwrap();
    tokio::time::timeout(
        Duration::from_secs(10),
        wait_for_terminal_bytes(terminal_stream, marker),
    )
    .await
    .expect("terminal input was not painted");
    started.elapsed()
}

async fn measure_dialog_poll_contention(
    spawn_control: &mut Socket,
    input_control: &mut Socket,
    terminal_stream: &mut Socket,
    terminal_id: i64,
    tool_id: i64,
    marker: &[u8],
) -> Duration {
    spawn_control
        .send(Message::Text(
            json!({
                "id": "spawn-codex",
                "method": "agents.spawn",
                "params": {
                    "project_id": 104,
                    "agent_tool_id": tool_id,
                    "name": "spawn-contention-codex",
                    "extra_args": [],
                    "auto_acknowledge_dialogs": true
                }
            })
            .to_string()
            .into(),
        ))
        .await
        .unwrap();
    // The stub prints immediately, which keeps first-run dialog polling active for at least the
    // 750 ms output-settle window. This pause puts the input in the middle of that window.
    tokio::time::sleep(Duration::from_millis(100)).await;

    let started = Instant::now();
    input_control
        .send(Message::Text(
            json!({
                "id": "latency-input-codex",
                "method": "process.send_input",
                "params": {
                    "process_id": terminal_id,
                    "data": BASE64.encode(marker)
                }
            })
            .to_string()
            .into(),
        ))
        .await
        .unwrap();
    let response = tokio::time::timeout(
        Duration::from_secs(2),
        wait_for_response(input_control, "latency-input-codex"),
    )
    .await
    .expect("dialog-contention input did not receive a response");
    assert_eq!(response["ok"], true, "{response}");
    tokio::time::timeout(
        Duration::from_secs(8),
        wait_for_terminal_bytes(terminal_stream, marker),
    )
    .await
    .expect("terminal input was not painted during dialog polling");
    started.elapsed()
}

async fn measure_daemon_timeout_contention(
    shared_socket: &mut Socket,
    terminal_id: i64,
    marker: &[u8],
) -> Duration {
    shared_socket
        .send(Message::Text(
            json!({
                "id": "stalling-readiness-request",
                "method": "process.wait_for_bound_port",
                "params": {
                    "process_id": terminal_id,
                    "timeout_ms": 900
                }
            })
            .to_string()
            .into(),
        ))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    let started = Instant::now();
    let mut input_frame = Vec::from(*b"WRI1");
    input_frame.extend_from_slice(&terminal_id.to_be_bytes());
    input_frame.extend_from_slice(marker);
    shared_socket
        .send(Message::Binary(input_frame.into()))
        .await
        .unwrap();
    tokio::time::timeout(
        Duration::from_secs(3),
        wait_for_terminal_bytes(shared_socket, marker),
    )
    .await
    .expect("terminal input was not painted while another RPC timed out");
    let latency = started.elapsed();

    let response = tokio::time::timeout(
        Duration::from_secs(3),
        wait_for_response(shared_socket, "stalling-readiness-request"),
    )
    .await
    .expect("stalling readiness request did not finish");
    assert_eq!(response["ok"], true, "{response}");
    assert_eq!(response["result"]["timed_out"], true, "{response}");
    latency
}

async fn collect_spawned_processes(control: &mut Socket, spawn_ids: &[&str]) -> Vec<i64> {
    tokio::time::timeout(Duration::from_secs(15), async {
        let mut pending = spawn_ids.iter().copied().collect::<HashSet<_>>();
        let mut process_ids = Vec::new();
        while !pending.is_empty() {
            let message = control.next().await.unwrap().unwrap();
            let Message::Text(text) = message else {
                continue;
            };
            let response: Value = serde_json::from_str(&text).unwrap();
            let Some(id) = response["id"].as_str() else {
                continue;
            };
            if !pending.remove(id) {
                continue;
            }
            assert_eq!(response["ok"], true, "{response}");
            process_ids.push(response["result"]["process_id"].as_i64().unwrap());
        }
        process_ids
    })
    .await
    .expect("agent spawn storm did not finish")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn unrelated_agent_home_preparation_does_not_delay_terminal_input()
-> Result<(), Box<dyn Error>> {
    const SOURCE_ENTRIES: usize = 20_000;
    const GROK_MARKER: &[u8] = b"WORKMAN_INPUT_104_GROK";
    const KIMI_MARKER: &[u8] = b"WORKMAN_INPUT_104_KIMI";
    const CODEX_MARKER: &[u8] = b"Z";

    let temp = tempfile::tempdir()?;
    let initial_grok_homes = private_agent_homes("grok");
    let initial_kimi_homes = private_agent_homes("kimi");
    let project_dir = temp.path().join("workspace");
    let source_home = temp.path().join("home/.grok");
    fs::create_dir_all(&project_dir)?;
    fs::create_dir_all(&source_home)?;
    fs::write(source_home.join("config.toml"), "[ui]\n")?;
    for index in 0..SOURCE_ENTRIES {
        fs::write(source_home.join(format!("seed-{index:05}")), [])?;
    }
    std::os::unix::fs::symlink(&source_home, temp.path().join("home/.kimi-code"))?;

    let shell = temp.path().join("isolated-shell");
    write_executable(
        &shell,
        &format!(
            "#!/bin/sh\nexport HOME={}\nexec /bin/sh \"$@\"\n",
            temp.path().join("home").display()
        ),
    )?;
    let config = temp.path().join("config.yml");
    fs::write(
        &config,
        format!("terminal:\n  shell: {}\n", shell.display()),
    )?;
    let _config = EnvironmentGuard::set("WORKMAN_CONFIG", &config);

    let fake_agent = temp.path().join("fake-grok");
    write_executable(
        &fake_agent,
        "#!/bin/sh\nprintf 'agent-ready\\n'\nsleep 30\n",
    )?;

    let server = DaemonServer::bind(DaemonConfig {
        data_dir: temp.path().join("com.workman.todo104-state"),
        port: 0,
    })
    .await?;
    let discovery = server.discovery().clone();
    server
        .registry()
        .lock()
        .await
        .store()
        .put_project(&Project {
            id: 104,
            path: project_dir.to_string_lossy().into_owned(),
            name: "input-latency".into(),
            display_name: None,
            icon: None,
            selected: true,
            sort_order: 0,
        })?;
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let server_task = tokio::spawn(server.serve_until(async move {
        let _ = shutdown_rx.await;
    }));

    let (mut control, _) = connect_async(request(&discovery)?).await?;
    let (mut terminal_stream, _) = connect_async(request(&discovery)?).await?;
    let (mut dialog_input, _) = connect_async(request(&discovery)?).await?;
    let tool = rpc(
        &mut control,
        "save-tool",
        "agent_tools.save",
        json!({
            "tool": {
                "name": "Latency Grok",
                "command": fake_agent,
                "tool_type": "grok",
                "enabled": true
            }
        }),
    )
    .await;
    let tool_id = tool["id"].as_i64().unwrap();
    let kimi_tool = rpc(
        &mut control,
        "save-kimi-tool",
        "agent_tools.save",
        json!({
            "tool": {
                "name": "Latency Kimi",
                "command": fake_agent,
                "tool_type": "kimi",
                "enabled": true
            }
        }),
    )
    .await;
    let kimi_tool_id = kimi_tool["id"].as_i64().unwrap();
    let codex_tool = rpc(
        &mut control,
        "save-codex-tool",
        "agent_tools.save",
        json!({
            "tool": {
                "name": "Latency Codex",
                "command": fake_agent,
                "tool_type": "codex",
                "enabled": true
            }
        }),
    )
    .await;
    let codex_tool_id = codex_tool["id"].as_i64().unwrap();
    let terminal = rpc(
        &mut control,
        "spawn-terminal",
        "process.spawn_terminal",
        json!({ "project_id": 104, "name": "latency-target" }),
    )
    .await;
    let terminal_id = terminal["id"].as_i64().unwrap();
    rpc(
        &mut terminal_stream,
        "attach",
        "terminal.attach",
        json!({ "process_id": terminal_id, "offset": 0 }),
    )
    .await;

    let grok_latency = measure_spawn_contention(
        &mut control,
        &mut terminal_stream,
        terminal_id,
        tool_id,
        "grok",
        "spawn-grok",
        GROK_MARKER,
    )
    .await;
    let kimi_latency = measure_spawn_contention(
        &mut control,
        &mut terminal_stream,
        terminal_id,
        kimi_tool_id,
        "kimi",
        "spawn-kimi",
        KIMI_MARKER,
    )
    .await;
    let codex_latency = measure_dialog_poll_contention(
        &mut control,
        &mut dialog_input,
        &mut terminal_stream,
        terminal_id,
        codex_tool_id,
        CODEX_MARKER,
    )
    .await;
    let daemon_timeout_latency =
        measure_daemon_timeout_contention(&mut terminal_stream, terminal_id, b"Q").await;
    let latency = grok_latency
        .max(kimi_latency)
        .max(codex_latency)
        .max(daemon_timeout_latency);
    eprintln!(
        "input_spawn_latency source_entries={SOURCE_ENTRIES} grok_paint_us={} kimi_paint_us={} codex_dialog_paint_us={} daemon_timeout_paint_us={} worst_paint_ms={}",
        grok_latency.as_micros(),
        kimi_latency.as_micros(),
        codex_latency.as_micros(),
        daemon_timeout_latency.as_micros(),
        latency.as_millis(),
    );
    assert!(
        latency <= Duration::from_millis(250),
        "unrelated spawn delayed key-to-terminal paint for {latency:?}"
    );

    let spawned =
        collect_spawned_processes(&mut control, &["spawn-grok", "spawn-kimi", "spawn-codex"]).await;
    for (index, process_id) in spawned.into_iter().enumerate() {
        rpc(
            &mut control,
            &format!("close-agent-{index}"),
            "process.close",
            json!({ "process_id": process_id }),
        )
        .await;
    }
    rpc(
        &mut control,
        "close-terminal",
        "process.close",
        json!({ "process_id": terminal_id }),
    )
    .await;
    assert_eq!(private_agent_homes("grok"), initial_grok_homes);
    assert_eq!(private_agent_homes("kimi"), initial_kimi_homes);

    control.close(None).await?;
    terminal_stream.close(None).await?;
    dialog_input.close(None).await?;
    let _ = shutdown_tx.send(());
    server_task.await??;
    Ok(())
}
