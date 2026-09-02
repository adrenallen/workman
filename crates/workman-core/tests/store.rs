use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

use workman_core::{
    ActiveWorktreeRemoval, Actor, AgentLaunchMode, AgentTemplate, AgentTool, AgentToolSource,
    LATEST_SCHEMA_VERSION, Process, ProcessKind, ProcessSource, ProcessStatus, Project,
    ProjectLock, ProjectWorktree, Scratchpad, Store, Timer, TimerKind, Todo, TodoBlocker,
    TodoComment, TodoPriority, TodoStatus, WorktreeRepository,
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
            "active_worktree_removals",
            "actors",
            "agent_notifications",
            "agent_templates",
            "agent_tools",
            "consumed_idle_watches",
            "locks",
            "notifications",
            "process_agent_sessions",
            "process_mcp_tokens",
            "processes",
            "profile_projects",
            "profiles",
            "project_folders",
            "project_worktrees",
            "projects",
            "quick_prompts",
            "recorded_feedback",
            "recorded_feedback_deliveries",
            "recorded_feedback_snapshots",
            "schema_migrations",
            "scratchpad_comments",
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
fn active_worktree_removal_journal_round_trips_and_clears() {
    let store = Store::open_in_memory().expect("open store");
    store
        .put_active_worktree_removal(&ActiveWorktreeRemoval {
            project_id: 42,
            phase: "processes".into(),
            delete_from_disk: true,
        })
        .expect("journal removal");
    assert!(
        store
            .update_active_worktree_removal_phase(42, "files")
            .expect("advance journal")
    );
    assert_eq!(
        store.list_active_worktree_removals().expect("list journal"),
        vec![ActiveWorktreeRemoval {
            project_id: 42,
            phase: "files".into(),
            delete_from_disk: true,
        }]
    );
    assert!(
        store
            .delete_active_worktree_removal(42)
            .expect("clear journal")
    );
    assert!(store.list_active_worktree_removals().unwrap().is_empty());
}

#[test]
fn agent_templates_are_profile_scoped_reorderable_and_cascade_with_their_tool() {
    let store = Store::open_in_memory().expect("open store");
    let profile_id = store.active_profile_id().unwrap();
    let tools = store.list_agent_tools().unwrap();
    let first_tool = &tools[0];
    let second_tool = &tools[1];
    let first = AgentTemplate {
        id: store.next_agent_template_id().unwrap(),
        profile_id,
        name: "Review".into(),
        agent_tool_id: first_tool.id,
        extra_args: vec!["--model".into(), "fast model".into()],
        prompt: "Review the change carefully.".into(),
        sort_order: 0,
        created_at: 0,
        updated_at: 0,
    };
    store.put_agent_template(&first).unwrap();
    let second = AgentTemplate {
        id: store.next_agent_template_id().unwrap(),
        profile_id,
        name: "Implement".into(),
        agent_tool_id: second_tool.id,
        extra_args: Vec::new(),
        prompt: "Implement the requested change.".into(),
        sort_order: 0,
        created_at: 0,
        updated_at: 0,
    };
    store.put_agent_template(&second).unwrap();

    let listed = store.list_agent_templates().unwrap();
    assert_eq!(listed.len(), 2);
    assert_eq!(listed[0].extra_args, first.extra_args);
    assert_eq!(
        store.get_agent_template(first.id).unwrap().unwrap().prompt,
        first.prompt
    );
    store
        .reorder_agent_templates(&[second.id, first.id])
        .unwrap();
    assert_eq!(
        store
            .list_agent_templates()
            .unwrap()
            .into_iter()
            .map(|template| template.id)
            .collect::<Vec<_>>(),
        vec![second.id, first.id]
    );
    assert!(store.reorder_agent_templates(&[first.id]).is_err());

    let (other_profile, _) = store.create_profile("Other", false).unwrap();
    assert!(
        store
            .list_profile_agent_templates(other_profile.id)
            .unwrap()
            .is_empty()
    );
    assert!(store.delete_agent_tool(first_tool.id).unwrap());
    assert!(store.get_agent_template(first.id).unwrap().is_none());
    assert_eq!(
        store.list_agent_templates().unwrap(),
        vec![store.get_agent_template(second.id).unwrap().unwrap()]
    );
    assert!(store.delete_agent_template(second.id).unwrap());
    assert!(store.list_agent_templates().unwrap().is_empty());
}

#[test]
fn copied_profiles_remap_agent_templates_and_keep_each_side_independent() {
    let store = Store::open_in_memory().expect("open store");
    let source_profile_id = store.active_profile_id().unwrap();
    let source_tools = store.list_agent_tools().unwrap();
    let source_templates = [
        AgentTemplate {
            id: store.next_agent_template_id().unwrap(),
            profile_id: source_profile_id,
            name: "Review".into(),
            agent_tool_id: source_tools[0].id,
            extra_args: vec!["--model".into(), "fast model".into()],
            prompt: "Review the change carefully.".into(),
            sort_order: 0,
            created_at: 0,
            updated_at: 0,
        },
        AgentTemplate {
            id: store.next_agent_template_id().unwrap() + 1,
            profile_id: source_profile_id,
            name: "Implement".into(),
            agent_tool_id: source_tools[1].id,
            extra_args: vec!["--effort".into(), "high".into()],
            prompt: "Implement the requested change.".into(),
            sort_order: 1,
            created_at: 0,
            updated_at: 0,
        },
    ];
    for template in &source_templates {
        store.put_agent_template(template).unwrap();
    }
    let source_templates = store.list_agent_templates().unwrap();

    let (copy, tool_id_pairs) = store.create_profile("Copy", true).unwrap();
    let tool_id_map = tool_id_pairs.into_iter().collect::<BTreeMap<_, _>>();
    let copied_templates = store.list_profile_agent_templates(copy.id).unwrap();
    assert_eq!(copied_templates.len(), source_templates.len());
    for (source, copied) in source_templates.iter().zip(&copied_templates) {
        assert_ne!(copied.id, source.id);
        assert_eq!(copied.profile_id, copy.id);
        assert_eq!(copied.agent_tool_id, tool_id_map[&source.agent_tool_id]);
        assert_eq!(copied.name, source.name);
        assert_eq!(copied.extra_args, source.extra_args);
        assert_eq!(copied.prompt, source.prompt);
        assert_eq!(copied.sort_order, source.sort_order);
    }

    let mut edited_source = source_templates[0].clone();
    edited_source.name = "Source review".into();
    edited_source.prompt = "Source-only edit".into();
    store.put_agent_template(&edited_source).unwrap();
    assert_eq!(
        store.list_profile_agent_templates(copy.id).unwrap()[0],
        copied_templates[0]
    );
    assert!(store.delete_agent_template(source_templates[0].id).unwrap());
    assert_eq!(
        store.list_profile_agent_templates(copy.id).unwrap()[0],
        copied_templates[0]
    );

    store.switch_profile(copy.id).unwrap();
    let mut edited_copy = copied_templates[1].clone();
    edited_copy.name = "Copied implementation".into();
    edited_copy.extra_args.push("--copied-only".into());
    store.put_agent_template(&edited_copy).unwrap();
    assert_eq!(
        store
            .list_profile_agent_templates(source_profile_id)
            .unwrap()[0],
        source_templates[1]
    );
    assert!(store.delete_agent_template(copied_templates[1].id).unwrap());
    assert_eq!(
        store
            .list_profile_agent_templates(source_profile_id)
            .unwrap()[0],
        source_templates[1]
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
        ("Grok", "grok --always-approve"),
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
             INSERT INTO agent_tools (
                name, display_name, command, tool_type, enabled, source, sort_order, profile_id
             ) VALUES
                ('Custom Codex', 'Custom Codex', 'codex', 'codex', 0, 'local', 43, 1),
                ('Claude custom', 'Claude custom', 'claude --safe', 'claude_code', 1, 'local', 44, 1);",
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
fn grok_preset_migration_seeds_each_profile_without_replacing_a_custom_grok() {
    let store = Store::open_in_memory().expect("open store");
    store
        .connection()
        .execute_batch(
            "INSERT INTO profiles (id, name, active) VALUES (2, 'Custom', 0), (3, 'Empty', 0);
             INSERT INTO agent_tools (
                id, name, display_name, command, tool_type, enabled, source, sort_order, profile_id
             ) VALUES (
                100, 'profile-2-tool-100', 'gRoK', 'grok --model private', 'grok', 1, 'local', 0, 2
             );",
        )
        .expect("arrange profiles");

    store
        .connection()
        .execute_batch(include_str!("../migrations/0026_grok_agent_preset.sql"))
        .expect("reapply Grok preset migration");

    let profile_two = store.list_profile_agent_tools(2).unwrap();
    assert_eq!(profile_two.len(), 1);
    assert_eq!(profile_two[0].command, "grok --model private");
    let profile_three = store.list_profile_agent_tools(3).unwrap();
    assert_eq!(profile_three.len(), 1);
    assert_eq!(profile_three[0].name, "Grok");
    assert_eq!(profile_three[0].command, "grok --always-approve");
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
fn process_ownership_migration_backfills_actor_links_and_timer_fallback() {
    let connection = rusqlite::Connection::open_in_memory().expect("open connection");
    connection
        .execute_batch(
            "CREATE TABLE processes (id INTEGER PRIMARY KEY);
             CREATE TABLE actors (id TEXT PRIMARY KEY, process_id INTEGER);
             CREATE TABLE timers (
                id INTEGER PRIMARY KEY,
                owner_actor TEXT NOT NULL,
                delivery_process_id INTEGER NOT NULL
             );
             CREATE TABLE todos (
                id INTEGER PRIMARY KEY,
                lock_actor TEXT
             );
             CREATE TABLE locks (
                project_id INTEGER NOT NULL,
                key TEXT NOT NULL,
                owner_actor TEXT NOT NULL,
                PRIMARY KEY (project_id, key)
             );
             INSERT INTO processes(id) VALUES (10), (11), (20), (30);
             INSERT INTO actors(id, process_id) VALUES
                ('timer-owner', 10), ('todo-owner', 20), ('lease-owner', 30);
             INSERT INTO timers(id, owner_actor, delivery_process_id) VALUES
                (1, 'timer-owner', 11), (2, 'missing-actor', 11);
             INSERT INTO todos(id, lock_actor) VALUES
                (3, 'todo-owner'), (4, 'missing-actor'), (5, NULL);
             INSERT INTO locks(project_id, key, owner_actor) VALUES
                (1, 'mapped', 'lease-owner'), (1, 'fallback', 'missing-actor');",
        )
        .expect("create legacy ownership rows");

    connection
        .execute_batch(include_str!("../migrations/0027_process_ownership.sql"))
        .expect("apply process ownership migration");

    let timers = connection
        .prepare("SELECT id, owner_process_id FROM timers ORDER BY id")
        .unwrap()
        .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    assert_eq!(timers, [(1, 10), (2, 11)]);

    let todos = connection
        .prepare("SELECT id, lock_process_id FROM todos ORDER BY id")
        .unwrap()
        .query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, Option<i64>>(1)?))
        })
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    assert_eq!(todos, [(3, Some(20)), (4, None), (5, None)]);

    let locks = connection
        .prepare("SELECT key, owner_process_id FROM locks ORDER BY key")
        .unwrap()
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<i64>>(1)?))
        })
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    assert_eq!(
        locks,
        [("fallback".into(), None), ("mapped".into(), Some(30))]
    );
}

