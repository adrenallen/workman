-- Retention must never recycle a notification ID: clients remember delivered IDs.
CREATE TABLE notifications_retained (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    type       TEXT NOT NULL CHECK (
        type IN ('agent_done', 'needs_input', 'project_ready', 'process_idle', 'process_crashed',
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

INSERT INTO notifications_retained
SELECT id, type, project_id, process_id, todo_id, comment_id, body, created_at, read_at
FROM notifications;

DROP TABLE notifications;
ALTER TABLE notifications_retained RENAME TO notifications;

CREATE INDEX notifications_unread_idx
    ON notifications(read_at, created_at DESC) WHERE read_at IS NULL;
CREATE INDEX notifications_process_idx ON notifications(process_id, read_at);
CREATE INDEX notifications_todo_idx ON notifications(todo_id, read_at);
CREATE INDEX notifications_comment_idx ON notifications(comment_id) WHERE comment_id IS NOT NULL;
CREATE INDEX notifications_project_ready_idx
    ON notifications(project_id, read_at) WHERE type = 'project_ready';

-- Timer event references also need stable IDs after old completed timers are pruned.
CREATE TABLE timer_id_sequence (singleton INTEGER PRIMARY KEY CHECK(singleton = 1), next_id INTEGER NOT NULL);
INSERT INTO timer_id_sequence SELECT 1, COALESCE(MAX(id), 0) + 1 FROM timers;
CREATE TRIGGER timers_advance_id AFTER INSERT ON timers BEGIN
    UPDATE timer_id_sequence SET next_id = MAX(next_id, NEW.id + 1) WHERE singleton = 1;
END;

CREATE INDEX notifications_read_retention ON notifications(read_at) WHERE read_at IS NOT NULL;
CREATE INDEX notifications_created_retention ON notifications(created_at);
CREATE INDEX actors_retention ON actors(last_seen_at) WHERE process_id IS NULL;
CREATE INDEX timers_retention ON timers(fired_at) WHERE fired = 1 AND loop = 0;
-- A permanently removed project should not leave orphan notifications behind.
CREATE TRIGGER projects_clean_notifications BEFORE DELETE ON projects BEGIN
    DELETE FROM notifications WHERE project_id = OLD.id;
END;

-- Media paths contain feedback IDs; do not attach a new recording to old orphaned files.
CREATE TABLE feedback_id_sequence (singleton INTEGER PRIMARY KEY CHECK(singleton = 1), next_id INTEGER NOT NULL);
INSERT INTO feedback_id_sequence SELECT 1, COALESCE(MAX(id), 0) + 1 FROM recorded_feedback;
CREATE TRIGGER feedback_advance_id AFTER INSERT ON recorded_feedback BEGIN
    UPDATE feedback_id_sequence SET next_id = MAX(next_id, NEW.id + 1) WHERE singleton = 1;
END;
