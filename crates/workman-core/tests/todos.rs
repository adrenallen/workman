use workman_core::{
    NewTodo, NotificationType, Project, Store, TodoActivityKind, TodoListQuery, TodoPriority,
    TodoService, TodoServiceError, TodoSort, TodoStatus, UpdateTodo,
};

const NOW: i64 = 1_800_000_000_000;

fn project(id: i64, name: &str) -> Project {
    Project {
        id,
        path: format!("/workspace/{name}"),
        name: name.into(),
        display_name: None,
        icon: None,
        selected: false,
        sort_order: id - 1,
    }
}

#[test]
fn todo_activity_records_lifecycle_edges_without_renewal_noise() {
    let store = Store::open_in_memory().unwrap();
    store.put_project(&project(1, "one")).unwrap();
    let service = TodoService::new(&store);
    let todo_id = create_todo(&service, 1, "Trace lifecycle");

    service
        .lock(1, todo_id, "desktop-ui", 60_000, NOW + 1)
        .unwrap();
    service
        .lock(1, todo_id, "desktop-ui", 60_000, NOW + 2)
        .unwrap();
    service.unlock(1, todo_id, "desktop-ui", NOW + 3).unwrap();
    service
        .complete(1, todo_id, "desktop-ui", true, false, NOW + 4)
        .unwrap();
    service
        .complete(1, todo_id, "desktop-ui", false, false, NOW + 5)
        .unwrap();

    let activity = service.activity_list(1, todo_id, NOW + 5).unwrap();
    assert_eq!(
        activity.iter().map(|item| item.kind).collect::<Vec<_>>(),
        [
            TodoActivityKind::Created,
            TodoActivityKind::Locked,
            TodoActivityKind::Unlocked,
            TodoActivityKind::Completed,
            TodoActivityKind::Reopened,
        ]
    );
    assert_eq!(activity[1].actor, "desktop-ui");
}

fn create_todo(service: &TodoService<'_>, project_id: i64, title: &str) -> i64 {
    service
        .create(
            project_id,
            NewTodo {
                title: title.into(),
                body: format!("Body for {title}"),
                priority: TodoPriority::Medium,
                tags: vec!["core".into()],
            },
            NOW,
        )
        .unwrap()
        .id
}

#[test]
fn todo_service_manages_graph_tags_comments_and_transfer() {
    let store = Store::open_in_memory().unwrap();
    store.put_project(&project(1, "one")).unwrap();
    store.put_project(&project(2, "two")).unwrap();
    let service = TodoService::new(&store);

    let prerequisite_id = create_todo(&service, 1, "Build store");
    let todo_id = create_todo(&service, 1, "Expose tools");
    let dependent_id = create_todo(&service, 1, "Build UI");

    let todo = service
        .update(
            1,
            todo_id,
            UpdateTodo {
                title: Some("Expose MCP tools".into()),
                body: Some("Add todo endpoints".into()),
                priority: Some(TodoPriority::High),
                status: Some(TodoStatus::InProgress),
                tags: Some(vec!["mcp".into(), "core".into(), "mcp".into()]),
            },
            NOW,
        )
        .unwrap();
    assert_eq!(todo.tags, ["mcp", "core"]);
    assert_eq!(todo.status, TodoStatus::InProgress);

    service
        .add_tag(1, todo_id, " coordination ".into(), NOW)
        .unwrap();
    let todo = service.remove_tag(1, todo_id, "core".into(), NOW).unwrap();
    assert_eq!(todo.tags, ["mcp", "coordination"]);
    assert_eq!(service.tags_list(1).unwrap()[0], "core");

    let blocked = service
        .set_blockers(1, todo_id, vec![prerequisite_id], NOW)
        .unwrap();
    assert!(blocked.is_blocked);
    assert_eq!(blocked.blocker_ids, [prerequisite_id]);
    service.add_blocker(1, dependent_id, todo_id, NOW).unwrap();

    let comment = service
        .comment_create(1, todo_id, "mcp-agent", "Initial note".into(), NOW)
        .unwrap();
    let comment = service
        .comment_update(1, comment.id, "Updated note".into(), NOW + 1)
        .unwrap();
    assert_eq!(comment.body, "Updated note");
    let comments = service
        .comment_list(1, todo_id, 0, Some(10), NOW + 1)
        .unwrap();
    assert_eq!(comments.total_count, 1);

    let (completed, affected) = service
        .complete(1, prerequisite_id, "mcp-agent", true, true, NOW + 2)
        .unwrap();
    assert!(completed.completed);
    assert_eq!(affected, [todo_id]);
    assert!(
        !service
            .get(1, todo_id, NOW + 2)
            .unwrap()
            .unwrap()
            .is_blocked
    );

    service
        .lock(1, todo_id, "mcp-agent", 60_000, NOW + 2)
        .unwrap();
    let (transferred, affected) = service.transfer(1, todo_id, 2, NOW + 3).unwrap();
    assert_eq!(transferred.project_id, 2);
    assert_eq!(transferred.locked_by, None);
    assert!(transferred.blocker_ids.is_empty());
    assert_eq!(transferred.comment_count, 1);
    assert_eq!(affected, [dependent_id]);
    assert!(
        service
            .get(1, dependent_id, NOW + 3)
            .unwrap()
            .unwrap()
            .blocker_ids
            .is_empty()
    );

    assert_eq!(
        service.comment_delete(2, comment.id, NOW + 3).unwrap(),
        todo_id
    );
    assert_eq!(
        service
            .comment_list(2, todo_id, 0, None, NOW + 3)
            .unwrap()
            .total_count,
        0
    );
}

