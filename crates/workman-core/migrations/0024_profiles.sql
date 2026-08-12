CREATE TABLE profiles (
    id                     INTEGER PRIMARY KEY,
    name                   TEXT NOT NULL COLLATE NOCASE UNIQUE,
    active                 INTEGER NOT NULL DEFAULT 0 CHECK (active IN (0, 1)),
    terminal_shell         TEXT,
    legacy_config_imported INTEGER NOT NULL DEFAULT 0 CHECK (legacy_config_imported IN (0, 1)),
    created_at             INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE UNIQUE INDEX profiles_one_active_idx ON profiles(active) WHERE active = 1;

INSERT INTO profiles (id, name, active)
VALUES (1, 'Default', 1);

CREATE TABLE profile_projects (
    profile_id  INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    project_id  INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    sort_order  INTEGER NOT NULL DEFAULT 0 CHECK (sort_order >= 0),
    selected    INTEGER NOT NULL DEFAULT 0 CHECK (selected IN (0, 1)),
    PRIMARY KEY (profile_id, project_id)
);

CREATE UNIQUE INDEX profile_projects_one_selected_idx
ON profile_projects(profile_id) WHERE selected = 1;

CREATE INDEX profile_projects_order_idx
ON profile_projects(profile_id, sort_order, project_id);

INSERT INTO profile_projects (profile_id, project_id, sort_order, selected)
SELECT 1,
       id,
       sort_order,
       CASE
           WHEN selected = 1
            AND id = (SELECT id FROM projects WHERE selected = 1 ORDER BY sort_order, id LIMIT 1)
           THEN 1
           ELSE 0
       END
FROM projects;

ALTER TABLE agent_tools
ADD COLUMN profile_id INTEGER REFERENCES profiles(id) ON DELETE CASCADE;

ALTER TABLE agent_tools
ADD COLUMN display_name TEXT;

UPDATE agent_tools
SET profile_id = 1,
    display_name = name;

CREATE INDEX agent_tools_profile_order_idx
ON agent_tools(profile_id, sort_order, id);

CREATE UNIQUE INDEX agent_tools_profile_display_name_idx
ON agent_tools(profile_id, display_name COLLATE NOCASE);
