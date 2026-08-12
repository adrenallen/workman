import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

test('unified project removal defaults to unregister-only and makes disk deletion explicit', async () => {
  const dialog = await readFile(
    new URL('../src/lib/WorktreeRemoveDialog.svelte', import.meta.url),
    'utf8'
  );
  const app = await readFile(new URL('../src/App.svelte', import.meta.url), 'utf8');

  assert.match(dialog, /let deleteFromDisk = \$state\(false\)/);
  assert.match(dialog, /Also delete from my computer/);
  assert.match(dialog, /This exact folder will be permanently deleted:/);
  assert.match(dialog, /<code class="break-all text-xs">\{path\}<\/code>/);
  assert.match(dialog, /Remove from Workman/);
  assert.match(dialog, /project: Project/);
  assert.match(dialog, /entry\?: WorktreeEntry \| null/);
  assert.match(app, /delete_from_disk: deleteFromDisk/);
});

test('dirty or unpublished work requires a force checkbox and exact branch confirmation', async () => {
  const dialog = await readFile(
    new URL('../src/lib/WorktreeRemoveDialog.svelte', import.meta.url),
    'utf8'
  );
  const app = await readFile(new URL('../src/App.svelte', import.meta.url), 'utf8');

  assert.match(dialog, /safety\?\.requires_force/);
  assert.match(dialog, /safety\.unpushed_commits/);
  assert.match(dialog, /safety\.unmerged_commits/);
  assert.match(dialog, /safety\.ignored_files/);
  assert.match(dialog, /safety\.ignored_paths/);
  assert.match(dialog, /confirmBranch === confirmationText/);
  assert.match(dialog, /safety\.dependent_worktrees/);
  assert.match(app, /cause\.code === 'dirty_worktree'/);
  assert.match(app, /force_dirty: forceDirty/);
  assert.match(app, /confirm_branch: confirmBranch/);
});

test('project removal RPC is reachable only from the explicit in-app dialog confirmation', async () => {
  const app = await readFile(new URL('../src/App.svelte', import.meta.url), 'utf8');
  const menuAction = app.match(/case 'remove-project':([\s\S]*?)case 'open-in-editor':/)?.[1] ?? '';
  const confirmation = app.match(/async function confirmRemoveWorktree\(([\s\S]*?)\n  }/)?.[0] ?? '';

  assert.match(menuAction, /openRemoveWorktree\(project\)/);
  assert.doesNotMatch(menuAction, /client\.control|confirm_remove/);
  assert.match(confirmation, /client\.control\('projects\.remove'/);
  assert.match(confirmation, /confirm_remove: true/);
  assert.match(app, /<WorktreeRemoveDialog/);
  assert.match(app, /onConfirm=\{\(deleteFromDisk, forceDirty, confirmBranch\)/);
  assert.equal((app.match(/client\.control\('projects\.remove'/g) ?? []).length, 1);
});

test('destructive desktop paths contain no native confirm, alert, or prompt calls', async () => {
  const sources = await Promise.all([
    '../src/App.svelte',
    '../src/lib/AgentsPanel.svelte',
    '../src/lib/settings/AgentToolsCard.svelte',
    '../src/lib/settings/ProfilesCard.svelte'
  ].map((path) => readFile(new URL(path, import.meta.url), 'utf8')));

  for (const source of sources) {
    assert.doesNotMatch(source, /window\.(?:confirm|alert|prompt)\s*\(/);
  }
});
