import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

const centerUrl = new URL('../src/lib/NotificationsCenter.svelte', import.meta.url);

test('notification rows show project icons, worktree markers, names, and aligned fallbacks', async () => {
  const center = await readFile(centerUrl, 'utf8');

  assert.match(center, /projects: Project\[\]/);
  assert.match(center, /projects\.find\(\(project\) => project\.id === notification\.project_id\)/);
  assert.match(center, /project\.display_name\?\.trim\(\) \|\| project\.name\.trim\(\) \|\| project\.path/);
  assert.match(center, /<TooltipLabel[\s\S]*label=\{`\$\{attachedProjectName\} · \$\{notification\.body\}`\}[\s\S]*side="left"[\s\S]*tabindex=\{-1\}/);
  assert.match(center, /aria-label=\{`\$\{notificationTypeLabel\(notification\)\} · \$\{attachedProjectName\}\$\{project\?\.parent_project_id[\s\S]*' · Worktree'/);
  assert.match(center, /worktree=\{project\.parent_project_id !== null\}/);
  assert.match(center, /worktreeTooltip=\{false\}/);
  assert.match(center, /class="notification-icons" aria-hidden="true"/);
  assert.match(center, /class="notification-type-icon"/);
  assert.match(center, /notification\.type === 'needs_input'[\s\S]*CircleHelpIcon/);
  assert.match(center, /notification\.type === 'todo_assigned_to_you'[\s\S]*UserRoundCheckIcon/);
  assert.match(center, /notification\.type === 'mentioned_in_comment'[\s\S]*AtSignIcon/);
  assert.match(center, /\.notification-icons \{[^}]*width: 38px;[^}]*height: 24px;/);
  assert.match(center, /Project no longer registered/);
  assert.match(center, /No project/);
  assert.match(center, /\{:else\}\s*<BellIcon/);
});
