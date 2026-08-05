CREATE TABLE worktree_repositories (
    id           INTEGER PRIMARY KEY,
    root_path    TEXT NOT NULL UNIQUE,
    name         TEXT NOT NULL,
    managed_root TEXT NOT NULL
);

CREATE TABLE project_worktrees (
    project_id        INTEGER PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
    repository_id     INTEGER NOT NULL REFERENCES worktree_repositories(id) ON DELETE CASCADE,
    parent_project_id INTEGER REFERENCES projects(id) ON DELETE SET NULL,
    branch            TEXT NOT NULL,
    managed           INTEGER NOT NULL DEFAULT 0 CHECK (managed IN (0, 1))
);

CREATE INDEX project_worktrees_repository_idx
ON project_worktrees(repository_id, parent_project_id, project_id);

CREATE TABLE worktree_preferences (
    repository_id INTEGER NOT NULL REFERENCES worktree_repositories(id) ON DELETE CASCADE,
    key           TEXT NOT NULL,
    value         TEXT NOT NULL,
    PRIMARY KEY (repository_id, key)
);
