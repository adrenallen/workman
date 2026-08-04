use std::{path::Path, process::Command, time::Duration};

use gbuildd::{discover_or_spawn, discovery_path, probe};

struct TerminateOnDrop(u32);

impl Drop for TerminateOnDrop {
    fn drop(&mut self) {
        let _ = Command::new("kill")
            .arg("-TERM")
            .arg(self.0.to_string())
            .status();
    }
}

#[tokio::test]
async fn client_auto_spawns_and_discovers_daemon() {
    let temp = tempfile::tempdir().unwrap();
    let daemon = Path::new(env!("CARGO_BIN_EXE_gbuildd"));
    let discovery = discover_or_spawn(temp.path(), daemon, Duration::from_secs(5))
        .await
        .unwrap();
    let terminate = TerminateOnDrop(discovery.pid);

    assert!(probe(&discovery).await);

    drop(terminate);
    tokio::time::timeout(Duration::from_secs(2), async {
        while discovery_path(temp.path()).exists() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("daemon did not clean up discovery after SIGTERM");
}
