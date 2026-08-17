use std::error::Error;

use workman_core::{
    NewScratchpadComment, Project, ScratchpadAnchorState, ScratchpadReadMode, ScratchpadService,
    ScratchpadServiceError, Store, resolve_scratchpad_anchor,
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
    assert!(created.can_edit);
    assert!(created.can_resolve);
    assert!(created.can_delete);
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

    let non_overlapping = resolve_scratchpad_anchor("aaa", Some("aa"), None, None, None, None);
    assert_eq!(
        non_overlapping.anchor_state,
        ScratchpadAnchorState::Anchored
    );
    assert_eq!(non_overlapping.current_start, Some(0));
    let context_mismatch = resolve_scratchpad_anchor(
        "only target remains",
        Some("target"),
        None,
        None,
        Some("old "),
        Some(" context"),
    );
    assert_eq!(
        context_mismatch.anchor_state,
        ScratchpadAnchorState::Orphaned
    );
    let crlf = resolve_scratchpad_anchor(
        "first\r\nsecond",
        Some("first\r\nsecond"),
        None,
        None,
        None,
        None,
    );
    assert_eq!(crlf.anchor_state, ScratchpadAnchorState::Anchored);
    assert_eq!(
        crlf.current_end,
        Some("first\nsecond".encode_utf16().count())
    );

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

#[test]
fn comment_permissions_revision_guards_and_mutation_signal_are_enforced()
-> Result<(), Box<dyn Error>> {
    let store = seeded_store()?;
    let user = ScratchpadService::attributed(&store, "user");
    let agent = ScratchpadService::attributed(&store, "mcp-agent-one");
    let other_agent = ScratchpadService::attributed(&store, "mcp-agent-two");
    let (scratchpad, _) =
        user.write(1, None, "Plan".into(), "first\r\nsecond".into(), None, None)?;
    assert_eq!(scratchpad.content, "first\nsecond");

    let user_comment = user.comment_create(
        1,
        scratchpad.id,
        NewScratchpadComment {
            body: "Human review".into(),
            quote: Some("missing quote".into()),
            anchor_prefix: Some("first ".into()),
            anchor_suffix: Some(" second".into()),
            allow_unanchored: true,
            expected_revision: Some(scratchpad.revision),
            ..NewScratchpadComment::default()
        },
        10,
    )?;
    assert_eq!(
        user_comment.comment.anchor_prefix.as_deref(),
        Some("first ")
    );
    assert_eq!(
        user_comment.comment.anchor_suffix.as_deref(),
        Some(" second")
    );
    assert_eq!(
        user.list(1, Default::default())?.scratchpads[0].comments_revision,
        1
    );

    for error in [
        agent
            .comment_update(1, user_comment.comment.id, "rewrite".into(), 11)
            .unwrap_err(),
        agent
            .comment_set_resolved(1, user_comment.comment.id, true, 12)
            .unwrap_err(),
        agent
            .comment_delete(1, user_comment.comment.id)
            .unwrap_err(),
    ] {
        assert!(matches!(
            error,
            ScratchpadServiceError::CommentPermissionDenied { .. }
        ));
    }

    let agent_comment = agent.comment_create(
        1,
        scratchpad.id,
        NewScratchpadComment {
            body: "Agent note".into(),
            ..NewScratchpadComment::default()
        },
        13,
    )?;
    assert!(
        !user
            .comment_list(1, scratchpad.id, true)?
            .comments
            .iter()
            .find(|comment| comment.comment.id == agent_comment.comment.id)
            .unwrap()
            .can_edit
    );
    assert!(
        user.comment_list(1, scratchpad.id, true)?
            .comments
            .iter()
            .find(|comment| comment.comment.id == agent_comment.comment.id)
            .unwrap()
            .can_resolve
    );
    user.comment_set_resolved(1, agent_comment.comment.id, true, 14)?;
    assert!(matches!(
        user.comment_delete(1, agent_comment.comment.id),
        Err(ScratchpadServiceError::CommentPermissionDenied { .. })
    ));
    assert!(matches!(
        other_agent.comment_update(1, agent_comment.comment.id, "nope".into(), 15),
        Err(ScratchpadServiceError::CommentPermissionDenied { .. })
    ));
    agent.comment_delete(1, agent_comment.comment.id)?;
    assert_eq!(
        user.list(1, Default::default())?.scratchpads[0].comments_revision,
        4,
        "insert + insert + resolve + delete must all advance the signal"
    );

    assert!(matches!(
        user.comment_create(
            1,
            scratchpad.id,
            NewScratchpadComment {
                body: "stale".into(),
                expected_revision: Some(scratchpad.revision + 1),
                ..NewScratchpadComment::default()
            },
            16,
        ),
        Err(ScratchpadServiceError::RevisionConflict { .. })
    ));
    assert!(matches!(
        user.comment_create(
            1,
            scratchpad.id,
            NewScratchpadComment {
                body: "large quote".into(),
                quote: Some("x".repeat(4_097)),
                allow_unanchored: true,
                ..NewScratchpadComment::default()
            },
            17,
        ),
        Err(ScratchpadServiceError::InvalidInput(_))
    ));
    Ok(())
}
