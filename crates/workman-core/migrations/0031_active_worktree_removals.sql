CREATE TABLE active_worktree_removals (
    project_id       INTEGER PRIMARY KEY,
    phase            TEXT NOT NULL,
    delete_from_disk INTEGER NOT NULL CHECK (delete_from_disk IN (0, 1)),
    updated_at       INTEGER NOT NULL DEFAULT (unixepoch())
);
