-- A conversation belongs to one durable Workman process. Clear IDs that were
-- ambiguously attributed by the old same-cwd timestamp scan before enforcing
-- that invariant for future captures.
UPDATE process_agent_sessions
SET session_id = NULL, captured_at = NULL
WHERE session_id IN (
    SELECT session_id
    FROM process_agent_sessions
    WHERE session_id IS NOT NULL
    GROUP BY session_id
    HAVING COUNT(*) > 1
);

CREATE UNIQUE INDEX process_agent_sessions_unique_session_id
ON process_agent_sessions(session_id)
WHERE session_id IS NOT NULL;
