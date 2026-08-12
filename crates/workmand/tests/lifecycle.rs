#![cfg(unix)]

use std::{
    collections::BTreeMap,
    fs,
    sync::Arc,
    time::{Duration, Instant},
};

use tempfile::TempDir;
use tokio::{
    sync::{Mutex, watch},
    task::JoinHandle,
    time::timeout,
};
use workman_core::{Process, ProcessKind, ProcessSource, ProcessStatus, Project, Store};
use workmand::{
    LifecycleOptions, ProcessRegistry, SharedProcessRegistry, auto_start_project,
    spawn_lifecycle_supervisor_with_options, sync_workman_yml_file, trust_hash_for_process,
};

const TEST_PROGRESS_TIMEOUT: Duration = Duration::from_secs(15);
const TEST_POLL_INTERVAL: Duration = Duration::from_millis(10);

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
            sort_order: 0,
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
        spawned_by_process_id: None,
        sort_order: 0,
    }
}

async fn wait_until(description: &str, mut predicate: impl FnMut() -> bool) -> Result<(), String> {
    timeout(TEST_PROGRESS_TIMEOUT, async {
        while !predicate() {
            tokio::time::sleep(TEST_POLL_INTERVAL).await;
        }
    })
    .await
    .map_err(|_| format!("timed out waiting for {description}"))
}

fn attempt_count(path: &std::path::Path) -> usize {
    fs::read(path).unwrap_or_default().len()
}

async fn wait_for_attempt_count(path: &std::path::Path, minimum: usize) -> Result<usize, String> {
    wait_until(&format!("restart attempt {minimum}"), || {
        attempt_count(path) >= minimum
    })
    .await?;
    Ok(attempt_count(path))
}

fn acknowledge_attempt(directory: &std::path::Path, attempt: usize) -> Result<(), String> {
    fs::write(directory.join(attempt.to_string()), b"ok")
        .map_err(|error| format!("could not acknowledge attempt {attempt}: {error}"))
}

struct SupervisorGuard {
    shutdown_tx: Option<watch::Sender<bool>>,
    task: Option<JoinHandle<()>>,
    registry: SharedProcessRegistry,
}

impl SupervisorGuard {
    fn spawn(registry: SharedProcessRegistry, options: LifecycleOptions) -> Self {
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let task = spawn_lifecycle_supervisor_with_options(registry.clone(), shutdown_rx, options)
            .unwrap();
        Self {
            shutdown_tx: Some(shutdown_tx),
            task: Some(task),
            registry,
        }
    }

    async fn stop(mut self) {
        if let Some(shutdown_tx) = self.shutdown_tx.take() {
            let _ = shutdown_tx.send(true);
        }
        if let Some(task) = self.task.take() {
            task.await.unwrap();
        }
        stop_all(&self.registry).await;
    }
}

impl Drop for SupervisorGuard {
    fn drop(&mut self) {
        if let Some(shutdown_tx) = self.shutdown_tx.take() {
            let _ = shutdown_tx.send(true);
        }
        if let Some(task) = self.task.take() {
            task.abort();
        }
        if let Ok(mut registry) = self.registry.try_lock() {
            let process_ids = registry
                .list(None)
                .unwrap_or_default()
                .into_iter()
                .map(|process| process.id)
                .collect::<Vec<_>>();
            for process_id in process_ids {
                let _ = registry.kill(process_id);
            }
        }
    }
}

