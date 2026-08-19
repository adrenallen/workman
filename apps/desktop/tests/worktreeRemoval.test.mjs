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
  assert.match(app, /fallbackCause\.code === 'dirty_worktree'/);
  assert.match(app, /force_dirty: forceDirty/);
  assert.doesNotMatch(app, /confirm_branch: confirmBranch/);
});

test('broken or duplicate registrations are unregistered with files untouched', async () => {
  const dialog = await readFile(
    new URL('../src/lib/WorktreeRemoveDialog.svelte', import.meta.url),
    'utf8'
  );

  const app = await readFile(new URL('../src/App.svelte', import.meta.url), 'utf8');

  assert.match(dialog, /entry\?\.status === 'missing'/);
  assert.doesNotMatch(dialog, /entry === null/);
  assert.match(dialog, /This worktree is known to be missing/);
  assert.match(dialog, /Registration cleanup only/);
  assert.match(dialog, /Git worktree removal will not run/);
  assert.match(dialog, /disabled=\{knownMissingRegistration\}/);
  assert.match(dialog, /if \(knownMissingRegistration\) deleteFromDisk = false/);
  assert.match(dialog, /files stay untouched/);
  assert.match(app, /client\.control<WorktreeRemoval>\('projects\.remove'/);
  assert.match(app, /client\.removeWorktreeAsync\(operationId/);
  assert.match(app, /removal\.registration_issue \|\| \(deleteFromDisk && removal\.files_untouched\)/);
  assert.match(app, /Files left untouched at \$\{removal\.path\}/);
  assert.match(app, /class="remove-worktree-notice"/);
});

test('project removal RPC is reachable only from the explicit in-app dialog confirmation', async () => {
  const app = await readFile(new URL('../src/App.svelte', import.meta.url), 'utf8');
  const menuAction = app.match(/case 'remove-project':([\s\S]*?)case 'open-in-editor':/)?.[1] ?? '';
  const confirmation = app.match(/async function confirmRemoveWorktree\(([\s\S]*?)\n  }/)?.[0] ?? '';

  assert.match(menuAction, /openRemoveWorktree\(project\)/);
  assert.doesNotMatch(menuAction, /client\.control|confirm_remove/);
  assert.match(confirmation, /beginWorktreeOperation\(\{/);
  assert.match(confirmation, /mode: 'remove'/);
  assert.match(confirmation, /client\.removeWorktreeAsync\(operationId/);
  assert.match(confirmation, /confirm_remove: true/);
  assert.match(confirmation, /removeWorktreeDialog = null/);
  assert.match(confirmation, /catch \(cause\)/);
  assert.match(app, /failWorktreeOperation\([\s\S]*operationId/);
  assert.match(app, /<WorktreeRemoveDialog/);
  assert.match(app, /onConfirm=\{\(deleteFromDisk, forceDirty\)/);
  assert.equal((app.match(/client\.control<WorktreeRemoval>\('projects\.remove'/g) ?? []).length, 1);
});

test('async removal closes the dialog, stays visible in the operation rail, and reconciles ghost rows', async () => {
  const app = await readFile(new URL('../src/App.svelte', import.meta.url), 'utf8');
  const daemon = await readFile(new URL('../src/lib/daemon.ts', import.meta.url), 'utf8');

  const close = app.indexOf('removeWorktreeDialog = null;', app.indexOf('async function confirmRemoveWorktree'));
  const start = app.indexOf('await client.removeWorktreeAsync(operationId', close);
  assert.ok(close > 0 && start > close, 'the dialog closes before awaiting the async acknowledgement');
  assert.match(daemon, /request\('worktree\.remove_async'/);
  assert.match(app, /operation\.status === 'completed'[\s\S]*\(operation\.project \|\| operation\.removal\)/);
  assert.match(app, /if \(operation\.removal\)[\s\S]*projects = await client\.projects\(\)/);
  assert.match(app, /function unattachedWorktreeOperations\(\)/);
  assert.match(app, /\{#each unattachedWorktreeOperations\(\) as operation/);
  assert.match(app, /openRemoveWorktree\(source, operation\.error_code === 'dirty_worktree'\)/);
  assert.match(app, /class:empty=\{selectedProject === null && activeWorktreeOperation === null\}/);
});

test('async keep-files completion stays quiet and running removals cannot be dismissed', async () => {
  const [app, panel] = await Promise.all([
    readFile(new URL('../src/App.svelte', import.meta.url), 'utf8'),
    readFile(new URL('../src/lib/WorktreeProgressPanel.svelte', import.meta.url), 'utf8')
  ]);

  assert.match(app, /operation\.removal\.registration_issue\s*\|\| \(operation\.removal\.delete_from_disk && operation\.removal\.files_untouched\)/);
  assert.match(panel, /operation\.removal\?\.delete_from_disk && operation\.removal\.files_untouched/);
  assert.match(panel, /operation\.mode === 'remove' && operation\.status === 'running'/);
  assert.match(panel, /Removal continues in the background and can be closed after it finishes/);
  assert.match(app, /cause\.code === 'worktree_operation_in_progress'/);
  assert.match(app, /Removal already in progress for this project/);
});

test('legacy synchronous removal timeout copy says the operation may still complete', async () => {
  const app = await readFile(new URL('../src/App.svelte', import.meta.url), 'utf8');
  assert.match(app, /isDaemonRequestTimeoutError\(fallbackCause\)/);
  assert.match(app, /is taking longer than the request window and may still complete/);
  assert.match(app, /Workman will refresh the project list to reconcile the result/);
  assert.doesNotMatch(app, /Removal of .*daemon did not respond/);
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
