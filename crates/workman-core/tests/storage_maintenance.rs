use workman_core::{Project, Store};

const DAY: i64 = 86_400_000;
fn fixture() -> Store {
    let store = Store::open_in_memory().unwrap();
    store
        .put_project(&Project {
            id: 1,
            path: "/tmp/maintenance".into(),
            name: "Maintenance".into(),
            display_name: None,
            icon: None,
            selected: true,
            sort_order: 0,
        })
        .unwrap();
    store.connection().execute_batch("INSERT INTO processes(id, project_id, kind, name, working_dir, source, status) VALUES (1,1,'agent','live','/tmp/maintenance','local','running');").unwrap();
    store
}

#[test]
fn housekeeping_expires_transient_data_without_deleting_user_or_live_state() {
    let store = fixture();
    let now = 100 * DAY;
    let sql = store.connection();
    for (id, created, read) in [
        (1, DAY, None),
        (2, 60 * DAY, Some(60 * DAY)),
        (3, 95 * DAY, None),
        (4, 60 * DAY, Some(99 * DAY)),
    ] {
        sql.execute("INSERT INTO notifications(id,type,project_id,body,created_at,read_at) VALUES (?1,'agent_done',1,'done',?2,?3)", rusqlite::params![id, created, read]).unwrap();
    }
    sql.execute_batch("INSERT INTO actors VALUES ('old', 'old-session', NULL, 1, 1, 1), ('live', 'live-session', 1, 1, 1, 1);
        INSERT INTO timers (id,owner_actor,delivery_process_id,body,kind,fired,fired_at,created_at) VALUES (1,'live',1,'old','delay',1,1,1), (2,'live',1,'active','delay',0,NULL,1);
        INSERT INTO scratchpads(id,project_id,name,content) VALUES(1,1,'Keep forever','User notes');").unwrap();
    assert_eq!(store.maintain_storage(now).unwrap(), 4);
    let ids: Vec<i64> = sql
        .prepare("SELECT id FROM notifications ORDER BY id")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(ids, vec![3, 4]);
    assert!(store.get_actor("old").unwrap().is_none());
    assert!(store.get_actor("live").unwrap().is_some());
    assert!(store.get_timer(1).unwrap().is_none());
    assert!(store.get_timer(2).unwrap().is_some());
    let content: String = sql
        .query_row("SELECT content FROM scratchpads WHERE id=1", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(content, "User notes");
    assert!(store.get_process(1).unwrap().is_some());
    assert_eq!(store.maintain_storage(now).unwrap(), 0);
}

#[test]
fn project_data_is_removed_only_after_the_last_profile_detaches() {
    let store = fixture();
    store.connection().execute_batch("INSERT INTO scratchpads(id,project_id,name,content) VALUES(1,1,'Notes','Keep with project'); INSERT INTO notifications(type,project_id,body,created_at) VALUES('agent_done',1,'Done',1);").unwrap();
    let (other, _) = store.create_profile("Other", true).unwrap();
    store.delete_project(1).unwrap();
    assert!(store.get_project_any(1).unwrap().is_some());
    store.switch_profile(other.id).unwrap();
    assert!(store.get_process(1).unwrap().is_some());
    store.delete_project(1).unwrap();
    assert!(store.get_project_any(1).unwrap().is_none());
    for table in ["notifications", "processes", "scratchpads"] {
        let count: i64 = store
            .connection()
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 0, "{table}");
    }
}

#[test]
fn profile_deletion_collects_only_its_unshared_projects() {
    let store = fixture();
    let (other, _) = store.create_profile("Other", false).unwrap();
    store.switch_profile(other.id).unwrap();
    store.delete_profile(1).unwrap();
    assert!(store.get_project_any(1).unwrap().is_none());
}

#[test]
fn cleanup_batches_are_bounded() {
    let store = fixture();
    store.connection().execute_batch("WITH RECURSIVE ids(id) AS (VALUES(1) UNION ALL SELECT id+1 FROM ids WHERE id<1200) INSERT INTO notifications(id,type,body,created_at) SELECT id,'agent_done','Old',1 FROM ids;").unwrap();
    assert_eq!(store.maintain_storage(100 * DAY).unwrap(), 1000);
    assert_eq!(store.maintain_storage(100 * DAY).unwrap(), 200);
    assert_eq!(store.maintain_storage(100 * DAY).unwrap(), 0);
}

#[test]
fn pruning_does_not_recycle_notification_or_timer_ids() {
    let store = fixture();
    store.connection().execute_batch("INSERT INTO notifications(type,body,created_at) VALUES('agent_done','Old',1);
      INSERT INTO timers(id,owner_actor,delivery_process_id,body,kind,fired,fired_at,created_at) VALUES(99,'actor',1,'Old','delay',1,1,1);").unwrap();
    let old = store.connection().last_insert_rowid();
    assert_eq!(old, 99);
    store.maintain_storage(100 * DAY).unwrap();
    store
        .connection()
        .execute(
            "INSERT INTO notifications(type,body,created_at) VALUES('agent_done','New',?1)",
            [100 * DAY],
        )
        .unwrap();
    assert_eq!(store.connection().last_insert_rowid(), 2);
    let next_timer: i64 = store
        .connection()
        .query_row("SELECT next_id FROM timer_id_sequence", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(next_timer, 100);
}

#[test]
fn feedback_ids_do_not_reuse_media_directories_after_deletion() {
    let store = fixture();
    let service = workman_core::RecordedFeedbackService::new(&store);
    let first = service.create(1, "First", "actor", 100, 1).unwrap();
    service.delete(1, first.id).unwrap();
    let second = service.create(1, "Second", "actor", 200, 2).unwrap();
    assert!(second.id > first.id);
}
