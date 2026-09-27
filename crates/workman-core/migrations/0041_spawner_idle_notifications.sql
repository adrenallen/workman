CREATE TABLE process_spawner_idle_notifications (
    process_id          INTEGER PRIMARY KEY REFERENCES processes(id) ON DELETE CASCADE,
    enabled_at          INTEGER NOT NULL,
    baseline_completion_id INTEGER NOT NULL DEFAULT 0,
    suppressed_completion_id INTEGER NOT NULL DEFAULT 0,
    last_finished_input_at INTEGER,
    last_reported_state TEXT NOT NULL DEFAULT 'neutral' CHECK (
        last_reported_state IN ('neutral', 'needs_input', 'exited', 'crashed')
    )
);
