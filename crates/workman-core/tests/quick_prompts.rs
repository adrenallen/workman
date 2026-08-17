use workman_core::{QuickPrompt, Store};

fn prompt(id: i64, name: &str, body: &str) -> QuickPrompt {
    QuickPrompt {
        id,
        name: name.to_owned(),
        body: body.to_owned(),
        sort_order: 0,
        created_at: 0,
        updated_at: 0,
    }
}

#[test]
fn quick_prompt_crud_and_reordering_round_trip() {
    let store = Store::open_in_memory().expect("open store");
    assert!(store.list_quick_prompts().unwrap().is_empty());

    let first_id = store.next_quick_prompt_id().unwrap();
    store
        .put_quick_prompt(&prompt(
            first_id,
            "Review",
            "Review this change\nfor regressions.",
        ))
        .unwrap();
    let second_id = store.next_quick_prompt_id().unwrap();
    store
        .put_quick_prompt(&prompt(
            second_id,
            "Summarize",
            "Summarize the current state.",
        ))
        .unwrap();

    let created = store.get_quick_prompt(first_id).unwrap().unwrap();
    assert_eq!(created.name, "Review");
    assert_eq!(created.body, "Review this change\nfor regressions.");
    assert!(created.created_at > 0);
    assert!(created.updated_at > 0);

    store
        .put_quick_prompt(&prompt(first_id, "Review carefully", "Find edge cases."))
        .unwrap();
    let updated = store.get_quick_prompt(first_id).unwrap().unwrap();
    assert_eq!(updated.name, "Review carefully");
    assert_eq!(updated.body, "Find edge cases.");

    store.reorder_quick_prompts(&[second_id, first_id]).unwrap();
    assert_eq!(
        store
            .list_quick_prompts()
            .unwrap()
            .into_iter()
            .map(|candidate| candidate.id)
            .collect::<Vec<_>>(),
        vec![second_id, first_id]
    );
    assert!(store.reorder_quick_prompts(&[first_id]).is_err());
    assert!(store.delete_quick_prompt(second_id).unwrap());
    assert!(!store.delete_quick_prompt(second_id).unwrap());
}

#[test]
fn quick_prompts_are_profile_scoped_and_names_are_case_insensitively_unique() {
    let store = Store::open_in_memory().expect("open store");
    let default_profile = store.active_profile_id().unwrap();
    store
        .put_quick_prompt(&prompt(100, "Ship it", "Run the release checks."))
        .unwrap();
    assert!(
        store
            .put_quick_prompt(&prompt(101, "ship IT", "Duplicate name."))
            .is_err()
    );

    let (second, _) = store.create_profile("Second", false).unwrap();
    store.switch_profile(second.id).unwrap();
    assert!(store.list_quick_prompts().unwrap().is_empty());
    store
        .put_quick_prompt(&prompt(101, "ship IT", "Allowed in another profile."))
        .unwrap();

    assert_eq!(
        store
            .list_profile_quick_prompts(default_profile)
            .unwrap()
            .into_iter()
            .map(|candidate| candidate.id)
            .collect::<Vec<_>>(),
        vec![100]
    );
    assert_eq!(store.list_quick_prompts().unwrap()[0].id, 101);
}

#[test]
fn quick_prompts_copy_with_profiles_and_cascade_when_a_profile_is_deleted() {
    let store = Store::open_in_memory().expect("open store");
    let default_profile = store.active_profile_id().unwrap();
    store
        .put_quick_prompt(&prompt(200, "Three lines", "one\ntwo\nthree"))
        .unwrap();

    let (copy, _) = store.create_profile("Copied", true).unwrap();
    let copied = store.list_profile_quick_prompts(copy.id).unwrap();
    assert_eq!(copied.len(), 1);
    assert_ne!(copied[0].id, 200);
    assert_eq!(copied[0].body, "one\ntwo\nthree");

    store.switch_profile(copy.id).unwrap();
    store.delete_profile(default_profile).unwrap();
    let remaining: i64 = store
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM quick_prompts WHERE profile_id = ?1",
            [default_profile],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(remaining, 0);
    assert_eq!(store.list_quick_prompts().unwrap().len(), 1);
}
