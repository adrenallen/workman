INSERT INTO agent_tools (name, command, tool_type, enabled) VALUES
    ('Claude', 'claude --dangerously-skip-permissions', 'claude_code', 1),
    ('Codex', 'codex --dangerously-bypass-approvals-and-sandbox', 'codex', 1),
    ('Gemini', 'gemini --approval-mode=yolo', 'gemini', 1),
    ('OpenCode', 'opencode --auto', 'opencode', 1),
    ('Kimi', 'kimi --yolo', 'kimi', 1),
    ('DeepSeek v4 flash', 'opencode --auto --model deepseek/deepseek-v4-flash', 'opencode', 1)
ON CONFLICT(name) DO NOTHING;
