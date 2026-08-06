use std::{os::unix::fs::PermissionsExt, process::Stdio, time::Duration};

use tempfile::TempDir;
use tokio::{
    io::AsyncWriteExt,
    process::{Child, Command},
    sync::oneshot,
};
use workman_core::{AgentTool, AgentToolSource, Project};
use workmand::{
    DaemonConfig, DaemonServer, Discovery, sync_workman_yml_file, trust_hash_for_process,
};

struct TestDaemon {
    data_dir: TempDir,
    project_dir: TempDir,
    discovery: Discovery,
    shutdown: Option<oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<std::io::Result<()>>,
    agent_marker: std::path::PathBuf,
}

impl TestDaemon {
    async fn start() -> Self {
        let data_dir = tempfile::tempdir().unwrap();
        let project_dir = tempfile::tempdir().unwrap();
        let fake_agent = project_dir.path().join("fake-codex-agent.sh");
        let agent_marker = project_dir.path().join("agent-launch.txt");
        std::fs::write(
            &fake_agent,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$WORKMAN_MCP_URL\" > '{}'\nif [ -n \"$WORKMAN_MCP_TOKEN\" ]; then printf 'token-present\\n' >> '{}'; fi\nprintf '%s\\n' \"$@\" >> '{}'\nsleep 30\n",
                agent_marker.display(),
                agent_marker.display(),
                agent_marker.display(),
            ),
        )
        .unwrap();
        std::fs::set_permissions(&fake_agent, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::write(
            project_dir.path().join("workman.yml"),
            "processes:\n  Saved:\n    command: \"trap 'exit 0' TERM; sleep 30\"\n",
        )
        .unwrap();
        let server = DaemonServer::bind(DaemonConfig {
            data_dir: data_dir.path().to_owned(),
            port: 0,
        })
        .await
        .unwrap();
        let discovery = server.discovery().clone();
        let registry = server.registry();
        let mut registry = registry.lock().await;
        let canonical_project = project_dir.path().canonicalize().unwrap();
        registry
            .store()
            .put_project(&Project {
                id: 1,
                path: canonical_project.to_string_lossy().into_owned(),
                name: "cli-test".into(),
                display_name: None,
                icon: None,
                selected: true,
                sort_order: 0,
            })
            .unwrap();
        registry
            .store()
            .put_agent_tool(&AgentTool {
                id: 900,
                name: "CLI fake Codex".into(),
                command: fake_agent.to_string_lossy().into_owned(),
                tool_type: "codex".into(),
                enabled: true,
                source: AgentToolSource::Local,
            })
            .unwrap();
        sync_workman_yml_file(&mut registry, 1).unwrap();
        let saved = registry
            .list(Some(1))
            .unwrap()
            .into_iter()
            .find(|process| process.name == "Saved")
            .unwrap();
        let hash = trust_hash_for_process(&saved);
        registry.trust_yml_process(saved.id, &hash).unwrap();
        drop(registry);
        let (shutdown, receive_shutdown) = oneshot::channel();
        let task = tokio::spawn(server.serve_until(async move {
            let _ = receive_shutdown.await;
        }));
        Self {
            data_dir,
            project_dir,
            discovery,
            shutdown: Some(shutdown),
            task,
            agent_marker,
        }
    }

    async fn stop(mut self) {
        self.shutdown.take().unwrap().send(()).unwrap();
        self.task.await.unwrap().unwrap();
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_wrk"));
        command
            .arg("--data-dir")
            .arg(self.data_dir.path())
            .arg("--daemon")
            .arg("/daemon-must-not-be-spawned-in-this-test")
            .current_dir(self.project_dir.path());
        command
    }

