ALTER TABLE agent_tools
ADD COLUMN mcp_tools_profile TEXT NOT NULL DEFAULT 'core'
CHECK (mcp_tools_profile IN ('core', 'extended'));

ALTER TABLE agent_templates
ADD COLUMN mcp_tools_profile TEXT NOT NULL DEFAULT 'core'
CHECK (mcp_tools_profile IN ('core', 'extended'));

-- A process snapshots its resolved profile at launch. Editing the source tool or
-- template therefore never changes an existing agent's MCP surface.
ALTER TABLE processes
ADD COLUMN mcp_tools_profile TEXT NOT NULL DEFAULT 'core'
CHECK (mcp_tools_profile IN ('core', 'extended'));
