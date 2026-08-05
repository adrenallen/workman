use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

use awm_core::{
    Actor, AgentTool, AgentToolSource, LATEST_SCHEMA_VERSION, Process, ProcessKind, ProcessSource,
    ProcessStatus, Project, ProjectLock, Scratchpad, Store, Timer, TimerKind, Todo, TodoBlocker,
    TodoComment, TodoPriority, TodoStatus,
};

#[test]
fn fresh_database_migrates_to_current_schema() {
    let mut store = Store::open_in_memory().expect("open store");

    assert_eq!(
        store.schema_version().expect("read schema version"),
        LATEST_SCHEMA_VERSION
    );
    let user_version: i64 = store
        .connection()
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("read user_version");
    assert_eq!(user_version, LATEST_SCHEMA_VERSION);

    let foreign_keys: bool = store
        .connection()
        .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
        .expect("read foreign_keys");
    assert!(foreign_keys);

    let mut statement = store
        .connection()
        .prepare(
            "SELECT name FROM sqlite_master
             WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
        )
        .expect("prepare table query");
    let tables = statement
        .query_map([], |row| row.get::<_, String>(0))
        .expect("query tables")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("read tables");
    assert_eq!(
        tables,
        [
            "actors",
            "agent_tools",
            "locks",
            "process_mcp_tokens",
            "processes",
            "projects",
            "schema_migrations",
            "scratchpad_tags",
            "scratchpads",
            "timer_runtime",
            "timers",
            "todo_blockers",
            "todo_comments",
            "todo_tags",
            "todos",
        ]
    );
    drop(statement);

    // Applying migrations more than once is a no-op.
    store.migrate().expect("re-run migrations");
    assert_eq!(
        store.schema_version().expect("read schema version"),
        LATEST_SCHEMA_VERSION
    );
}

#[test]
fn version_one_database_migrates_mcp_identity_schema() {
    let connection = rusqlite::Connection::open_in_memory().expect("open connection");
    connection
        .execute_batch(include_str!("../migrations/0001_initial.sql"))
        .expect("apply version one");
    connection
        .execute_batch(
            "CREATE TABLE schema_migrations (
                version INTEGER PRIMARY KEY,
                name TEXT NOT NULL,
                applied_at INTEGER NOT NULL DEFAULT (unixepoch())
             );
             INSERT INTO schema_migrations (version, name) VALUES (1, 'initial');
             PRAGMA user_version = 1;",
        )
        .expect("record version one");

    let store = Store::from_connection(connection).expect("migrate store");
    assert_eq!(store.schema_version().unwrap(), LATEST_SCHEMA_VERSION);
    let token_table_exists: bool = store
        .connection()
        .query_row(
            "SELECT EXISTS(
                SELECT 1 FROM sqlite_master
                WHERE type = 'table' AND name = 'process_mcp_tokens'
             )",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(token_table_exists);
}

#[test]
fn file_database_uses_wal() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "awm-core-wal-{}-{unique}.sqlite",
        std::process::id()
    ));

    let store = Store::open(&path).expect("open file store");
    let journal_mode: String = store
        .connection()
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .expect("read journal mode");
    assert_eq!(journal_mode, "wal");
    drop(store);

    let _ = fs::remove_file(&path);
    let _ = fs::remove_file(with_suffix(&path, "-wal"));
    let _ = fs::remove_file(with_suffix(&path, "-shm"));
}

