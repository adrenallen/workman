//! Harness-free integration test whose executable doubles as a disposable workmand.

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

use workman_core::Store;
use workmand::{DaemonConfig, DaemonServer, Discovery, database_path, discovery_path, probe};

const STARTUP_DELAY_ENV: &str = "WORKMAN_CLI_AUTOSPAWN_TEST_DELAY_MS";
const STARTED_MARKER_ENV: &str = "WORKMAN_CLI_AUTOSPAWN_TEST_STARTED_MARKER";
const SIMULATED_BUSY_STARTUP: Duration = Duration::from_secs(6);
const CLI_COMPLETION_TIMEOUT: Duration = Duration::from_secs(20);

fn main() {
    let mut args = env::args_os().skip(1);
    if matches!(args.next().as_deref(), Some(value) if value == "--data-dir") {
        let data_dir = PathBuf::from(args.next().expect("--data-dir requires a path"));
        run_daemon(data_dir);
    } else {
        run_test();
    }
}

fn run_daemon(data_dir: PathBuf) {
    if let Some(marker) = env::var_os(STARTED_MARKER_ENV) {
        fs::write(marker, std::process::id().to_string()).unwrap();
    }
    if let Some(delay) = env::var_os(STARTUP_DELAY_ENV) {
        let delay = delay
            .to_string_lossy()
            .parse::<u64>()
            .expect("startup delay must be milliseconds");
        std::thread::sleep(Duration::from_millis(delay));
    }
    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async move {
            let server = DaemonServer::bind(DaemonConfig { data_dir, port: 0 })
                .await
                .unwrap();
            server.serve_until(shutdown_signal()).await.unwrap();
        });
}

fn run_test() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        run_case(false, true).await;
        run_case(true, false).await;
    });
}

async fn run_case(use_environment: bool, simulate_busy_startup: bool) {
    let data_dir = tempfile::tempdir().unwrap();
    let project_dir = tempfile::tempdir().unwrap();
    let daemon_executable = env::current_exe().unwrap();
    let started_marker = data_dir.path().join("daemon-started");
    let started_at = Instant::now();
    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_wrk"));
    command
        .env("WORKMAN_REQUIRE_EXPLICIT_DAEMON", "1")
        .env(STARTED_MARKER_ENV, &started_marker);
    if use_environment {
        command.env("WORKMAN_DATA_DIR", data_dir.path());
    } else {
        command.arg("--data-dir").arg(data_dir.path());
    }
    if simulate_busy_startup {
        command.env(
            STARTUP_DELAY_ENV,
            SIMULATED_BUSY_STARTUP.as_millis().to_string(),
        );
    }
    let child = command
        .arg("--daemon")
        .arg(&daemon_executable)
        .current_dir(project_dir.path())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let daemon_pid = wait_for_daemon_start(&started_marker).await;
    let mut daemon_guard = TerminateOnDrop::new(daemon_pid);
    let output = tokio::time::timeout(CLI_COMPLETION_TIMEOUT, child.wait_with_output())
        .await
        .expect("auto-spawned CLI did not finish within its bounded startup window")
        .unwrap();
    let elapsed = started_at.elapsed();
    assert!(
        output.status.success(),
        "auto-spawned CLI failed after {elapsed:.2?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    println!("auto-spawned CLI became ready after {elapsed:.2?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("workspace status"));
    assert!(stdout.contains(project_dir.path().file_name().unwrap().to_str().unwrap()));
    assert!(stdout.contains("✓ healthy"));

    let store = Store::open(database_path(data_dir.path())).unwrap();
    let projects = store.list_projects().unwrap();
    assert_eq!(projects.len(), 1);
    assert_eq!(
        projects[0].path,
        project_dir.path().canonicalize().unwrap().to_string_lossy()
    );

    let discovery = Discovery::read(data_dir.path()).unwrap();
    assert_ne!(discovery.pid, std::process::id());
    assert_eq!(discovery.pid, daemon_pid);
    assert!(probe(&discovery).await);
    daemon_guard.terminate();

    tokio::time::timeout(Duration::from_secs(10), async {
        while discovery_path(data_dir.path()).exists() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("auto-spawned daemon did not remove discovery after SIGTERM");
}

async fn wait_for_daemon_start(path: &Path) -> u32 {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            match fs::read_to_string(path) {
                Ok(pid) => return pid.parse().expect("daemon marker must contain its PID"),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                Err(error) => panic!("could not read daemon start marker: {error}"),
            }
        }
    })
    .await
    .expect("wrk did not spawn the isolated daemon")
}

struct TerminateOnDrop(Option<u32>);

impl TerminateOnDrop {
    fn new(pid: u32) -> Self {
        Self(Some(pid))
    }

    fn terminate(&mut self) {
        if let Some(pid) = self.0.take() {
            terminate(pid);
        }
    }
}

impl Drop for TerminateOnDrop {
    fn drop(&mut self) {
        if let Some(pid) = self.0.take() {
            let _ = Command::new("kill")
                .arg("-TERM")
                .arg(pid.to_string())
                .status();
        }
    }
}

fn terminate(pid: u32) {
    let status = Command::new("kill")
        .arg("-TERM")
        .arg(pid.to_string())
        .status()
        .unwrap();
    assert!(status.success());
}

async fn shutdown_signal() {
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .expect("install SIGTERM handler");
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = terminate.recv() => {}
    }
}
