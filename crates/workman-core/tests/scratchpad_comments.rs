use std::error::Error;

use workman_core::{
    NewScratchpadComment, Project, ScratchpadAnchorState, ScratchpadReadMode, ScratchpadService,
    Store, resolve_scratchpad_anchor,
};

fn seeded_store() -> Result<Store, Box<dyn Error>> {
    let store = Store::open_in_memory()?;
    store.put_project(&Project {
        id: 1,
        path: "/tmp/workman-scratchpad-comment-test".into(),
        name: "scratchpad-comments".into(),
        display_name: None,
        icon: None,
        selected: true,
        sort_order: 0,
    })?;
    Ok(store)
}

#[test]
fn scratchpad_comment_lifecycle_counts_and_cascades() -> Result<(), Box<dyn Error>> {
    let store = seeded_store()?;
    let service = ScratchpadService::attributed(&store, "user");
    let content = "Intro 😀\nShip the quiet comment panel.\nDone";
    let (scratchpad, _) = service.write(1, None, "Plan".into(), content.into(), None, None)?;
    let quote = "Ship the quiet comment panel.";
    let start = content.find(quote).unwrap();
    let start_utf16 = content[..start].encode_utf16().count();
    let end_utf16 = start_utf16 + quote.encode_utf16().count();
    let created = service.comment_create(
        1,
        scratchpad.id,
        NewScratchpadComment {
            body: "Tighten this step.".into(),
            quote: Some(quote.into()),
            anchor_start: Some(start_utf16),
            anchor_end: Some(end_utf16),
            ..NewScratchpadComment::default()
        },
        10,
    )?;
    assert_eq!(created.anchor.anchor_state, ScratchpadAnchorState::Anchored);
    assert_eq!(created.anchor.current_start_line, Some(2));
    assert_eq!(service.comment_count(1, scratchpad.id, false)?, 1);

    let updated = service.comment_update(1, created.comment.id, "Looks good now.".into(), 11)?;
    assert_eq!(updated.comment.body, "Looks good now.");
    let resolved = service.comment_set_resolved(1, created.comment.id, true, 12)?;
    assert!(resolved.comment.resolved);
    assert_eq!(service.comment_count(1, scratchpad.id, false)?, 0);
    assert_eq!(
        service
            .comment_list(1, scratchpad.id, false)?
            .comments
            .len(),
        0
    );
    assert_eq!(
        service.comment_list(1, scratchpad.id, true)?.comments.len(),
        1
    );

    service.comment_set_resolved(1, created.comment.id, false, 13)?;
    assert_eq!(
        service.comment_delete(1, created.comment.id)?,
        scratchpad.id
    );
    assert_eq!(service.comment_count(1, scratchpad.id, true)?, 0);

    let comment = service.comment_create(
        1,
        scratchpad.id,
        NewScratchpadComment {
            body: "Whole document note".into(),
            ..NewScratchpadComment::default()
        },
        14,
    )?;
    assert_eq!(
        comment.anchor.anchor_state,
        ScratchpadAnchorState::Unanchored
    );
    service.delete(1, scratchpad.id, scratchpad.revision)?;
    let persisted: usize = store.connection().query_row(
        "SELECT COUNT(*) FROM scratchpad_comments WHERE id = ?1",
        [comment.comment.id],
        |row| row.get(0),
    )?;
    assert_eq!(persisted, 0);
    Ok(())
}

#[test]
fn reanchor_uses_utf16_offsets_context_and_orphans_missing_quotes() -> Result<(), Box<dyn Error>> {
    let duplicate = "Alpha target Omega\nBeta target Gamma";
    let second_byte = duplicate.rfind("target").unwrap();
    let second_start = duplicate[..second_byte].encode_utf16().count();
    let second_end = second_start + "target".encode_utf16().count();
    let original = resolve_scratchpad_anchor(
        duplicate,
        Some("target"),
        Some(second_start),
        Some(second_end),
        Some("Beta "),
        Some(" Gamma"),
    );
    assert_eq!(original.anchor_state, ScratchpadAnchorState::Anchored);
    assert_eq!(original.current_start_line, Some(2));

    let shifted = "😀 heading\nAlpha target Omega\nBeta target Gamma";
    let reanchored = resolve_scratchpad_anchor(
        shifted,
        Some("target"),
        Some(second_start),
        Some(second_end),
        Some("Beta "),
        Some(" Gamma"),
    );
    assert_eq!(reanchored.anchor_state, ScratchpadAnchorState::Anchored);
    assert_eq!(reanchored.current_start_line, Some(3));
    assert!(reanchored.current_start.unwrap() > second_start);

    let ambiguous =
        resolve_scratchpad_anchor("target then target", Some("target"), None, None, None, None);
    assert_eq!(ambiguous.anchor_state, ScratchpadAnchorState::Orphaned);
    let orphaned = resolve_scratchpad_anchor(
        "the sentence was removed",
        Some("target"),
        Some(0),
        Some(6),
        Some(""),
        Some(""),
    );
    assert_eq!(orphaned.anchor_state, ScratchpadAnchorState::Orphaned);

    let store = seeded_store()?;
    let service = ScratchpadService::new(&store);
    let (scratchpad, _) = service.write(1, None, "Plan".into(), duplicate.into(), None, None)?;
    let listed = service.list(1, Default::default())?;
    assert_eq!(listed.scratchpads[0].unresolved_comment_count, 0);
    assert_eq!(
        service
            .read(1, scratchpad.id, ScratchpadReadMode::Full, None, 0, None)?
            .scratchpad
            .content,
        duplicate
    );
    Ok(())
}
