-- Legacy reports have no known delivery time and cannot satisfy a current-turn watch.
ALTER TABLE process_completion_reports ADD COLUMN reported_at INTEGER;
