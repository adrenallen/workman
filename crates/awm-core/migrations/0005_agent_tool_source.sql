ALTER TABLE agent_tools
ADD COLUMN source TEXT NOT NULL DEFAULT 'local'
    CHECK (source IN ('local', 'config'));

CREATE INDEX agent_tools_source_idx ON agent_tools(source);
