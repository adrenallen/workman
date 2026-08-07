use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

use workman_core::{
    Actor, AgentTool, AgentToolSource, LATEST_SCHEMA_VERSION, Process, ProcessKind, ProcessSource,
    ProcessStatus, Project, ProjectLock, ProjectWorktree, Scratchpad, Store, Timer, TimerKind,
    Todo, TodoBlocker, TodoComment, TodoPriority, TodoStatus, WorktreeRepository,
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
            "agent_notifications",
            "agent_tools",
            "locks",
            "notifications",
            "process_mcp_tokens",
            "processes",
            "project_worktrees",
            "projects",
            "schema_migrations",
            "scratchpad_tags",
            "scratchpads",
            "timer_runtime",
            "timers",
            "todo_activity",
            "todo_blockers",
            "todo_comments",
            "todo_tags",
            "todos",
            "worktree_preferences",
            "worktree_repositories",
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
fn fresh_and_legacy_agent_defaults_use_yolo_commands_without_rewriting_custom_tools() {
    let store = Store::open_in_memory().expect("open store");
    let expected = [
        ("Claude", "claude --dangerously-skip-permissions"),
        ("Codex", "codex --dangerously-bypass-approvals-and-sandbox"),
        ("Gemini", "gemini --approval-mode=yolo"),
        ("OpenCode", "opencode --auto"),
        ("Kimi", "kimi --yolo"),
        (
            "DeepSeek v4 flash",
            "opencode --auto --model deepseek/deepseek-v4-flash",
        ),
    ];
    let fresh = store.list_agent_tools().expect("list fresh defaults");
    assert_eq!(fresh.len(), expected.len());
    for (name, command) in expected {
        assert_eq!(
            fresh
                .iter()
                .find(|tool| tool.name == name)
                .map(|tool| tool.command.as_str()),
            Some(command),
            "fresh {name} command"
        );
    }

    store
        .connection()
        .execute_batch(
            "UPDATE agent_tools SET command = 'claude', enabled = 0, source = 'config', sort_order = 42 WHERE name = 'Claude';
             UPDATE agent_tools SET command = 'codex' WHERE name = 'Codex';
             UPDATE agent_tools SET command = 'gemini --yolo' WHERE name = 'Gemini';
             UPDATE agent_tools SET command = 'opencode' WHERE name = 'OpenCode';
             UPDATE agent_tools SET command = 'kimi' WHERE name = 'Kimi';
             UPDATE agent_tools SET command = 'opencode --model deepseek/deepseek-v4-flash' WHERE name = 'DeepSeek v4 flash';
             INSERT INTO agent_tools (name, command, tool_type, enabled, source, sort_order)
             VALUES ('Custom Codex', 'codex', 'codex', 0, 'local', 43),
                    ('Claude custom', 'claude --safe', 'claude_code', 1, 'local', 44);",
        )
        .expect("arrange legacy defaults and custom tools");
    store
        .connection()
        .execute_batch(include_str!(
            "../migrations/0014_agent_tool_yolo_defaults.sql"
        ))
        .expect("reapply yolo repair migration");

    let migrated = store.list_agent_tools().expect("list migrated defaults");
    for (name, command) in expected {
        assert_eq!(
            migrated
                .iter()
                .find(|tool| tool.name == name)
                .map(|tool| tool.command.as_str()),
            Some(command),
            "migrated {name} command"
        );
    }
    let claude = migrated.iter().find(|tool| tool.name == "Claude").unwrap();
    assert!(!claude.enabled);
    assert_eq!(claude.source, AgentToolSource::Config);
    let claude_sort_order: i64 = store
        .connection()
        .query_row(
            "SELECT sort_order FROM agent_tools WHERE name = 'Claude'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(claude_sort_order, 42);
    assert_eq!(
        migrated
            .iter()
            .find(|tool| tool.name == "Custom Codex")
            .unwrap()
            .command,
        "codex"
    );
    assert_eq!(
        migrated
            .iter()
            .find(|tool| tool.name == "Claude custom")
            .unwrap()
            .command,
        "claude --safe"
    );

    store
        .connection()
        .execute(
            "UPDATE agent_tools SET command = 'claude --model private' WHERE name = 'Claude'",
            [],
        )
        .unwrap();
    store
        .connection()
        .execute_batch(include_str!(
            "../migrations/0014_agent_tool_yolo_defaults.sql"
        ))
        .unwrap();
    assert_eq!(
        store
            .list_agent_tools()
            .unwrap()
            .iter()
            .find(|tool| tool.name == "Claude")
            .unwrap()
            .command,
        "claude --model private"
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
        "workman-core-wal-{}-{unique}.sqlite",
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
fn worktree_preferences_survive_store_reopen() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "workman-core-worktree-preferences-{}-{unique}.sqlite",
        std::process::id()
    ));
    let repository = WorktreeRepository {
        id: 4,
        root_path: "/fixture/repository".into(),
        name: "fixture".into(),
        managed_root: "/fixture/worktrees".into(),
    };

    {
        let store = Store::open(&path).expect("open file store");
        store
            .put_worktree_repository(&repository)
            .expect("put repository");
        store
            .set_worktree_preference(repository.id, "env_policy", Some("copy"))
            .expect("set preference");
    }
    {
        let store = Store::open(&path).expect("reopen file store");
        assert_eq!(
            store.worktree_preferences(repository.id).unwrap(),
            BTreeMap::from([("env_policy".into(), "copy".into())])
        );
    }

    let _ = fs::remove_file(&path);
    let _ = fs::remove_file(with_suffix(&path, "-wal"));
    let _ = fs::remove_file(with_suffix(&path, "-shm"));
}

#[test]
fn domain_records_round_trip_through_store() {
    let mut store = Store::open_in_memory().expect("open store");

    let project = Project {
        id: 1,
        path: "/workspace/workman".into(),
        name: "workman".into(),
        display_name: Some("Workman".into()),
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

    let repository = WorktreeRepository {
        id: 1,
        root_path: project.path.clone(),
        name: "workman".into(),
        managed_root: "/workspace/worktrees".into(),
    };
    store
        .put_worktree_repository(&repository)
        .expect("put worktree repository");
    assert_eq!(
        store.get_worktree_repository(repository.id).unwrap(),
        Some(repository.clone())
    );
    assert_eq!(
        store
            .get_worktree_repository_by_root(&repository.root_path)
            .unwrap(),
        Some(repository.clone())
    );
    let worktree = ProjectWorktree {
        project_id: project.id,
        repository_id: repository.id,
        parent_project_id: None,
        branch: "main".into(),
        managed: false,
    };
    store
        .put_project_worktree(&worktree)
        .expect("put project worktree");
    assert_eq!(
        store.get_project_worktree(project.id).unwrap(),
        Some(worktree.clone())
    );
    store
        .set_worktree_preference(repository.id, "copy_env", Some("yes"))
        .expect("set worktree preference");
    assert_eq!(
        store.worktree_preferences(repository.id).unwrap(),
        BTreeMap::from([("copy_env".into(), "yes".into())])
    );

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
    let mut reordered_ids = store
        .list_agent_tools()
        .unwrap()
        .into_iter()
        .map(|tool| tool.id)
        .collect::<Vec<_>>();
    reordered_ids.reverse();
    store
        .reorder_agent_tools(&reordered_ids)
        .expect("reorder agent tools");
    assert_eq!(
        store
            .list_agent_tools()
            .unwrap()
            .into_iter()
            .map(|tool| tool.id)
            .collect::<Vec<_>>(),
        reordered_ids
    );
    assert!(store.reorder_agent_tools(&[agent_tool.id]).is_err());

    let process = Process {
        id: 3,
        project_id: project.id,
        kind: ProcessKind::Agent,
        name: "codex-w1".into(),
        command: Some("codex --full-auto".into()),
        working_dir: project.path.clone(),
        env: BTreeMap::from([
            ("WORKMAN_PROCESS_ID".into(), "3".into()),
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
