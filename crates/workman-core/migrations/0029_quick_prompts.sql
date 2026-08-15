CREATE TABLE quick_prompts (
    id         INTEGER PRIMARY KEY,
    profile_id INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    name       TEXT NOT NULL COLLATE NOCASE,
    body       TEXT NOT NULL,
    sort_order INTEGER NOT NULL DEFAULT 0 CHECK (sort_order >= 0),
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
    UNIQUE (profile_id, name)
);

CREATE INDEX quick_prompts_profile_order_idx
ON quick_prompts(profile_id, sort_order, id);
