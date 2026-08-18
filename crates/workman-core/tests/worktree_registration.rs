use tempfile::tempdir;
use workman_core::{Project, ProjectWorktree, Store};

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
