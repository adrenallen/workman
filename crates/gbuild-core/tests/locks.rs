use gbuild_core::{LockService, LockServiceError, Project, Store};

const NOW: i64 = 1_800_000_000_000;

fn project(id: i64) -> Project {
    Project {
        id,
        path: format!("/workspace/project-{id}"),
        name: format!("project-{id}"),
        display_name: None,
        icon: None,
        selected: false,
        sort_order: id - 1,
    }
}

#[test]
fn leases_are_atomic_renewable_releasable_and_project_scoped() {
    let store = Store::open_in_memory().unwrap();
    store.put_project(&project(1)).unwrap();
    store.put_project(&project(2)).unwrap();
    let service = LockService::new(&store);

    let lease = service
        .acquire(1, "schema.migration", "actor-a", 1_000, NOW)
        .unwrap();
    assert_eq!(lease.owner_actor_id, "actor-a");
    assert_eq!(lease.expires_at, NOW + 1_000);
    assert!(matches!(
        service.acquire(1, "schema.migration", "actor-b", 1_000, NOW + 1),
        Err(LockServiceError::Held { owner_actor_id, .. }) if owner_actor_id == "actor-a"
    ));

    let renewed = service
        .acquire(1, "schema.migration", "actor-a", 2_000, NOW + 10)
        .unwrap();
    assert_eq!(renewed.expires_at, NOW + 2_010);
    assert!(matches!(
        service.release(1, "schema.migration", "actor-b", NOW + 11),
        Err(LockServiceError::NotOwned { owner_actor_id, .. }) if owner_actor_id == "actor-a"
    ));

    assert!(
        service
            .acquire(2, "schema.migration", "actor-b", 1_000, NOW + 11)
            .is_ok()
    );
    assert!(
        service
            .release(1, "schema.migration", "actor-a", NOW + 12)
            .unwrap()
    );
    assert_eq!(
        service.status(1, "schema.migration", NOW + 12).unwrap(),
        None
    );
    assert!(
        !service
            .release(1, "schema.migration", "actor-a", NOW + 12)
            .unwrap()
    );
}

#[test]
fn an_expired_lease_can_be_claimed_by_another_actor() {
    let store = Store::open_in_memory().unwrap();
    store.put_project(&project(1)).unwrap();
    let service = LockService::new(&store);

    service.acquire(1, "release", "actor-a", 100, NOW).unwrap();
    assert!(service.status(1, "release", NOW + 99).unwrap().is_some());
    let acquired = service
        .acquire(1, "release", "actor-b", 500, NOW + 100)
        .unwrap();
    assert_eq!(acquired.owner_actor_id, "actor-b");
    assert_eq!(acquired.expires_at, NOW + 600);
}

#[test]
fn invalid_keys_and_ttls_are_rejected() {
    let store = Store::open_in_memory().unwrap();
    store.put_project(&project(1)).unwrap();
    let service = LockService::new(&store);

    assert!(matches!(
        service.acquire(1, "Uppercase", "actor", 1_000, NOW),
        Err(LockServiceError::InvalidKey(_))
    ));
    assert!(matches!(
        service.acquire(1, "valid", "actor", 0, NOW),
        Err(LockServiceError::InvalidLeaseTtl)
    ));
}
