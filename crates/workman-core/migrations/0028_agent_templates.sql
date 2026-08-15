CREATE TABLE agent_templates (
    id            INTEGER PRIMARY KEY,
    profile_id    INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    name          TEXT NOT NULL COLLATE NOCASE,
    agent_tool_id INTEGER NOT NULL REFERENCES agent_tools(id) ON DELETE CASCADE,
    extra_args    TEXT NOT NULL DEFAULT '[]'
                  CHECK (json_valid(extra_args) AND json_type(extra_args) = 'array'),
    prompt        TEXT NOT NULL DEFAULT '',
    sort_order    INTEGER NOT NULL DEFAULT 0 CHECK (sort_order >= 0),
    created_at    INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at    INTEGER NOT NULL DEFAULT (unixepoch()),
    UNIQUE (profile_id, name)
);

CREATE INDEX agent_templates_profile_order_idx
ON agent_templates(profile_id, sort_order, id);

CREATE INDEX agent_templates_agent_tool_idx
ON agent_templates(agent_tool_id);
