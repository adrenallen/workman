ALTER TABLE processes
ADD COLUMN spawned_by_process_id INTEGER REFERENCES processes(id) ON DELETE SET NULL;

CREATE INDEX processes_spawned_by_process_id_idx
ON processes(spawned_by_process_id);
