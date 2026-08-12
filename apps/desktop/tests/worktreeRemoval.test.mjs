import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

test('worktree removal defaults to unregister-only and makes disk deletion explicit', async () => {
  const dialog = await readFile(
    new URL('../src/lib/WorktreeRemoveDialog.svelte', import.meta.url),
    'utf8'
  );
  const app = await readFile(new URL('../src/App.svelte', import.meta.url), 'utf8');

  assert.match(dialog, /let deleteFromDisk = \$state\(false\)/);
  assert.match(dialog, /Also delete from my computer/);
  assert.match(dialog, /This exact folder will be permanently deleted:/);
  assert.match(dialog, /<code class="break-all text-xs">\{entry\.path\}<\/code>/);
  assert.match(dialog, /Remove from Workman/);
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
  assert.match(dialog, /confirmBranch === entry\.branch/);
  assert.match(app, /cause\.code === 'dirty_worktree'/);
  assert.match(app, /force_dirty: forceDirty/);
  assert.match(app, /confirm_branch: confirmBranch/);
});
