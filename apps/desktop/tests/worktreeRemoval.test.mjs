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
  assert.match(dialog, /role="alert" aria-live="assertive"/);
  assert.match(dialog, /Project removal failed: \{error\}/);
});

test('pending local work is listed and requires one explicit Delete anyway click', async () => {
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
  assert.match(dialog, /safety\.dirty_paths\.slice\(0, 6\)/);
  assert.match(dialog, /safety\.unpushed_subjects/);
  assert.match(dialog, /safety\.unmerged_subjects/);
  assert.match(dialog, /Delete anyway/);
  assert.doesNotMatch(dialog, /confirmBranch|Type <code>|Allow forced deletion/);
  assert.match(dialog, /safety\.dependent_worktrees/);
  assert.match(app, /cause\.code === 'dirty_worktree'/);
  assert.match(app, /force_dirty: forceDirty/);
  assert.doesNotMatch(app, /confirm_branch: confirmBranch/);
});

test('broken or duplicate registrations are unregistered with files untouched', async () => {
  const dialog = await readFile(
    new URL('../src/lib/WorktreeRemoveDialog.svelte', import.meta.url),
    'utf8'
  );

  assert.match(dialog, /project\.repository_id !== null && entry === null/);
  assert.match(dialog, /This entry is broken or duplicates another project/);
  assert.match(dialog, /Registration cleanup only/);
  assert.match(dialog, /Git worktree removal will not run/);
  assert.match(dialog, /disabled=\{brokenRegistration\}/);
  assert.match(dialog, /if \(brokenRegistration\) deleteFromDisk = false/);
  assert.match(dialog, /files stay untouched/);
});

test('project removal RPC is reachable only from the explicit in-app dialog confirmation', async () => {
  const app = await readFile(new URL('../src/App.svelte', import.meta.url), 'utf8');
  const menuAction = app.match(/case 'remove-project':([\s\S]*?)case 'open-in-editor':/)?.[1] ?? '';
  const confirmation = app.match(/async function confirmRemoveWorktree\(([\s\S]*?)\n  }/)?.[0] ?? '';

  assert.match(menuAction, /openRemoveWorktree\(project\)/);
  assert.doesNotMatch(menuAction, /client\.control|confirm_remove/);
  assert.match(confirmation, /client\.control\('projects\.remove'/);
  assert.match(confirmation, /confirm_remove: true/);
  assert.match(confirmation, /removeWorktreeDialog = null/);
  assert.match(confirmation, /catch \(cause\)/);
  assert.match(confirmation, /removeWorktreeError = cause instanceof Error/);
  assert.match(app, /<WorktreeRemoveDialog/);
  assert.match(app, /onConfirm=\{\(deleteFromDisk, forceDirty\)/);
  assert.equal((app.match(/client\.control\('projects\.remove'/g) ?? []).length, 1);
});

test('removal, confirmation, and quick jump dialogs use wider responsive bounds', async () => {
  const [removal, confirmation, quickJump] = await Promise.all([
    readFile(new URL('../src/lib/WorktreeRemoveDialog.svelte', import.meta.url), 'utf8'),
    readFile(new URL('../src/lib/ConfirmationDialog.svelte', import.meta.url), 'utf8'),
    readFile(new URL('../src/lib/QuickJumpPalette.svelte', import.meta.url), 'utf8')
  ]);

  assert.match(removal, /min\(760px,calc\(100vw-24px\)\)/);
  assert.match(confirmation, /min\(620px,calc\(100vw-24px\)\)/);
  assert.match(quickJump, /min\(840px,calc\(100vw-24px\)\)/);
  assert.match(removal, /!max-w-none/);
  assert.match(confirmation, /!max-w-none/);
  assert.match(quickJump, /!max-w-none/);
  assert.match(quickJump, /@media \(max-width: 620px\)/);
});

test('destructive desktop paths contain no native confirm, alert, or prompt calls', async () => {
  const sources = await Promise.all([
    '../src/App.svelte',
    '../src/lib/settings/AgentToolsCard.svelte',
    '../src/lib/settings/ProfilesCard.svelte'
  ].map((path) => readFile(new URL(path, import.meta.url), 'utf8')));

  for (const source of sources) {
    assert.doesNotMatch(source, /window\.(?:confirm|alert|prompt)\s*\(/);
  }
});
