CREATE TABLE notifications_with_project_ready (
    id         INTEGER PRIMARY KEY,
    type       TEXT NOT NULL CHECK (
        type IN ('agent_done', 'needs_input', 'project_ready', 'process_crashed',
                 'timer_fired', 'todo_assigned_to_you', 'mentioned_in_comment')
    ),
    project_id INTEGER REFERENCES projects(id) ON DELETE SET NULL,
    process_id INTEGER REFERENCES processes(id) ON DELETE SET NULL,
    todo_id    INTEGER REFERENCES todos(id) ON DELETE SET NULL,
    comment_id INTEGER REFERENCES todo_comments(id) ON DELETE SET NULL,
    body       TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    read_at    INTEGER
);

INSERT INTO notifications_with_project_ready
SELECT id, type, project_id, process_id, todo_id, comment_id, body, created_at, read_at
FROM notifications;

DROP TABLE notifications;
ALTER TABLE notifications_with_project_ready RENAME TO notifications;

CREATE INDEX notifications_unread_idx
    ON notifications(read_at, created_at DESC) WHERE read_at IS NULL;
CREATE INDEX notifications_process_idx ON notifications(process_id, read_at);
CREATE INDEX notifications_todo_idx ON notifications(todo_id, read_at);
CREATE INDEX notifications_comment_idx ON notifications(comment_id) WHERE comment_id IS NOT NULL;
CREATE INDEX notifications_project_ready_idx
    ON notifications(project_id, read_at) WHERE type = 'project_ready';
