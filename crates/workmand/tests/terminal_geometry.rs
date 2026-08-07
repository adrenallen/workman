use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

use workman_core::{Process, ProcessKind, ProcessSource, ProcessStatus, Project, Store};
use workmand::ProcessRegistry;

fn process(id: i64, kind: ProcessKind, working_dir: &str) -> Process {
    Process {
        id,
        project_id: 1,
        kind,
        name: format!("geometry-{id}"),
        command: Some("stty size; exec sleep 30".into()),
        working_dir: working_dir.into(),
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
    }
}

fn wait_for_reported_size(registry: &mut ProcessRegistry, process_id: i64, rows: u16, cols: u16) {
    let expected = format!("{rows} {cols}");
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let output = registry
            .raw_output(process_id, None, usize::MAX)
            .expect("read PTY output")
            .data;
        if String::from_utf8_lossy(&output).contains(&expected) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "process {process_id} never reported {expected}; output={output:?}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn remembered_geometry_reaches_every_terminal_kind_before_first_output() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::open_in_memory().unwrap();
    store
        .put_project(&Project {
            id: 1,
            path: temp.path().to_string_lossy().into_owned(),
            name: "terminal-geometry".into(),
            display_name: None,
            icon: None,
            selected: true,
            sort_order: 0,
        })
        .unwrap();
    let mut registry = ProcessRegistry::with_stop_grace(store, Duration::from_millis(50)).unwrap();
    let working_dir = temp.path().to_string_lossy();

    for (id, kind) in [
        (61, ProcessKind::Agent),
        (62, ProcessKind::Terminal),
        (63, ProcessKind::Command),
    ] {
        registry.create(process(id, kind, &working_dir)).unwrap();

        let stopped = registry.resize(id, 37, 111, 1_110, 740).unwrap();
        assert_eq!(stopped.status, ProcessStatus::Stopped);
        registry.start(id).unwrap();
        wait_for_reported_size(&mut registry, id, 37, 111);

        registry.stop(id).unwrap();
        registry.resize(id, 43, 127, 1_270, 860).unwrap();
        registry.start(id).unwrap();
        wait_for_reported_size(&mut registry, id, 43, 127);

        // A live resize is retained by the same standard restart path.
        registry.resize(id, 51, 139, 1_390, 1_020).unwrap();
        registry.restart(id).unwrap();
        wait_for_reported_size(&mut registry, id, 51, 139);
        registry.stop(id).unwrap();
    }
}
