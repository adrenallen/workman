use std::error::Error;

use workman_core::{Project, ProjectLayoutEntry, Store};

fn project(id: i64, sort_order: i64) -> Project {
    Project {
        id,
        path: format!("/tmp/project-{id}"),
        name: format!("project-{id}"),
        display_name: None,
        icon: None,
        selected: id == 1,
        sort_order,
    }
}

#[test]
fn project_folders_move_reorder_collapse_delete_and_persist() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let database = temp.path().join("workman.sqlite3");
    {
        let store = Store::open(&database)?;
        store.put_project(&project(1, 0))?;
        store.put_project(&project(2, 1))?;
        store.put_project(&project(3, 2))?;
        let first = store.create_project_folder("Worktrees")?;
        let second = store.create_project_folder("Clients")?;
        store.update_project_layout(&[
            ProjectLayoutEntry::Folder {
                id: second.id,
                project_ids: vec![3],
            },
            ProjectLayoutEntry::Project { id: 1 },
            ProjectLayoutEntry::Folder {
                id: first.id,
                project_ids: vec![2],
            },
        ])?;
        assert!(store.set_project_folder_collapsed(first.id, true)?);
    }

    let store = Store::open(&database)?;
    let folders = store.list_project_folders()?;
    assert_eq!(folders[0].name, "Clients");
    assert_eq!(folders[1].name, "Worktrees");
    assert!(folders[1].collapsed);
    assert_eq!(
        store.project_layout()?,
        vec![
            ProjectLayoutEntry::Folder {
                id: folders[0].id,
                project_ids: vec![3],
            },
            ProjectLayoutEntry::Project { id: 1 },
            ProjectLayoutEntry::Folder {
                id: folders[1].id,
                project_ids: vec![2],
            },
        ]
    );
    assert!(store.delete_project_folder(folders[0].id)?);
    assert_eq!(
        store.project_layout()?,
        vec![
            ProjectLayoutEntry::Project { id: 3 },
            ProjectLayoutEntry::Project { id: 1 },
            ProjectLayoutEntry::Folder {
                id: folders[1].id,
                project_ids: vec![2],
            },
        ],
        "deleting a folder lifts its children into the same top-level position"
    );
    assert_eq!(
        store.list_projects()?.len(),
        3,
        "folder deletion never deletes projects"
    );
    Ok(())
}

#[test]
fn project_layout_rejects_missing_duplicate_and_foreign_ids() -> Result<(), Box<dyn Error>> {
    let store = Store::open_in_memory()?;
    store.put_project(&project(1, 0))?;
    store.put_project(&project(2, 1))?;
    let folder = store.create_project_folder("Group")?;

    for invalid in [
        vec![ProjectLayoutEntry::Folder {
            id: folder.id,
            project_ids: vec![1],
        }],
        vec![
            ProjectLayoutEntry::Folder {
                id: folder.id,
                project_ids: vec![1, 1],
            },
            ProjectLayoutEntry::Project { id: 2 },
        ],
        vec![
            ProjectLayoutEntry::Folder {
                id: folder.id + 99,
                project_ids: vec![1],
            },
            ProjectLayoutEntry::Project { id: 2 },
        ],
    ] {
        assert!(store.update_project_layout(&invalid).is_err());
    }
    Ok(())
}

#[test]
fn migration_0025_preserves_existing_flat_profile_order() -> Result<(), Box<dyn Error>> {
    let connection = rusqlite::Connection::open_in_memory()?;
    connection.execute_batch(
        "PRAGMA foreign_keys = ON;
         CREATE TABLE profiles (id INTEGER PRIMARY KEY);
         CREATE TABLE projects (id INTEGER PRIMARY KEY);
         CREATE TABLE profile_projects (
             profile_id INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
             project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
             sort_order INTEGER NOT NULL DEFAULT 0,
             selected INTEGER NOT NULL DEFAULT 0,
             PRIMARY KEY (profile_id, project_id)
         );
         INSERT INTO profiles VALUES (1);
         INSERT INTO projects VALUES (4), (7);
         INSERT INTO profile_projects VALUES (1, 4, 0, 1), (1, 7, 1, 0);",
    )?;
    connection.execute_batch(include_str!("../migrations/0025_project_folders.sql"))?;
    let memberships = connection
        .prepare(
            "SELECT project_id, sort_order, folder_id
             FROM profile_projects ORDER BY sort_order, project_id",
        )?
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, Option<i64>>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    assert_eq!(memberships, vec![(4, 0, None), (7, 1, None)]);
    Ok(())
}

#[test]
fn copying_a_profile_remaps_folder_identity_and_membership() -> Result<(), Box<dyn Error>> {
    let store = Store::open_in_memory()?;
    store.put_project(&project(1, 0))?;
    store.put_project(&project(2, 1))?;
    let source_folder = store.create_project_folder("Clients")?;
    store.update_project_layout(&[ProjectLayoutEntry::Folder {
        id: source_folder.id,
        project_ids: vec![2, 1],
    }])?;
    store.set_project_folder_collapsed(source_folder.id, true)?;

    let (copy, _) = store.create_profile("Copy", true)?;
    store.switch_profile(copy.id)?;
    let copied_folder = store.list_project_folders()?.remove(0);
    assert_ne!(copied_folder.id, source_folder.id);
    assert_eq!(copied_folder.name, source_folder.name);
    assert!(copied_folder.collapsed);
    assert_eq!(
        store.project_layout()?,
        vec![ProjectLayoutEntry::Folder {
            id: copied_folder.id,
            project_ids: vec![2, 1],
        }]
    );
    Ok(())
}
