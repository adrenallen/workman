-- Add Grok to every existing profile without claiming a user-owned display name.
-- The storage name is profile-scoped so it stays unique across profile copies.
WITH missing_profiles AS (
    SELECT
        profiles.id AS profile_id,
        ROW_NUMBER() OVER (ORDER BY profiles.id) AS offset
    FROM profiles
    WHERE NOT EXISTS (
        SELECT 1
        FROM agent_tools
        WHERE agent_tools.profile_id = profiles.id
          AND agent_tools.display_name = 'Grok' COLLATE NOCASE
    )
), numbered_tools AS (
    SELECT
        profile_id,
        (SELECT COALESCE(MAX(id), 0) FROM agent_tools) + offset AS tool_id
    FROM missing_profiles
)
INSERT INTO agent_tools (
    id,
    name,
    display_name,
    command,
    tool_type,
    enabled,
    sort_order,
    resume_args,
    continue_args,
    profile_id
)
SELECT
    numbered_tools.tool_id,
    'profile-' || numbered_tools.profile_id || '-tool-' || numbered_tools.tool_id,
    'Grok',
    'grok --always-approve',
    'grok',
    1,
    COALESCE((SELECT MAX(sort_order) + 1 FROM agent_tools WHERE profile_id = numbered_tools.profile_id), 0),
    '--resume {session_id}',
    '--continue',
    numbered_tools.profile_id
FROM numbered_tools;
