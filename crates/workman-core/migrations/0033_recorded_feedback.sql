CREATE TABLE recorded_feedback (
    id               INTEGER PRIMARY KEY,
    project_id       INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    title            TEXT NOT NULL,
    status           TEXT NOT NULL CHECK (status IN ('recording', 'transcribing', 'ready', 'failed')),
    revision         INTEGER NOT NULL DEFAULT 1,
    duration_ms      INTEGER NOT NULL DEFAULT 0,
    audio_path       TEXT,
    transcript_json  TEXT NOT NULL DEFAULT '[]',
    blocks_json      TEXT NOT NULL DEFAULT '[]',
    error_code       TEXT,
    archived         INTEGER NOT NULL DEFAULT 0,
    lease_owner      TEXT,
    lease_expires_at INTEGER,
    created_at       INTEGER NOT NULL,
    updated_at       INTEGER NOT NULL
);

CREATE INDEX recorded_feedback_project_status_idx
    ON recorded_feedback(project_id, archived, updated_at DESC, id DESC);

CREATE TABLE recorded_feedback_snapshots (
    id             INTEGER PRIMARY KEY,
    feedback_id    INTEGER NOT NULL REFERENCES recorded_feedback(id) ON DELETE CASCADE,
    ordinal        INTEGER NOT NULL,
    anchor_ms      INTEGER NOT NULL,
    anchor_samples INTEGER NOT NULL,
    invoked_at_ms  INTEGER NOT NULL,
    completed_at_ms INTEGER NOT NULL,
    image_path     TEXT NOT NULL,
    caption        TEXT NOT NULL DEFAULT '',
    width          INTEGER NOT NULL,
    height         INTEGER NOT NULL,
    sha256         TEXT NOT NULL,
    UNIQUE(feedback_id, ordinal)
);

CREATE TABLE recorded_feedback_deliveries (
    id            INTEGER PRIMARY KEY,
    feedback_id   INTEGER NOT NULL REFERENCES recorded_feedback(id) ON DELETE CASCADE,
    target_kind   TEXT NOT NULL CHECK (target_kind IN ('agent', 'scratchpad', 'clipboard')),
    target_id     INTEGER,
    status        TEXT NOT NULL CHECK (status IN ('queued', 'unverified', 'failed')),
    packet_path   TEXT,
    error_message TEXT,
    created_at    INTEGER NOT NULL,
    updated_at    INTEGER NOT NULL
);

CREATE INDEX recorded_feedback_deliveries_feedback_idx
    ON recorded_feedback_deliveries(feedback_id, created_at DESC, id DESC);
