INSERT INTO agent_tools (name, command, tool_type, enabled) VALUES
    ('Claude', 'claude', 'claude_code', 1),
    ('Codex', 'codex', 'codex', 1),
    ('Gemini', 'gemini', 'gemini', 1),
    ('OpenCode', 'opencode', 'opencode', 1)
ON CONFLICT(name) DO NOTHING;
