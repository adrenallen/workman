import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

const dialogUrl = new URL('../src/lib/WorktreeDialog.svelte', import.meta.url);
const appUrl = new URL('../src/App.svelte', import.meta.url);
const daemonUrl = new URL('../src/lib/daemon.ts', import.meta.url);

test('new branch flow explains the starting ref and keeps free-text entry', async () => {
  const dialog = await readFile(dialogUrl, 'utf8');

  assert.match(dialog, /Start branch from…/);
  assert.match(dialog, /HEAD = your current checkout state/);
  assert.match(dialog, /the latest remote default branch/);
  assert.match(dialog, /role="combobox"/);
  assert.match(dialog, /aria-autocomplete="list"/);
  assert.match(dialog, /or type any ref/);
  assert.match(dialog, /refOptions/);
  assert.match(dialog, /current.*default.*local.*remote/s);
  assert.match(dialog, /event\.key === 'Escape' && baseRefOpen/);
  assert.match(dialog, /event\.stopPropagation\(\)/);
});

test('detected origin default is applied until the user edits the ref', async () => {
  const dialog = await readFile(dialogUrl, 'utf8');
  const app = await readFile(appUrl, 'utf8');

  assert.match(dialog, /!defaultRef \|\| baseRefTouched \|\| appliedDefaultRef/);
  assert.match(dialog, /baseRef = defaultRef/);
  assert.match(app, /worktreeDefaultRef = response\.default_ref \?\? null/);
  assert.match(app, /if \(mode === 'create'\) void loadOriginBranches\(\)/);
});

test('unknown refs stay inline and cannot start the create operation', async () => {
  const dialog = await readFile(dialogUrl, 'utf8');
  const app = await readFile(appUrl, 'utf8');
  const daemon = await readFile(daemonUrl, 'utf8');

  assert.match(dialog, /await onValidateRef\(value\)/);
  assert.match(dialog, /refValidation = 'invalid'/);
  assert.match(dialog, /id="worktree-base-ref-error"/);
  assert.match(dialog, /refValidationError = cause[\s\S]*?baseRefOpen = false/);
  assert.match(dialog, /createKind === 'new' && !\(await validateBaseRef\(\)\)/);
  assert.match(dialog, /refValidation === 'valid'/);
  assert.match(app, /client\.validateWorktreeRef\(state\.sourceProject\.id, ref\)/);
  assert.match(daemon, /worktree\.ref_validate/);
});

test('create and fork previews state their different commit semantics', async () => {
  const dialog = await readFile(dialogUrl, 'utf8');

  assert.match(dialog, /Creates branch/);
  assert.match(dialog, /\{previewBranch\}/);
  assert.match(dialog, /\{previewRef\}/);
  assert.match(dialog, /· commit/);
  assert.match(dialog, /\{previewCommit\}/);
  assert.match(dialog, /at exact HEAD/);
  assert.match(dialog, /from this worktree's exact HEAD commit/);
});

test('worktree title follows the branch default until edited and is sent with every mode', async () => {
  const dialog = await readFile(dialogUrl, 'utf8');
  const app = await readFile(appUrl, 'utf8');
  const daemon = await readFile(daemonUrl, 'utf8');

  assert.match(dialog, /<span class="text-sm font-medium">Title<\/span>/);
  assert.match(dialog, /defaultWorktreeTitle\(titleBranch\)/);
  assert.match(dialog, /syncProjectTitleDefault\(projectTitle, defaultProjectTitle, projectTitleTouched\)/);
  assert.match(dialog, /function updateProjectTitle\(event: Event\)/);
  assert.equal(dialog.match(/value=\{projectTitle\}/g)?.length, 2);
  assert.equal(dialog.match(/oninput=\{updateProjectTitle\}/g)?.length, 2);
  assert.match(dialog, /onSubmit\(\{ mode, path: adoptPath\.trim\(\), title \}\)/);
  assert.match(dialog, /onSubmit\(\{ mode, branch: nextBranch, title, envPolicy, rememberEnvPolicy \}\)/);
  assert.equal(app.match(/display_name: submission\.title/g)?.length, 2);
  assert.match(app, /adoptWorktreeAsync\(operationId, submission\.path, submission\.title\)/);
  assert.match(daemon, /display_name: displayName/);
});

test('adopt starts at the required path and does not select a user-authored title', async () => {
  const dialog = await readFile(dialogUrl, 'utf8');

  assert.match(dialog, /mode === 'adopt'[\s\S]*?\? adoptPathInput/);
  assert.match(dialog, /onOpenAutoFocus=\{\(event\) =>/);
  assert.match(dialog, /const selectDerivedTitle = !projectTitleTouched/);
  assert.match(dialog, /if \(selectDerivedTitle\) projectTitleInput\?\.select\(\)/);
  assert.match(dialog, /defaultProjectTitleFromPath\(adoptPath, ''\)/);
});
