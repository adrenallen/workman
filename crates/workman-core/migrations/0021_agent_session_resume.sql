ALTER TABLE agent_tools ADD COLUMN resume_args TEXT;
ALTER TABLE agent_tools ADD COLUMN continue_args TEXT;

-- Only exact built-in presets receive implicit resume behavior. Custom commands
-- stay opt-in through config.yml even when they use the same runtime type.
UPDATE agent_tools
SET resume_args = '--resume {session_id}', continue_args = '--continue'
WHERE name = 'Claude'
  AND tool_type IN ('claude', 'claude_code')
  AND command = 'claude --dangerously-skip-permissions';

UPDATE agent_tools
SET resume_args = 'resume {session_id}', continue_args = 'resume --last'
WHERE name = 'Codex'
  AND tool_type = 'codex'
  AND command = 'codex --dangerously-bypass-approvals-and-sandbox';

UPDATE agent_tools
SET continue_args = '--resume latest'
WHERE name = 'Gemini'
  AND tool_type IN ('gemini', 'gemini_cli')
  AND command = 'gemini --approval-mode=yolo';

UPDATE agent_tools
SET resume_args = '--session {session_id}', continue_args = '--continue'
WHERE name IN ('OpenCode', 'DeepSeek v4 flash')
  AND tool_type IN ('opencode', 'open_code')
  AND command IN (
    'opencode --auto',
    'opencode --auto --model deepseek/deepseek-v4-flash'
  );

UPDATE agent_tools
SET resume_args = '--session {session_id}', continue_args = '--continue'
WHERE name = 'Kimi'
  AND tool_type IN ('kimi', 'kimi_code')
  AND command = 'kimi --yolo';

CREATE TABLE process_agent_sessions (
    process_id   INTEGER PRIMARY KEY REFERENCES processes(id) ON DELETE CASCADE,
    session_id   TEXT,
    launch_mode  TEXT NOT NULL CHECK (launch_mode IN ('fresh', 'continued_latest', 'resumed_session')),
    launched_at  INTEGER NOT NULL,
    captured_at  INTEGER
);
