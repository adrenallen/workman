CREATE TABLE project_command_approvals (
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    trust_hash TEXT NOT NULL,
    PRIMARY KEY (project_id, trust_hash)
);

-- Preserve approvals made before project-level history existed.
INSERT OR IGNORE INTO project_command_approvals (project_id, trust_hash)
SELECT project_id, trust_hash FROM processes
WHERE source = 'yml' AND trust_hash IS NOT NULL;