#[test]
fn todo_service_rejects_cycles_and_foreign_project_blockers() {
    let store = Store::open_in_memory().unwrap();
    store.put_project(&project(1, "one")).unwrap();
    store.put_project(&project(2, "two")).unwrap();
    let service = TodoService::new(&store);
    let first = create_todo(&service, 1, "First");
    let second = create_todo(&service, 1, "Second");
    let foreign = create_todo(&service, 2, "Foreign");

    service.add_blocker(1, second, first, NOW).unwrap();
    assert!(matches!(
        service.add_blocker(1, first, second, NOW),
        Err(TodoServiceError::BlockerCycle { .. })
    ));
    assert!(matches!(
        service.add_blocker(1, first, foreign, NOW),
        Err(TodoServiceError::TodoNotFound(id)) if id == foreign
    ));
}

#[test]
fn todo_service_filters_sorts_and_paginates() {
    let store = Store::open_in_memory().unwrap();
    store.put_project(&project(1, "one")).unwrap();
    let service = TodoService::new(&store);
    let low = create_todo(&service, 1, "Low item");
    let high = create_todo(&service, 1, "High item");
    service
        .update(
            1,
            low,
            UpdateTodo {
                priority: Some(TodoPriority::Low),
                tags: Some(vec!["later".into()]),
                ..UpdateTodo::default()
            },
            NOW,
        )
        .unwrap();
    service
        .update(
            1,
            high,
            UpdateTodo {
                priority: Some(TodoPriority::High),
                tags: Some(vec!["now".into()]),
                ..UpdateTodo::default()
            },
            NOW,
        )
        .unwrap();
    service
        .comment_create(1, low, "actor", "contains searchable phrase".into(), NOW)
        .unwrap();

    let page = service
        .list(
            1,
            TodoListQuery {
                sort: TodoSort::Priority,
                limit: Some(1),
                ..TodoListQuery::default()
            },
            NOW,
        )
        .unwrap();
    assert_eq!(page.todos[0].id, high);
    assert_eq!(page.total_count, 2);
    assert!(page.has_more);
    assert_eq!(page.next_offset, Some(1));

    let search = service
        .list(
            1,
            TodoListQuery {
                query: Some("SEARCHABLE".into()),
                tags: vec!["later".into()],
                ..TodoListQuery::default()
            },
            NOW,
        )
        .unwrap();
    assert_eq!(search.todos.len(), 1);
    assert_eq!(search.todos[0].id, low);
}

#[test]
fn user_assignment_edges_filter_and_reassignment_notifications_are_durable() {
    let store = Store::open_in_memory().unwrap();
    store.put_project(&project(1, "one")).unwrap();
    let service = TodoService::new(&store);
    let todo_id = create_todo(&service, 1, "Human review");

    let assigned = service
        .assign(1, todo_id, Some("@user".into()), "agent-one", NOW + 1)
        .unwrap();
    assert_eq!(assigned.assignee.as_deref(), Some("user"));
    service
        .assign(1, todo_id, Some("user".into()), "agent-one", NOW + 2)
        .unwrap();
    service
        .assign(1, todo_id, Some("none".into()), "agent-one", NOW + 3)
        .unwrap();
    service
        .assign(1, todo_id, Some("user".into()), "agent-two", NOW + 4)
        .unwrap();

    let notifications = store.list_notifications(None, 20).unwrap();
    assert_eq!(notifications.len(), 2);
    assert!(
        notifications
            .iter()
            .all(|notification| notification.kind == NotificationType::TodoAssignedToYou)
    );
    assert!(notifications[0].body.contains("agent-two"));
    assert!(notifications[0].body.contains("Human review"));

    let assigned_page = service
        .list(
            1,
            TodoListQuery {
                assignee: Some("user".into()),
                ..TodoListQuery::default()
            },
            NOW + 4,
        )
        .unwrap();
    assert_eq!(assigned_page.todos.len(), 1);
    assert_eq!(assigned_page.todos[0].id, todo_id);
}

