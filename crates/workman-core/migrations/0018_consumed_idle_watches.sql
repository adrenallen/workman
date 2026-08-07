CREATE TABLE consumed_idle_watches (
    process_id INTEGER NOT NULL REFERENCES processes(id) ON DELETE CASCADE,
    timer_id   INTEGER NOT NULL REFERENCES timers(id) ON DELETE CASCADE,
    fired_at   INTEGER NOT NULL,
    PRIMARY KEY (process_id, timer_id)
);

CREATE INDEX consumed_idle_watches_process_idx
    ON consumed_idle_watches(process_id, fired_at);
