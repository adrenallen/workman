-- Repair only commands that exactly match defaults shipped without their
-- non-interactive flags. Names, runtime types, and commands must all match so
-- user-customized tools are never rewritten.
UPDATE agent_tools
SET command = 'claude --dangerously-skip-permissions'
WHERE name = 'Claude'
  AND tool_type IN ('claude', 'claude_code')
  AND command = 'claude';

UPDATE agent_tools
SET command = 'codex --dangerously-bypass-approvals-and-sandbox'
WHERE name = 'Codex'
  AND tool_type = 'codex'
  AND command = 'codex';

UPDATE agent_tools
SET command = 'gemini --approval-mode=yolo'
WHERE name = 'Gemini'
  AND tool_type IN ('gemini', 'gemini_cli')
  AND command IN ('gemini', 'gemini --yolo');

UPDATE agent_tools
SET command = 'opencode --auto'
WHERE name = 'OpenCode'
  AND tool_type IN ('opencode', 'open_code')
  AND command = 'opencode';

UPDATE agent_tools
SET command = 'kimi --yolo'
WHERE name = 'Kimi'
  AND tool_type IN ('kimi', 'kimi_code')
  AND command = 'kimi';

UPDATE agent_tools
SET command = 'opencode --auto --model deepseek/deepseek-v4-flash'
WHERE name = 'DeepSeek v4 flash'
  AND tool_type IN ('opencode', 'open_code')
  AND command = 'opencode --model deepseek/deepseek-v4-flash';

-- Kimi and the DeepSeek/OpenCode preset were added after the original four
-- defaults. Existing installations receive them only when those names remain
-- available; any user-owned row with either name wins unchanged.
INSERT INTO agent_tools (name, command, tool_type, enabled, sort_order)
SELECT
    'Kimi',
    'kimi --yolo',
    'kimi',
    1,
    COALESCE((SELECT MAX(sort_order) FROM agent_tools), -1) + 1
WHERE NOT EXISTS (SELECT 1 FROM agent_tools WHERE name = 'Kimi');

INSERT INTO agent_tools (name, command, tool_type, enabled, sort_order)
SELECT
    'DeepSeek v4 flash',
    'opencode --auto --model deepseek/deepseek-v4-flash',
    'opencode',
    1,
    COALESCE((SELECT MAX(sort_order) FROM agent_tools), -1) + 1
WHERE NOT EXISTS (SELECT 1 FROM agent_tools WHERE name = 'DeepSeek v4 flash');
