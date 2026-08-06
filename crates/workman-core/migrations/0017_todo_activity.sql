CREATE TABLE todo_activity (
    id         INTEGER PRIMARY KEY,
    todo_id    INTEGER NOT NULL REFERENCES todos(id) ON DELETE CASCADE,
    actor      TEXT NOT NULL,
    kind       TEXT NOT NULL CHECK (kind IN ('created', 'completed', 'reopened', 'locked', 'unlocked')),
    created_at INTEGER NOT NULL
);

CREATE INDEX todo_activity_todo_created_idx ON todo_activity(todo_id, created_at, id);