#[test]
fn new_user_mentions_notify_once_and_comment_edits_do_not_create_edges() {
    let store = Store::open_in_memory().unwrap();
    store.put_project(&project(1, "one")).unwrap();
    let service = TodoService::new(&store);
    let todo_id = create_todo(&service, 1, "Choose an approach");

    let comment = service
        .comment_create_as(
            1,
            todo_id,
            "process-7",
            "review-agent",
            "Could @UsEr choose the safer option?".into(),
            NOW + 1,
        )
        .unwrap();
    service
        .comment_update(
            1,
            comment.id,
            "Could @user choose the safer option, please?".into(),
            NOW + 2,
        )
        .unwrap();
    service
        .comment_create_as(
            1,
            todo_id,
            "process-7",
            "review-agent",
            "Email agent@user.example instead.".into(),
            NOW + 3,
        )
        .unwrap();

    let notifications = store.list_notifications(None, 20).unwrap();
    assert_eq!(notifications.len(), 1);
    assert_eq!(notifications[0].kind, NotificationType::MentionedInComment);
    assert_eq!(notifications[0].todo_id, Some(todo_id));
    assert_eq!(notifications[0].comment_id, Some(comment.id));
    assert!(notifications[0].body.contains("review-agent"));
    assert!(notifications[0].body.contains("Choose an approach"));
}

#[test]
fn todo_locks_are_leased_owned_and_completion_releases_only_callers_lock() {
    let store = Store::open_in_memory().unwrap();
    store.put_project(&project(1, "one")).unwrap();
    let service = TodoService::new(&store);
    let todo_id = create_todo(&service, 1, "Claim me");

    service.lock(1, todo_id, "actor-a", 100, NOW).unwrap();
    assert!(matches!(
        service.lock(1, todo_id, "actor-b", 100, NOW + 1),
        Err(TodoServiceError::Locked { .. })
    ));
    assert!(matches!(
        service.unlock(1, todo_id, "actor-b", NOW + 1),
        Err(TodoServiceError::LockNotOwned(id)) if id == todo_id
    ));

    let (completed, _) = service
        .complete(1, todo_id, "actor-b", true, true, NOW + 2)
        .unwrap();
    assert_eq!(completed.locked_by.as_deref(), Some("actor-a"));
    let (completed, _) = service
        .complete(1, todo_id, "actor-a", true, true, NOW + 3)
        .unwrap();
    assert_eq!(completed.locked_by, None);

    service.lock(1, todo_id, "actor-b", 100, NOW + 101).unwrap();
    assert_eq!(
        service
            .get(1, todo_id, NOW + 102)
            .unwrap()
            .unwrap()
            .locked_by
            .as_deref(),
        Some("actor-b")
    );
}

#[test]
fn todo_sidebar_order_appends_and_preserves_completed_slots() {
    let store = Store::open_in_memory().unwrap();
    store.put_project(&project(1, "one")).unwrap();
    let service = TodoService::new(&store);
    let first = create_todo(&service, 1, "First");
    let second = create_todo(&service, 1, "Second");
    let third = create_todo(&service, 1, "Third");

    let reordered = service.reorder(1, &[third, first, second], NOW).unwrap();
    assert_eq!(
        reordered.iter().map(|todo| todo.id).collect::<Vec<_>>(),
        [third, first, second]
    );

    service
        .complete(1, first, "test", true, false, NOW + 1)
        .unwrap();
    service.reorder(1, &[second, third], NOW + 2).unwrap();
    service
        .complete(1, first, "test", false, false, NOW + 3)
        .unwrap();
    let fourth = create_todo(&service, 1, "Fourth");

    let mut statement = store
        .connection()
        .prepare("SELECT id, sort_order FROM todos WHERE project_id = 1 ORDER BY sort_order, id")
        .unwrap();
    let rows = statement
        .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    assert_eq!(rows, [(second, 0), (first, 1), (third, 2), (fourth, 3)]);
}
