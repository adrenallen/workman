//! Harness-free integration test whose executable doubles as a disposable workmand.

use std::{env, path::PathBuf, process::Command, time::Duration};

use workman_core::Store;
use workmand::{DaemonConfig, DaemonServer, Discovery, database_path, discovery_path, probe};

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
        run_case(false).await;
        run_case(true).await;
    });
}

async fn run_case(use_environment: bool) {
    let data_dir = tempfile::tempdir().unwrap();
    let project_dir = tempfile::tempdir().unwrap();
    let daemon_executable = env::current_exe().unwrap();
    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_wrk"));
    command.env("WORKMAN_REQUIRE_EXPLICIT_DAEMON", "1");
    if use_environment {
        command.env("WORKMAN_DATA_DIR", data_dir.path());
    } else {
        command.arg("--data-dir").arg(data_dir.path());
    }
    let output = command
        .arg("--daemon")
        .arg(&daemon_executable)
        .current_dir(project_dir.path())
        .output()
        .await
        .unwrap();
    assert!(
        output.status.success(),
        "auto-spawned CLI failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
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
    assert!(probe(&discovery).await);
    terminate(discovery.pid);

    tokio::time::timeout(Duration::from_secs(2), async {
        while discovery_path(data_dir.path()).exists() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("auto-spawned daemon did not remove discovery after SIGTERM");
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
