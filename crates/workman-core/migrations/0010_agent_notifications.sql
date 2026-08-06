CREATE TABLE agent_notifications (
    process_id     INTEGER PRIMARY KEY REFERENCES processes(id) ON DELETE CASCADE,
    observed_state TEXT NOT NULL CHECK (
        observed_state IN ('working', 'needs_input', 'idle', 'exited')
    ),
    unread         INTEGER NOT NULL DEFAULT 0 CHECK (unread IN (0, 1)),
    unread_at      INTEGER
);

CREATE INDEX agent_notifications_unread_idx
    ON agent_notifications(unread, unread_at)
    WHERE unread = 1;