#[test]
fn domain_records_round_trip_through_store() {
    let mut store = Store::open_in_memory().expect("open store");

    let project = Project {
        id: 1,
        path: "/workspace/awm".into(),
        name: "awm".into(),
        display_name: Some("Awm".into()),
        icon: Some("terminal".into()),
        selected: true,
        sort_order: 0,
    };
    store.put_project(&project).expect("put project");
    assert_eq!(
        store.get_project(project.id).unwrap(),
        Some(project.clone())
    );
    assert_eq!(store.list_projects().unwrap(), vec![project.clone()]);
    assert_eq!(store.next_project_id().unwrap(), 2);

    let agent_tool = AgentTool {
        id: 2,
        name: "Codex".into(),
        command: "codex".into(),
        tool_type: "codex".into(),
        enabled: true,
        source: AgentToolSource::Local,
    };
    store.put_agent_tool(&agent_tool).expect("put agent tool");
    assert_eq!(
        store.get_agent_tool(agent_tool.id).unwrap(),
        Some(agent_tool.clone())
    );

    let process = Process {
        id: 3,
        project_id: project.id,
        kind: ProcessKind::Agent,
        name: "codex-w1".into(),
        command: Some("codex --full-auto".into()),
        working_dir: project.path.clone(),
        env: BTreeMap::from([
            ("AWM_PROCESS_ID".into(), "3".into()),
            ("RUST_LOG".into(), "debug".into()),
        ]),
        auto_start: false,
        auto_restart: true,
        restart_when_changed: vec!["crates/**/*.rs".into(), "Cargo.lock".into()],
        source: ProcessSource::Local,
        trust_hash: Some("sha256:abc".into()),
        status: ProcessStatus::Running,
        pid: Some(1234),
        exit_code: None,
        exit_signal: None,
        exited_at: None,
        agent_tool_id: Some(agent_tool.id),
        spawned_by_process_id: None,
        sort_order: 0,
    };
    store.put_process(&process).expect("put process");
    assert_eq!(
        store.get_process(process.id).unwrap(),
        Some(process.clone())
    );
    store
        .set_process_mcp_token(process.id, "process-secret", 1_700_000_000_000)
        .expect("set process token");
    assert_eq!(
        store.get_process_by_mcp_token("process-secret").unwrap(),
        Some(process.clone())
    );

    let actor = Actor {
        id: "mcp-abc".into(),
        session_id: "session-abc".into(),
        process_id: Some(process.id),
        selected_project_id: Some(project.id),
        created_at: 1_700_000_000_000,
        last_seen_at: 1_700_000_000_123,
    };
    store.put_actor(&actor).expect("put actor");
    assert_eq!(store.get_actor(&actor.id).unwrap(), Some(actor.clone()));
    assert_eq!(
        store.get_actor_by_session_id(&actor.session_id).unwrap(),
        Some(actor.clone())
    );

    let prerequisite = Todo {
        id: 4,
        project_id: project.id,
        title: "Scaffold workspace".into(),
        body: String::new(),
        status: TodoStatus::Completed,
        priority: TodoPriority::High,
        completed: true,
        tags: vec!["core".into(), "m0".into()],
        lock_actor: None,
        lock_expiry: None,
    };
    store.put_todo(&prerequisite).expect("put prerequisite");
    assert_eq!(
        store.get_todo(prerequisite.id).unwrap(),
        Some(prerequisite.clone())
    );

    let todo = Todo {
        id: 5,
        project_id: project.id,
        title: "Persist state".into(),
        body: "Add SQLite.".into(),
        status: TodoStatus::InProgress,
        priority: TodoPriority::Medium,
        completed: false,
        tags: vec!["sqlite".into(), "core".into()],
        lock_actor: Some(actor.id.clone()),
        lock_expiry: Some(1_700_000_030_000),
    };
    store.put_todo(&todo).expect("put todo");
    assert_eq!(store.get_todo(todo.id).unwrap(), Some(todo.clone()));

    let blocker = TodoBlocker {
        todo_id: todo.id,
        blocked_by_todo_id: prerequisite.id,
    };
    store.put_todo_blocker(&blocker).expect("put blocker");
    assert_eq!(
        store
            .get_todo_blocker(blocker.todo_id, blocker.blocked_by_todo_id)
            .unwrap(),
        Some(blocker)
    );

    let comment = TodoComment {
        id: 6,
        todo_id: todo.id,
        actor: actor.id.clone(),
        body: "Migration is atomic.".into(),
        created_at: 1_700_000_001_000,
        updated_at: 1_700_000_001_500,
    };
    store.put_todo_comment(&comment).expect("put todo comment");
    assert_eq!(store.get_todo_comment(comment.id).unwrap(), Some(comment));

    let scratchpad = Scratchpad {
        id: 7,
        project_id: project.id,
        name: "plan".into(),
        content: "# Plan\n\nShip it.".into(),
        revision: 3,
        tags: vec!["shared".into(), "architecture".into()],
        archived: false,
    };
    store.put_scratchpad(&scratchpad).expect("put scratchpad");
    assert_eq!(
        store.get_scratchpad(scratchpad.id).unwrap(),
        Some(scratchpad)
    );

    let lock = ProjectLock {
        project_id: project.id,
        key: "schema".into(),
        owner_actor: actor.id.clone(),
        acquired_at: 1_700_000_002_000,
        ttl_ms: 30_000,
    };
    store.put_project_lock(&lock).expect("put lock");
    assert_eq!(
        store.get_project_lock(lock.project_id, &lock.key).unwrap(),
        Some(lock)
    );

    let timer = Timer {
        id: 8,
        owner_actor: actor.id.clone(),
        delivery_process_id: process.id,
        body: "Check worker status".into(),
        kind: TimerKind::IdleAll,
        watch_process_ids: vec![process.id, 99],
        interval_ms: Some(10_000),
        repeating: true,
        max_wait_deadline: Some(1_700_000_060_000),
        paused: false,
        fired: true,
        fired_at: Some(1_700_000_010_000),
        created_at: 1_700_000_000_000,
    };
    store.put_timer(&timer).expect("put timer");
    assert_eq!(store.get_timer(timer.id).unwrap(), Some(timer));

    assert!(store.smoke_test().expect("run smoke test"));
    assert!(
        store
            .clear_process_mcp_token(process.id)
            .expect("clear process token")
    );
    assert_eq!(
        store.get_process_by_mcp_token("process-secret").unwrap(),
        None
    );

    let child = Process {
        id: 9,
        name: "codex-child".into(),
        pid: Some(4321),
        spawned_by_process_id: Some(process.id),
        ..process.clone()
    };
    store.put_process(&child).expect("put child process");
    assert_eq!(store.get_process(child.id).unwrap(), Some(child.clone()));
    assert!(
        store
            .delete_process(process.id)
            .expect("delete parent process")
    );
    assert_eq!(
        store
            .get_process(child.id)
            .unwrap()
            .expect("child remains")
            .spawned_by_process_id,
        None,
        "closing a parent promotes its children by clearing their lineage foreign key"
    );
}

#[test]
fn project_catalog_can_delete_empty_projects() {
    let store = Store::open_in_memory().expect("open store");
    let project = Project {
        id: 7,
        path: "/workspace/empty".into(),
        name: "empty".into(),
        display_name: None,
        icon: None,
        selected: false,
        sort_order: 0,
    };
    store.put_project(&project).expect("put project");

    assert!(store.delete_project(project.id).expect("delete project"));
    assert_eq!(store.get_project(project.id).unwrap(), None);
    assert!(
        !store
            .delete_project(project.id)
            .expect("delete missing project")
    );
}

fn with_suffix(path: &std::path::Path, suffix: &str) -> std::path::PathBuf {
    let mut value = OsString::from(path.as_os_str());
    value.push(suffix);
    value.into()
}
