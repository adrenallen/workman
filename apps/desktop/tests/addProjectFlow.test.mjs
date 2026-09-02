import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

const appUrl = new URL('../src/App.svelte', import.meta.url);
const dialogUrl = new URL('../src/lib/AddProjectDialog.svelte', import.meta.url);
const titleDialogUrl = new URL('../src/lib/RegisterProjectDialog.svelte', import.meta.url);

test('project entry points open a two-path Add project chooser', async () => {
  const [app, dialog] = await Promise.all([
    readFile(appUrl, 'utf8'),
    readFile(dialogUrl, 'utf8')
  ]);

  assert.match(app, /function showAddProject\(\)[\s\S]*addProjectDialogOpen = true/);
  assert.match(app, /<AddProjectDialog[\s\S]*onChooseFolder=[\s\S]*onCreateWorktree=/);
  assert.match(dialog, /<Dialog\.Title class="text-base">\{step === 'kind' \? 'Add project' : 'Create a worktree'\}<\/Dialog\.Title>/);
  assert.match(dialog, /<strong>\{folderBusy \? 'Opening folder picker…' : 'Choose a folder'\}<\/strong>/);
  assert.match(dialog, /<strong>Create a worktree<\/strong>/);
  assert.doesNotMatch(app, />Register project</);
  assert.doesNotMatch(app, />Register folder</);
});

test('folder choice retains the chooser on cancel and advances to project naming on selection', async () => {
  const app = await readFile(appUrl, 'utf8');
  const handlerStart = app.indexOf('async function chooseFolderFromAddProject');
  const handlerEnd = app.indexOf('function returnToAddProject', handlerStart);
  const handler = app.slice(handlerStart, handlerEnd);

  assert.ok(handlerStart >= 0 && handlerEnd > handlerStart, 'folder handler exists');
  assert.match(handler, /const path = await chooseRegisterProjectFolder\(\)/);
  assert.match(handler, /if \(!path\) return/);
  assert.ok(
    handler.indexOf('if (!path) return') < handler.indexOf('addProjectDialogOpen = false'),
    'cancel returns before the chooser is closed'
  );
  assert.match(handler, /addProjectDialogOpen = false;[\s\S]*showRegisterProjectTitle\(path\)/);
});

test('worktree choice lists only Git-backed top-level projects and opens the existing creator', async () => {
  const [app, dialog] = await Promise.all([
    readFile(appUrl, 'utf8'),
    readFile(dialogUrl, 'utf8')
  ]);

  assert.match(dialog, /project\.parent_project_id === null && project\.repository_id !== null/);
  assert.match(dialog, /\{#each worktreeSources as project \(project\.id\)\}/);
  assert.match(dialog, /projectDisplayName\(project\)/);
  assert.match(dialog, /disabled=\{busy \|\| worktreeSources\.length === 0\}/);
  assert.match(dialog, /Add a Git repository first to create worktrees\./);
  assert.match(app, /async function createWorktreeFromAddProject\(project: Project\)[\s\S]*await openWorktreeDialog\('create', project\)/);
  assert.match(app, /if \(worktreeDialog\) addProjectDialogOpen = false/);
});

test('the folder naming step uses Add project language and returns to the action chooser', async () => {
  const [app, dialog] = await Promise.all([
    readFile(appUrl, 'utf8'),
    readFile(titleDialogUrl, 'utf8')
  ]);

  assert.match(dialog, /busy \? 'Adding…' : 'Add project'/);
  assert.match(dialog, /Esc adds as/);
  assert.match(app, /onBack=\{returnToAddProject\}/);
});
