import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

const files = {
  app: new URL('../src/App.svelte', import.meta.url),
  dialog: new URL('../src/lib/AddCommandDialog.svelte', import.meta.url),
  menu: new URL('../src/lib/contextMenu.ts', import.meta.url),
  processPanel: new URL('../src/lib/ProcessPanel.svelte', import.meta.url),
  terminal: new URL('../src/lib/TerminalView.svelte', import.meta.url),
  tree: new URL('../src/lib/ProjectTree.svelte', import.meta.url)
};

test('command row activation selects without spawning a process', async () => {
  const [app, processPanel, terminal, tree] = await Promise.all([
    readFile(files.app, 'utf8'),
    readFile(files.processPanel, 'utf8'),
    readFile(files.terminal, 'utf8'),
    readFile(files.tree, 'utf8')
  ]);

  const rowClass = tree.indexOf('class="tree-row command-row"');
  const rowStart = tree.lastIndexOf('<button', rowClass);
  const rowEnd = tree.indexOf('</button>', rowClass);
  const row = tree.slice(rowStart, rowEnd);
  const actionsStart = tree.indexOf('class="command-actions"', rowEnd);
  const actionsEnd = tree.indexOf('</div>', actionsStart);
  const actions = tree.slice(actionsStart, actionsEnd);
  const selectionStart = app.indexOf('async function selectTreeItem');
  const selectionEnd = app.indexOf('function openClaimedTodo', selectionStart);
  const selection = app.slice(selectionStart, selectionEnd);

  assert.ok(rowClass >= 0 && rowStart >= 0 && rowEnd > rowStart, 'command row exists');
  assert.match(row, /type="button"/);
  assert.match(row, /onclick=\{\(\) => selectProcess\(process\)\}/);
  assert.doesNotMatch(row, /onStartProcess|startProcess/);
  assert.match(actions, /onclick=\{\(\) => onStartProcess\(process\)\}/);
  assert.ok(selectionStart >= 0 && selectionEnd > selectionStart, 'selection handler exists');
  assert.doesNotMatch(selection, /startProcess|startOrReviewProcess|client\.startProcess/);
  assert.match(processPanel, /onclick=\{\(\) => selectProcess\(process\)\}/);
  assert.doesNotMatch(processPanel, /function runOrSelect/);
  assert.match(terminal, /processNeverRun \? 'Run command' : 'Run again'/);
  assert.match(terminal, /class="process-ended-bar"/);
});

test('command rows bind an explicit cancellable contextmenu handler for WKWebView', async () => {
  const [tree, menu] = await Promise.all([
    readFile(files.tree, 'utf8'),
    readFile(files.menu, 'utf8')
  ]);
  const rowStart = tree.indexOf('class="tree-row command-row"');
  const rowEnd = tree.indexOf('</button>', rowStart);
  const row = tree.slice(rowStart, rowEnd);

  assert.ok(rowStart >= 0, 'command row target exists');
  assert.match(row, /data-context-kind="command"/);
  assert.match(row, /oncontextmenu=\{\(event\) => openPointerMenu\(event, processTarget\(process\)\)\}/);
  assert.match(tree, /function openPointerMenu\([\s\S]*contextMenuRequest\(event, target\)/);
  assert.match(menu, /function contextMenuRequest\([\s\S]*event\.preventDefault\(\)/);
});

test('command context menu exposes edit and destructive remove', async () => {
  const menu = await readFile(files.menu, 'utf8');
  assert.match(menu, /id: 'edit-command', label: 'Edit command…'/);
  assert.match(menu, /id: 'remove-command',[\s\S]*label: 'Remove command…',[\s\S]*destructive: true/);
});

test('the edit command dialog is prefilled and shares environment parsing with creation', async () => {
  const dialog = await readFile(files.dialog, 'utf8');
  assert.match(dialog, /initialProcess: ProcessView/);
  assert.match(dialog, /initialProcess\.name/);
  assert.match(dialog, /formatCommandEnvironment\(initialProcess\.env\)/);
  assert.match(dialog, /parseCommandEnvironment\(environment\)/);
  assert.match(dialog, /'config\.command_update'/);
  assert.doesNotMatch(dialog, /config\.command_save|process\.create|onPending|onFailed/);
  assert.match(dialog, /Saved changes apply the next time it starts; the current run is unchanged\./);
});

test('remove confirmation is honest for a running command and uses the durable endpoint', async () => {
  const app = await readFile(files.app, 'utf8');
  assert.match(app, /It is running and will be stopped first\./);
  assert.match(app, /'config\.command_delete'/);
  assert.match(app, /onRemove=\{\(process\) => void removeCommand\(process\)\}/);
});
