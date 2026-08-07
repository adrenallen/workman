use std::error::Error;

use workman_core::{Project, ScratchpadReadMode, ScratchpadService, Store};

#[test]
fn leading_h1_is_name_metadata_across_section_appends() -> Result<(), Box<dyn Error>> {
    let store = Store::open_in_memory()?;
    store.put_project(&Project {
        id: 1,
        path: "/tmp/workman-scratchpad-h1-test".into(),
        name: "scratchpad-h1".into(),
        display_name: None,
        icon: None,
        selected: true,
        sort_order: 0,
    })?;
    let service = ScratchpadService::new(&store);

    let (created, was_created) = service.write(
        1,
        None,
        "fallback name".into(),
        "# Acceptance Notes\n".into(),
        None,
        None,
    )?;
    assert!(was_created);
    assert_eq!(created.name, "Acceptance Notes");
    assert_eq!(created.content, "", "the H1 is canonical name metadata");

    let first = service.append_section(
        1,
        created.id,
        "Acceptance Notes",
        "## Fact A: Rust\n\nCargo\n".into(),
        Some(created.revision),
    )?;
    let second = service.append_section(
        1,
        created.id,
        "acceptance notes",
        "## Fact B: UI\n\nTauri".into(),
        Some(first.revision),
    )?;
    let expected_body = "## Fact A: Rust\n\nCargo\n## Fact B: UI\n\nTauri";
    assert_eq!(second.name, "Acceptance Notes");
    assert_eq!(second.content, expected_body);

    let full = service.read(1, created.id, ScratchpadReadMode::Full, None, 0, None)?;
    assert_eq!(full.scratchpad.content, expected_body);
    let title_section = service.read(
        1,
        created.id,
        ScratchpadReadMode::Section,
        Some("Acceptance Notes"),
        0,
        None,
    )?;
    assert_eq!(
        title_section.scratchpad.content,
        format!("# Acceptance Notes\n\n{expected_body}")
    );
    Ok(())
}

#[test]
fn scratchpad_sidebar_order_appends_and_preserves_archived_slots() -> Result<(), Box<dyn Error>> {
    let store = Store::open_in_memory()?;
    store.put_project(&Project {
        id: 1,
        path: "/tmp/workman-scratchpad-order-test".into(),
        name: "scratchpad-order".into(),
        display_name: None,
        icon: None,
        selected: true,
        sort_order: 0,
    })?;
    let service = ScratchpadService::new(&store);
    let first = service
        .write(1, None, "First".into(), String::new(), None, None)?
        .0;
    let second = service
        .write(1, None, "Second".into(), String::new(), None, None)?
        .0;
    let third = service
        .write(1, None, "Third".into(), String::new(), None, None)?
        .0;

    let reordered = service.reorder(1, &[third.id, first.id, second.id])?;
    assert_eq!(
        reordered
            .iter()
            .map(|scratchpad| scratchpad.id)
            .collect::<Vec<_>>(),
        [third.id, first.id, second.id]
    );

    service.archive(1, first.id, Some(first.revision))?;
    service.reorder(1, &[second.id, third.id])?;
    let fourth = service
        .write(1, None, "Fourth".into(), String::new(), None, None)?
        .0;

    let mut statement = store.connection().prepare(
        "SELECT id, sort_order FROM scratchpads WHERE project_id = 1 ORDER BY sort_order, id",
    )?;
    let rows = statement
        .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    assert_eq!(
        rows,
        [(second.id, 0), (first.id, 1), (third.id, 2), (fourth.id, 3)]
    );
    Ok(())
}