#[test]
fn unique_agent_session_migration_clears_ambiguous_legacy_ids() {
    let connection = rusqlite::Connection::open_in_memory().expect("open connection");
    connection
        .execute_batch(
            "CREATE TABLE process_agent_sessions (
                process_id INTEGER PRIMARY KEY,
                session_id TEXT,
                launch_mode TEXT NOT NULL,
                launched_at INTEGER NOT NULL,
                captured_at INTEGER
             );
             INSERT INTO process_agent_sessions VALUES
                (1, 'shared', 'fresh', 1, 2),
                (2, 'shared', 'fresh', 1, 2),
                (3, 'distinct', 'fresh', 1, 2);",
        )
        .expect("create legacy agent sessions");
    connection
        .execute_batch(include_str!("../migrations/0023_unique_agent_sessions.sql"))
        .expect("apply unique agent session migration");

    let sessions = connection
        .prepare("SELECT process_id, session_id FROM process_agent_sessions ORDER BY process_id")
        .unwrap()
        .query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, Option<String>>(1)?))
        })
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    assert_eq!(
        sessions,
        [(1, None), (2, None), (3, Some("distinct".into()))]
    );
    assert!(
        connection
            .execute(
                "UPDATE process_agent_sessions SET session_id = 'distinct' WHERE process_id = 1",
                [],
            )
            .is_err()
    );
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
        resume_args: None,
        continue_args: None,
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
        .set_agent_launch_mode(process.id, AgentLaunchMode::Fresh, 1_700_000_000_100)
        .expect("set agent launch mode");
    assert!(
        store
            .set_agent_session_id(process.id, "session-codex", 1_700_000_000_200)
            .expect("capture agent session")
    );
    let agent_session = store
        .get_agent_session(process.id)
        .unwrap()
        .expect("agent session");
    assert_eq!(agent_session.session_id.as_deref(), Some("session-codex"));
    assert_eq!(agent_session.launch_mode, AgentLaunchMode::Fresh);
    store
        .set_agent_launch_mode(
            process.id,
            AgentLaunchMode::ResumedSession,
            1_700_000_000_300,
        )
        .expect("update launch mode");
    let agent_session = store.get_agent_session(process.id).unwrap().unwrap();
    assert_eq!(agent_session.session_id.as_deref(), Some("session-codex"));
    assert_eq!(agent_session.launch_mode, AgentLaunchMode::ResumedSession);

    let competing_process = Process {
        id: 30,
        name: "codex-w2".into(),
        pid: Some(5678),
        ..process.clone()
    };
    store
        .put_process(&competing_process)
        .expect("put competing process");
    store
        .set_agent_launch_mode(
            competing_process.id,
            AgentLaunchMode::Fresh,
            1_700_000_000_400,
        )
        .expect("set competing launch mode");
    assert!(
        !store
            .set_agent_session_id(competing_process.id, "session-codex", 1_700_000_000_500,)
            .expect("reject duplicate agent session")
    );
    assert_eq!(
        store
            .get_agent_session(competing_process.id)
            .unwrap()
            .unwrap()
            .session_id,
        None
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
    assert_eq!(store.actor_display_label(&actor.id), "codex-w1");
    assert_eq!(store.actor_display_label("mcp-0123456789abcdef"), "session");
    assert_eq!(store.actor_display_label("desktop-ui"), "user");
    assert_eq!(store.actor_display_label("workman"), "user");

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
        lock_process_id: None,
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
        lock_process_id: actor.process_id,
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
        created_by: "codex-w1 (agent 3)".into(),
        updated_by: "codex-w1 (agent 3)".into(),
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
        owner_process_id: actor.process_id,
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
        owner_process_id: actor.process_id,
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