async fn stop_all(registry: &SharedProcessRegistry) {
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
async fn restart_on_exit_strictly_follows_auto_restart_policy() {
    let (root, mut registry) = fixture();
    let watcher_ready = root.path().join("watcher-ready");
    let mut sentinel = process(&root, 99, "touch \"$WATCHER_READY\"; sleep 30");
    sentinel.env = BTreeMap::from([(
        "WATCHER_READY".into(),
        watcher_ready.to_string_lossy().into_owned(),
    )]);
    registry.create(sentinel).unwrap();

    let registry = Arc::new(Mutex::new(registry));
    let supervisor = SupervisorGuard::spawn(registry.clone(), test_options());
    wait_until("project watcher initialization", || watcher_ready.exists())
        .await
        .unwrap();

    let one_shot_attempts = root.path().join("one-shot-attempts");
    let one_shot_yaml = format!(
        "processes:\n  one-shot:\n    command: 'printf x >> \"{}\"; exit 7'\n    auto_start: true\n",
        one_shot_attempts.display()
    );
    fs::write(root.path().join("workman.yml"), &one_shot_yaml).unwrap();
    let one_shot = {
        let mut registry = registry.lock().await;
        sync_workman_yml_file(&mut registry, 1).unwrap();
        let pending = registry
            .list(Some(1))
            .unwrap()
            .into_iter()
            .find(|process| process.name == "one-shot")
            .unwrap();
        let hash = trust_hash_for_process(&pending);
        registry.trust_yml_process(pending.id, &hash).unwrap()
    };
    wait_for_attempt_count(&one_shot_attempts, 1).await.unwrap();

    tokio::time::sleep(Duration::from_millis(600)).await;
    assert_eq!(
        attempt_count(&one_shot_attempts),
        1,
        "passive config reconciliation must not restart a one-shot command"
    );
    assert_eq!(
        registry.lock().await.get(one_shot.id).unwrap().status,
        ProcessStatus::Crashed
    );
    {
        let mut registry = registry.lock().await;
        assert!(auto_start_project(&mut registry, 1).unwrap().is_empty());
    }
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        attempt_count(&one_shot_attempts),
        1,
        "project auto-start must not override restart-on-exit policy"
    );

    let restarting_attempts = root.path().join("restarting-attempts");
    let restarting_yaml = format!(
        "{one_shot_yaml}  restarting:\n    command: 'printf x >> \"{}\"; exit 0'\n    auto_start: true\n    auto_restart: true\n",
        restarting_attempts.display()
    );
    fs::write(root.path().join("workman.yml"), restarting_yaml).unwrap();
    {
        let mut registry = registry.lock().await;
        sync_workman_yml_file(&mut registry, 1).unwrap();
        let pending = registry
            .list(Some(1))
            .unwrap()
            .into_iter()
            .find(|process| process.name == "restarting")
            .unwrap();
        let hash = trust_hash_for_process(&pending);
        registry.trust_yml_process(pending.id, &hash).unwrap();
    }
    wait_for_attempt_count(&restarting_attempts, 2)
        .await
        .unwrap();
    assert_eq!(
        attempt_count(&one_shot_attempts),
        1,
        "syncing another command must not restart an exited one-shot"
    );

    supervisor.stop().await;
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
    let supervisor = SupervisorGuard::spawn(registry.clone(), test_options());
    let outcome = async {
        wait_until("initial watched command launch", || {
            fs::read_to_string(&launches).is_ok_and(|contents| contents.lines().count() == 1)
        })
        .await?;
        fs::write(&watched, "after")
            .map_err(|error| format!("could not update watched fixture: {error}"))?;
        wait_until("debounced watched command restart", || {
            fs::read_to_string(&launches).is_ok_and(|contents| contents.lines().count() >= 2)
        })
        .await?;
        fs::read_to_string(&launches)
            .map_err(|error| format!("could not read launch records: {error}"))
    }
    .await;
    supervisor.stop().await;

    let contents = outcome.expect("watched command lifecycle did not make progress");
    let launches = contents.lines().collect::<Vec<_>>();
    assert_eq!(launches.len(), 2, "one file write should be debounced");
    assert!(launches.iter().all(|line| line.ends_with(":injected")));
    assert_ne!(launches[0], launches[1]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn crash_loop_restarts_with_backoff_instead_of_hot_spinning() {
    let (root, mut registry) = fixture();
    let attempts = root.path().join("attempts.txt");
    let acknowledgements = root.path().join("attempt-acknowledgements");
    fs::create_dir(&acknowledgements).unwrap();
    let mut command = process(
        &root,
        1,
        "printf x >> \"$ATTEMPTS_FILE\"; attempt=$(( $(/usr/bin/wc -c < \"$ATTEMPTS_FILE\") )); while [ ! -f \"$ATTEMPTS_ACK_DIR/$attempt\" ]; do /bin/sleep 0.01; done; exit 7",
    );
    command.env = BTreeMap::from([
        (
            "ATTEMPTS_FILE".into(),
            attempts.to_string_lossy().into_owned(),
        ),
        (
            "ATTEMPTS_ACK_DIR".into(),
            acknowledgements.to_string_lossy().into_owned(),
        ),
    ]);
    command.auto_restart = true;
    registry.create(command).unwrap();

    let registry = Arc::new(Mutex::new(registry));
    let options = test_options();
    let supervisor = SupervisorGuard::spawn(registry.clone(), options.clone());
    let outcome = async {
        let first = wait_for_attempt_count(&attempts, 1).await?;
        acknowledge_attempt(&acknowledgements, first)?;
        let first_acknowledged_at = Instant::now();

        let second = wait_for_attempt_count(&attempts, 2).await?;
        let first_restart_elapsed = first_acknowledged_at.elapsed();
        acknowledge_attempt(&acknowledgements, second)?;
        let second_acknowledged_at = Instant::now();

        let third = wait_for_attempt_count(&attempts, 3).await?;
        let second_restart_elapsed = second_acknowledged_at.elapsed();
        Ok::<_, String>((third, first_restart_elapsed, second_restart_elapsed))
    }
    .await;
    supervisor.stop().await;

    let (count, first_restart_elapsed, second_restart_elapsed) =
        outcome.expect("crash-loop lifecycle did not make progress");
    assert_eq!(
        count, 3,
        "the acknowledgement handshake serializes attempts"
    );
    assert!(
        first_restart_elapsed >= options.restart_backoff_initial,
        "first restart arrived after {first_restart_elapsed:?}, before {:?}",
        options.restart_backoff_initial
    );
    assert!(
        second_restart_elapsed >= options.restart_backoff_initial.saturating_mul(2),
        "second restart arrived after {second_restart_elapsed:?}, before {:?}",
        options.restart_backoff_initial.saturating_mul(2)
    );
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
    let supervisor = SupervisorGuard::spawn(registry.clone(), test_options());

    tokio::time::sleep(Duration::from_millis(500)).await;
    let marker_exists = marker.exists();
    let status = registry.lock().await.get(1).unwrap().status;
    supervisor.stop().await;

    assert!(!marker_exists);
    assert_eq!(status, ProcessStatus::Crashed);
}
