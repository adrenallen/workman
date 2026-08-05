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
