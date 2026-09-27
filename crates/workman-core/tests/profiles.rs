use workman_core::{AgentTool, AgentToolSource, Project, Store};

fn project(id: i64, path: &str, selected: bool, sort_order: i64) -> Project {
    Project {
        id,
        path: path.into(),
        name: path.rsplit('/').next().unwrap_or("project").into(),
        display_name: None,
        icon: None,
        selected,
        sort_order,
    }
}

#[test]
fn migration_0024_wraps_existing_state_in_default_without_reordering() {
    let connection = rusqlite::Connection::open_in_memory().unwrap();
    connection
        .execute_batch(
            "PRAGMA foreign_keys = ON;
             CREATE TABLE projects (
                id INTEGER PRIMARY KEY, path TEXT NOT NULL UNIQUE, name TEXT NOT NULL,
                display_name TEXT, icon TEXT, selected INTEGER NOT NULL DEFAULT 0,
                sort_order INTEGER NOT NULL DEFAULT 0
             );
             CREATE TABLE agent_tools (
                id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE, command TEXT NOT NULL,
                tool_type TEXT NOT NULL, enabled INTEGER NOT NULL DEFAULT 1,
                source TEXT NOT NULL DEFAULT 'local', sort_order INTEGER NOT NULL DEFAULT 0,
                resume_args TEXT, continue_args TEXT
             );
             INSERT INTO projects VALUES
                (7, '/tmp/second', 'second', NULL, NULL, 0, 1),
                (4, '/tmp/first', 'first', NULL, NULL, 1, 0);
             INSERT INTO agent_tools VALUES
                (3, 'Codex', 'codex', 'codex', 1, 'config', 0, NULL, NULL);",
        )
        .unwrap();
    connection
        .execute_batch(include_str!("../migrations/0024_profiles.sql"))
        .unwrap();

    let profile: (String, bool) = connection
        .query_row("SELECT name, active FROM profiles", [], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .unwrap();
    assert_eq!(profile, ("Default".into(), true));
    let membership = connection
        .prepare(
            "SELECT project_id, selected FROM profile_projects
             WHERE profile_id = 1 ORDER BY sort_order",
        )
        .unwrap()
        .query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, bool>(1)?))
        })
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    assert_eq!(membership, vec![(4, true), (7, false)]);
    let tool_profile: (i64, String) = connection
        .query_row(
            "SELECT profile_id, display_name FROM agent_tools WHERE id = 3",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(tool_profile, (1, "Codex".into()));
}

#[test]
fn profile_switches_isolate_membership_and_agent_tools_while_projects_remain_canonical() {
    let store = Store::open_in_memory().unwrap();
    store
        .put_project(&project(10, "/tmp/original", true, 0))
        .unwrap();
    let original_tool_ids = store
        .list_agent_tools()
        .unwrap()
        .into_iter()
        .map(|tool| tool.id)
        .collect::<Vec<_>>();

    let (copy, icon_pairs) = store.create_profile("Work", true).unwrap();
    assert_eq!(icon_pairs.len(), original_tool_ids.len());
    store.switch_profile(copy.id).unwrap();
    assert_eq!(store.list_projects().unwrap()[0].path, "/tmp/original");
    let copied_tool_ids = store
        .list_agent_tools()
        .unwrap()
        .into_iter()
        .map(|tool| tool.id)
        .collect::<Vec<_>>();
    assert!(
        copied_tool_ids
            .iter()
            .all(|id| !original_tool_ids.contains(id))
    );

    let (vanilla, _) = store.create_profile("Recording", false).unwrap();
    store.switch_profile(vanilla.id).unwrap();
    assert!(store.list_projects().unwrap().is_empty());
    assert!(store.list_agent_tools().unwrap().is_empty());
    store
        .put_project(&project(11, "/tmp/throwaway", true, 0))
        .unwrap();
    store
        .put_agent_tool(&AgentTool {
            id: store.next_agent_tool_id().unwrap(),
            name: "Demo agent".into(),
            command: "demo-agent".into(),
            tool_type: "custom".into(),
            enabled: true,
            source: AgentToolSource::Config,
            resume_args: None,
            continue_args: None,
        })
        .unwrap();

    store.switch_profile(1).unwrap();
    assert_eq!(store.list_projects().unwrap()[0].path, "/tmp/original");
    assert_eq!(
        store
            .list_agent_tools()
            .unwrap()
            .into_iter()
            .map(|tool| tool.id)
            .collect::<Vec<_>>(),
        original_tool_ids
    );
    assert!(store.get_project_any(11).unwrap().is_some());
    assert!(store.get_project(11).unwrap().is_none());
}

#[test]
fn project_detach_is_profile_scoped_but_permanent_delete_is_global() {
    let store = Store::open_in_memory().unwrap();
    store
        .put_project(&project(10, "/tmp/shared", true, 0))
        .unwrap();
    let (second, _) = store.create_profile("Second", true).unwrap();

    assert!(store.delete_project(10).unwrap());
    assert!(store.get_project(10).unwrap().is_none());
    store.switch_profile(second.id).unwrap();
    assert!(store.get_project(10).unwrap().is_some());

    assert!(store.delete_project_everywhere(10).unwrap());
    assert!(store.get_project_any(10).unwrap().is_none());
    store.switch_profile(1).unwrap();
    assert!(store.get_project(10).unwrap().is_none());
}
