ALTER TABLE todos ADD COLUMN lock_acquired_at INTEGER;

CREATE INDEX todos_lock_actor_idx ON todos(lock_actor) WHERE lock_actor IS NOT NULL;