    async fn output(&self, args: &[&str]) -> std::process::Output {
        let output = self.command().args(args).output().await.unwrap();
        assert!(
            output.status.success(),
            "wrk {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        output
    }

    async fn run(&self, name: &str, shell_command: &str) -> i64 {
        let output = self
            .output(&[
                "run",
                "--name",
                name,
                "--cwd",
                self.project_dir.path().to_str().unwrap(),
                "--",
                shell_command,
            ])
            .await;
        parse_started_id(&output.stdout)
    }

    fn attach(&self, process_id: i64) -> Child {
        self.command()
            .arg("attach")
            .arg(process_id.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap()
    }
}

fn parse_started_id(stdout: &[u8]) -> i64 {
    String::from_utf8_lossy(stdout)
        .split_whitespace()
        .nth(2)
        .unwrap()
        .parse()
        .unwrap()
}

async fn wait_for_logs(daemon: &TestDaemon, process_id: i64, needle: &str) -> String {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let output = daemon.output(&["logs", &process_id.to_string()]).await;
            let logs = String::from_utf8_lossy(&output.stdout).into_owned();
            if logs.contains(needle) {
                return logs;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("timed out waiting for CLI-visible output")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn all_cli_commands_drive_a_real_daemon() {
    let daemon = TestDaemon::start().await;

    let status = daemon.output(&[]).await;
    let status = String::from_utf8_lossy(&status.stdout);
    assert!(
        status.contains("cli-test · workspace status"),
        "unexpected status output:\n{status}"
    );
    assert!(status.contains("Saved"));
    assert!(status.contains("healthy"));

    let setup = daemon.output(&["mcp-setup"]).await;
    let setup = String::from_utf8_lossy(&setup.stdout);
    assert!(
        setup.starts_with("Claude Code\n-----------\nclaude mcp add --transport http workman ")
    );
    assert!(setup.contains("Codex\n-----\n"));
    assert!(setup.contains("Gemini CLI\n----------\n"));
    assert!(setup.contains("OpenCode\n--------\n"));
    assert!(setup.contains("Generic\n-------\n"));
    assert!(setup.contains(&format!("http://127.0.0.1:{}/mcp", daemon.discovery.port)));
    assert!(setup.contains(&daemon.discovery.token));

    let codex_setup = daemon.output(&["mcp-setup", "--client", "codex"]).await;
    let codex_setup = String::from_utf8_lossy(&codex_setup.stdout);
    assert!(codex_setup.contains("[mcp_servers.workman]"));
    assert!(codex_setup.contains("env_http_headers"));
    assert!(!codex_setup.contains("Claude Code\n"));

    let up = daemon.output(&["up"]).await;
    assert!(String::from_utf8_lossy(&up.stdout).contains("Started 1 command"));
    let ps = daemon.output(&["ps"]).await;
    let saved = String::from_utf8_lossy(&ps.stdout);
    assert!(
        saved
            .lines()
            .any(|line| line.contains("Saved") && line.contains("running"))
    );
    let down = daemon.output(&["down"]).await;
    assert!(String::from_utf8_lossy(&down.stdout).contains("Stopped 1 command"));

    let agent = daemon
        .output(&[
            "agent",
            "--tool",
            "900",
            "--name",
            "cli-agent",
            "--",
            "--probe",
        ])
        .await;
    let agent_id = parse_started_id(&agent.stdout);
    let agent_launch = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if let Ok(contents) = std::fs::read_to_string(&daemon.agent_marker) {
                break contents;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("timed out waiting for CLI agent launch context");
    assert!(agent_launch.contains(&format!("http://127.0.0.1:{}/mcp", daemon.discovery.port)));
    assert!(agent_launch.contains("token-present"));
    assert!(agent_launch.contains("mcp_servers.workman.url"));
    assert!(agent_launch.contains("WORKMAN_MCP_TOKEN"));
    assert!(agent_launch.contains("--probe"));
    let stopped = daemon.output(&["stop", &agent_id.to_string()]).await;
    assert!(String::from_utf8_lossy(&stopped.stdout).contains("Stopped process"));

    let extra = tempfile::tempdir().unwrap();
    let added = daemon
        .output(&["add", extra.path().to_str().unwrap()])
        .await;
    assert!(String::from_utf8_lossy(&added.stdout).contains("Added to workman"));

    let interactive = daemon
        .run(
            "interactive",
            "printf 'ready\\n'; IFS= read -r line; printf 'got:%s\\n' \"$line\"",
        )
        .await;
    let ps = daemon.output(&["ps"]).await;
    let ps = String::from_utf8_lossy(&ps.stdout);
    assert!(ps.contains("interactive"));
    assert!(ps.contains("running"));
    assert!(ps.contains(&interactive.to_string()));
    assert!(
        wait_for_logs(&daemon, interactive, "ready")
            .await
            .contains("ready")
    );

    let mut attached = daemon.attach(interactive);
    attached
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"hello from attach\n")
        .await
        .unwrap();
    drop(attached.stdin.take());
    let attached = tokio::time::timeout(Duration::from_secs(3), attached.wait_with_output())
        .await
        .expect("attach did not return after the process exited")
        .unwrap();
    assert!(
        attached.status.success(),
        "attach failed: {}",
        String::from_utf8_lossy(&attached.stderr)
    );
    assert!(String::from_utf8_lossy(&attached.stdout).contains("got:hello from attach"));

    let followed = daemon
        .run(
            "followed",
            "printf 'first\\n'; sleep 0.1; printf 'second\\n'",
        )
        .await;
    let followed = daemon
        .output(&["logs", "--follow", &followed.to_string()])
        .await;
    let followed = String::from_utf8_lossy(&followed.stdout);
    assert!(followed.contains("first"));
    assert!(followed.contains("second"));

    let sleeper = daemon.run("sleeper", "sleep 30").await;
    let stopped = daemon.output(&["stop", &sleeper.to_string()]).await;
    assert!(String::from_utf8_lossy(&stopped.stdout).contains("Stopped process"));
    let ps = daemon.output(&["ps", "--project", "1"]).await;
    let ps = String::from_utf8_lossy(&ps.stdout);
    let sleeper_row = ps.lines().find(|line| line.contains("sleeper")).unwrap();
    assert!(sleeper_row.contains("stopped"));

    daemon.stop().await;
}
