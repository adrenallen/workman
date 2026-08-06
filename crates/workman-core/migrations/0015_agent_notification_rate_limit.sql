ALTER TABLE agent_notifications ADD COLUMN last_notified_at INTEGER;
ALTER TABLE agent_notifications ADD COLUMN last_viewed_at INTEGER;

-- Existing unread markers represent a completion that has already notified.
UPDATE agent_notifications
SET last_notified_at = unread_at
WHERE unread_at IS NOT NULL;
