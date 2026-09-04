ALTER TABLE recorded_feedback ADD COLUMN append_state_json TEXT;

CREATE TABLE recorded_feedback_deliveries_new (
    id INTEGER PRIMARY KEY,
    feedback_id INTEGER NOT NULL REFERENCES recorded_feedback(id) ON DELETE CASCADE,
    target_kind TEXT NOT NULL CHECK (target_kind IN ('agent', 'scratchpad', 'clipboard')),
    target_id INTEGER,
    target_name TEXT,
    feedback_revision INTEGER,
    status TEXT NOT NULL CHECK (status IN ('pending', 'sent', 'queued', 'unverified', 'failed')),
    packet_path TEXT,
    error_message TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

INSERT INTO recorded_feedback_deliveries_new
    (id, feedback_id, target_kind, target_id, target_name, status, packet_path,
     error_message, created_at, updated_at)
SELECT delivery.id, feedback_id, target_kind, target_id,
       CASE target_kind
           WHEN 'agent' THEN (SELECT name FROM processes WHERE id = target_id)
           WHEN 'scratchpad' THEN (SELECT name FROM scratchpads WHERE id = target_id)
       END,
       CASE WHEN target_kind = 'scratchpad' AND status = 'queued' THEN 'sent' ELSE status END,
       packet_path, error_message, created_at, updated_at
FROM recorded_feedback_deliveries AS delivery;

DROP TABLE recorded_feedback_deliveries;
ALTER TABLE recorded_feedback_deliveries_new RENAME TO recorded_feedback_deliveries;
CREATE INDEX recorded_feedback_deliveries_feedback_idx
    ON recorded_feedback_deliveries(feedback_id, created_at DESC, id DESC);
