CREATE TABLE notifications_with_needs_input (
    id         INTEGER PRIMARY KEY,
    type       TEXT NOT NULL CHECK (
        type IN ('agent_done', 'needs_input', 'process_crashed', 'timer_fired')
    ),
    project_id INTEGER REFERENCES projects(id) ON DELETE SET NULL,
    process_id INTEGER REFERENCES processes(id) ON DELETE SET NULL,
    todo_id    INTEGER REFERENCES todos(id) ON DELETE SET NULL,
    body       TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    read_at    INTEGER
);

INSERT INTO notifications_with_needs_input
    (id, type, project_id, process_id, todo_id, body, created_at, read_at)
SELECT id, type, project_id, process_id, todo_id, body, created_at, read_at
FROM notifications;

DROP TABLE notifications;
ALTER TABLE notifications_with_needs_input RENAME TO notifications;

CREATE INDEX notifications_unread_idx
    ON notifications(read_at, created_at DESC)
    WHERE read_at IS NULL;

CREATE INDEX notifications_process_idx
    ON notifications(process_id, read_at);
