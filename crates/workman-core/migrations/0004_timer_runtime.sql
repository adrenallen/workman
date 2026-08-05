CREATE TABLE timer_runtime (
    timer_id    INTEGER PRIMARY KEY REFERENCES timers(id) ON DELETE CASCADE,
    due_at      INTEGER NOT NULL,
    paused_at   INTEGER,
    watch_state TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(watch_state))
);

CREATE INDEX timer_runtime_due_idx ON timer_runtime(due_at);
