CREATE TABLE notifications (
    id         INTEGER PRIMARY KEY,
    type       TEXT NOT NULL CHECK (
        type IN ('agent_done', 'process_crashed', 'timer_fired')
    ),
    project_id INTEGER REFERENCES projects(id) ON DELETE SET NULL,
    process_id INTEGER REFERENCES processes(id) ON DELETE SET NULL,
    todo_id    INTEGER REFERENCES todos(id) ON DELETE SET NULL,
    body       TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    read_at    INTEGER
);

CREATE INDEX notifications_unread_idx
    ON notifications(read_at, created_at DESC)
    WHERE read_at IS NULL;

CREATE INDEX notifications_process_idx
    ON notifications(process_id, read_at);

-- Preserve unread completions created by the earlier per-agent marker store.
INSERT INTO notifications (type, project_id, process_id, body, created_at)
SELECT 'agent_done', process.project_id, process.id,
       process.name || ' finished and has unread output.',
       COALESCE(marker.unread_at, unixepoch('subsec') * 1000)
FROM agent_notifications AS marker
JOIN processes AS process ON process.id = marker.process_id
WHERE marker.unread = 1;
