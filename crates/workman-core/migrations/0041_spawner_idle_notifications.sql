CREATE TABLE process_spawner_idle_notifications (
    process_id          INTEGER PRIMARY KEY REFERENCES processes(id) ON DELETE CASCADE,
    enabled_at          INTEGER NOT NULL,
    last_reported_state TEXT NOT NULL DEFAULT 'neutral' CHECK (
        last_reported_state IN ('neutral', 'needs_input', 'exited', 'crashed')
    )
);

