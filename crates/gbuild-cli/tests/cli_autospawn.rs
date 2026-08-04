//! Harness-free integration test whose executable doubles as a disposable gbuildd.

use std::{env, path::PathBuf, process::Command, time::Duration};

use gbuildd::{DaemonConfig, DaemonServer, Discovery, discovery_path, probe};

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
        let data_dir = tempfile::tempdir().unwrap();
        let daemon_executable = env::current_exe().unwrap();
        let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_gbuild"))
            .arg("--data-dir")
            .arg(data_dir.path())
            .arg("--daemon")
            .arg(&daemon_executable)
            .arg("ps")
            .output()
            .await
            .unwrap();
        assert!(
            output.status.success(),
            "auto-spawned CLI failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("STATUS"));

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
    });
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
