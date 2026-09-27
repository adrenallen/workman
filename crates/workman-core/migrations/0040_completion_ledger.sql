CREATE TABLE process_completion_observations (
    process_id                     INTEGER PRIMARY KEY REFERENCES processes(id) ON DELETE CASCADE,
    last_completed_input_at        INTEGER,
    last_completed_work_evidence_at INTEGER
);

CREATE TABLE process_completions (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    process_id       INTEGER NOT NULL REFERENCES processes(id) ON DELETE CASCADE,
    input_at         INTEGER NOT NULL,
    work_evidence_at INTEGER NOT NULL,
    completed_at     INTEGER NOT NULL,
    UNIQUE(process_id, input_at, work_evidence_at)
);

CREATE INDEX process_completions_process_idx
    ON process_completions(process_id, completed_at, id);

CREATE TABLE process_completion_inputs (
    owner_process_id INTEGER NOT NULL REFERENCES processes(id) ON DELETE CASCADE,
    process_id       INTEGER NOT NULL REFERENCES processes(id) ON DELETE CASCADE,
    last_input_at    INTEGER NOT NULL,
    PRIMARY KEY (owner_process_id, process_id)
);

CREATE INDEX process_completion_inputs_process_idx
    ON process_completion_inputs(process_id);

CREATE TABLE process_completion_reports (
    owner_process_id INTEGER NOT NULL REFERENCES processes(id) ON DELETE CASCADE,
    process_id       INTEGER NOT NULL REFERENCES processes(id) ON DELETE CASCADE,
    completion_id    INTEGER NOT NULL REFERENCES process_completions(id) ON DELETE CASCADE,
    PRIMARY KEY (owner_process_id, process_id)
);

CREATE INDEX process_completion_reports_process_idx
    ON process_completion_reports(process_id);

CREATE INDEX process_completion_reports_completion_idx
    ON process_completion_reports(completion_id);

ALTER TABLE timer_runtime ADD COLUMN diagnostics TEXT NOT NULL DEFAULT '{}'
    CHECK (json_valid(diagnostics));
