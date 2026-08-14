ALTER TABLE timers
ADD COLUMN owner_process_id INTEGER REFERENCES processes(id) ON DELETE SET NULL;

ALTER TABLE todos
ADD COLUMN lock_process_id INTEGER REFERENCES processes(id) ON DELETE SET NULL;

ALTER TABLE locks
ADD COLUMN owner_process_id INTEGER REFERENCES processes(id) ON DELETE SET NULL;

UPDATE timers
SET owner_process_id = COALESCE(
    (SELECT actor.process_id FROM actors AS actor WHERE actor.id = timers.owner_actor),
    delivery_process_id
);

UPDATE todos
SET lock_process_id = (
    SELECT actor.process_id FROM actors AS actor WHERE actor.id = todos.lock_actor
)
WHERE lock_actor IS NOT NULL;

UPDATE locks
SET owner_process_id = (
    SELECT actor.process_id FROM actors AS actor WHERE actor.id = locks.owner_actor
);

CREATE INDEX timers_owner_process_id_idx ON timers(owner_process_id);
CREATE INDEX todos_lock_process_id_idx ON todos(lock_process_id);
CREATE INDEX locks_owner_process_id_idx ON locks(owner_process_id);
