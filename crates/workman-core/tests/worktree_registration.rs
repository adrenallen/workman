use tempfile::tempdir;
use workman_core::{Project, ProjectWorktree, Store, WorktreeRepository};

#[test]
fn project_and_worktree_link_registration_is_atomic() {
    let fixture = tempdir().unwrap();
    let store = Store::open(fixture.path().join("state.sqlite3")).unwrap();
    let project = Project {
        id: 1,
        path: fixture
            .path()
            .join("checkout")
            .to_string_lossy()
            .into_owned(),
        name: "checkout".into(),
        display_name: None,
        icon: None,
        selected: true,
        sort_order: 0,
    };
    let invalid_link = ProjectWorktree {
        project_id: project.id,
        repository_id: 999,
        parent_project_id: None,
        branch: "main".into(),
        managed: true,
    };

    store
        .put_project_with_worktree(&project, &invalid_link)
        .expect_err("a missing repository must reject the complete registration");

    assert!(store.get_project_any(project.id).unwrap().is_none());
    assert!(store.list_projects().unwrap().is_empty());
    assert!(store.get_project_worktree(project.id).unwrap().is_none());
}

#[test]
fn worktree_registration_leaves_profile_selection_untouched() {
    let store = Store::open_in_memory().unwrap();
    let legacy_selected = Project {
        id: 1,
        path: "/fixture/legacy-selected".into(),
        name: "legacy-selected".into(),
        display_name: None,
        icon: None,
        selected: true,
        sort_order: 0,
    };
    let active = Project {
        id: 2,
        path: "/fixture/active".into(),
        name: "active".into(),
        display_name: None,
        icon: None,
        selected: true,
        sort_order: 1,
    };
    store.put_project(&legacy_selected).unwrap();
    store.put_project(&active).unwrap();
    store
        .put_worktree_repository(&WorktreeRepository {
            id: 1,
            root_path: legacy_selected.path.clone(),
            name: legacy_selected.name.clone(),
            managed_root: "/fixture/worktrees".into(),
        })
        .unwrap();

    let legacy_row = store
        .get_project_by_path_any(&legacy_selected.path)
        .unwrap()
        .expect("legacy project row");
    assert!(
        legacy_row.selected,
        "the legacy projects.selected bit is set"
    );
    store
        .put_project_with_worktree(
            &legacy_row,
            &ProjectWorktree {
                project_id: legacy_row.id,
                repository_id: 1,
                parent_project_id: None,
                branch: "main".into(),
                managed: false,
            },
        )
        .expect("re-registration must not violate profile selection uniqueness");

    assert!(!store.get_project(legacy_row.id).unwrap().unwrap().selected);
    assert!(store.get_project(active.id).unwrap().unwrap().selected);
}
