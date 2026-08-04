#![cfg(unix)]

use std::{
    collections::BTreeMap,
    fs,
    sync::Arc,
    time::{Duration, Instant},
};

use gbuild_core::{Process, ProcessKind, ProcessSource, ProcessStatus, Project, Store};
use gbuildd::{
    LifecycleOptions, ProcessRegistry, SharedProcessRegistry,
    spawn_lifecycle_supervisor_with_options,
};
use tempfile::TempDir;
use tokio::sync::{Mutex, watch};

fn test_options() -> LifecycleOptions {
    LifecycleOptions {
        reconcile_interval: Duration::from_millis(20),
        change_debounce: Duration::from_millis(80),
        restart_backoff_initial: Duration::from_millis(100),
        restart_backoff_max: Duration::from_millis(400),
        stable_run_reset: Duration::from_secs(2),
    }
}

fn fixture() -> (TempDir, ProcessRegistry) {
    let root = tempfile::tempdir().unwrap();
    let path = fs::canonicalize(root.path()).unwrap();
    let store = Store::open_in_memory().unwrap();
    store
        .put_project(&Project {
            id: 1,
            path: path.to_string_lossy().into_owned(),
            name: "lifecycle-fixture".into(),
            display_name: None,
            icon: None,
            selected: true,
        })
        .unwrap();
    (root, ProcessRegistry::new(store).unwrap())
}

fn process(root: &TempDir, id: i64, command: &str) -> Process {
    Process {
        id,
        project_id: 1,
        kind: ProcessKind::Command,
        name: format!("process-{id}"),
        command: Some(command.into()),
        working_dir: fs::canonicalize(root.path())
            .unwrap()
            .to_string_lossy()
            .into_owned(),
        env: BTreeMap::new(),
        auto_start: true,
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
    }
}

async fn wait_until(mut predicate: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !predicate() {
        assert!(
            Instant::now() < deadline,
            "condition was not met before timeout"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn stop_supervisor(
    shutdown_tx: watch::Sender<bool>,
    task: tokio::task::JoinHandle<()>,
    registry: &SharedProcessRegistry,
) {
    let _ = shutdown_tx.send(true);
    task.await.unwrap();
    let process_ids = registry
        .lock()
        .await
        .list(None)
        .unwrap()
        .into_iter()
        .map(|process| process.id)
        .collect::<Vec<_>>();
    let mut registry = registry.lock().await;
    for process_id in process_ids {
        let _ = registry.stop(process_id);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn watched_file_restart_uses_project_relative_globs_and_configured_env() {
    let (root, mut registry) = fixture();
    let nested = root.path().join("src/nested");
    fs::create_dir_all(&nested).unwrap();
    let watched = nested.join("watched.txt");
    fs::write(&watched, "before").unwrap();
    let launches = root.path().join("launches.txt");

    let mut command = process(
        &root,
        1,
        "printf '%s:%s\\n' \"$$\" \"$LIFECYCLE_VALUE\" >> \"$LAUNCHES_FILE\"; sleep 30",
    );
    command.env = BTreeMap::from([
        (
            "LAUNCHES_FILE".into(),
            launches.to_string_lossy().into_owned(),
        ),
        ("LIFECYCLE_VALUE".into(), "injected".into()),
    ]);
    command.restart_when_changed = vec!["[invalid".into(), "src/**/*.txt".into()];
    registry.create(command).unwrap();

    let registry = Arc::new(Mutex::new(registry));
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let task =
        spawn_lifecycle_supervisor_with_options(registry.clone(), shutdown_rx, test_options())
            .unwrap();

    wait_until(|| {
        fs::read_to_string(&launches).is_ok_and(|contents| contents.lines().count() == 1)
    })
    .await;
    fs::write(&watched, "after").unwrap();
    wait_until(|| {
        fs::read_to_string(&launches).is_ok_and(|contents| contents.lines().count() >= 2)
    })
    .await;

    let contents = fs::read_to_string(&launches).unwrap();
    let launches = contents.lines().collect::<Vec<_>>();
    assert_eq!(launches.len(), 2, "one file write should be debounced");
    assert!(launches.iter().all(|line| line.ends_with(":injected")));
    assert_ne!(launches[0], launches[1]);

    stop_supervisor(shutdown_tx, task, &registry).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn crash_loop_restarts_with_backoff_instead_of_hot_spinning() {
    let (root, mut registry) = fixture();
    let attempts = root.path().join("attempts.txt");
    let mut command = process(&root, 1, "printf x >> \"$ATTEMPTS_FILE\"; exit 7");
    command.env = BTreeMap::from([(
        "ATTEMPTS_FILE".into(),
        attempts.to_string_lossy().into_owned(),
    )]);
    command.auto_restart = true;
    registry.create(command).unwrap();

    let registry = Arc::new(Mutex::new(registry));
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let task =
        spawn_lifecycle_supervisor_with_options(registry.clone(), shutdown_rx, test_options())
            .unwrap();

    tokio::time::sleep(Duration::from_millis(760)).await;
    let count = fs::read(&attempts).unwrap_or_default().len();
    assert!(
        count >= 3,
        "expected multiple restart attempts, got {count}"
    );
    assert!(
        count <= 4,
        "exponential backoff should bound attempts, got {count}"
    );

    stop_supervisor(shutdown_tx, task, &registry).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn untrusted_yml_process_never_auto_starts_or_auto_restarts() {
    let (root, mut registry) = fixture();
    let marker = root.path().join("untrusted-started");
    let mut command = process(&root, 1, "touch \"$MARKER_FILE\"; exit 9");
    command.env = BTreeMap::from([("MARKER_FILE".into(), marker.to_string_lossy().into_owned())]);
    command.auto_restart = true;
    command.source = ProcessSource::Yml;
    let mut command = registry.create(command).unwrap();
    command.status = ProcessStatus::Crashed;
    registry.store().put_process(&command).unwrap();

    let registry = Arc::new(Mutex::new(registry));
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let task =
        spawn_lifecycle_supervisor_with_options(registry.clone(), shutdown_rx, test_options())
            .unwrap();

    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(!marker.exists());
    assert_eq!(
        registry.lock().await.get(1).unwrap().status,
        ProcessStatus::Crashed
    );

    stop_supervisor(shutdown_tx, task, &registry).await;
}
