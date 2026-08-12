import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

const files = {
  app: new URL('../src/App.svelte', import.meta.url),
  dialog: new URL('../src/lib/AddCommandDialog.svelte', import.meta.url),
  menu: new URL('../src/lib/contextMenu.ts', import.meta.url),
  tree: new URL('../src/lib/ProjectTree.svelte', import.meta.url)
};

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

test('the shared command dialog is prefilled for edit and preserves the full definition', async () => {
  const dialog = await readFile(files.dialog, 'utf8');
  assert.match(dialog, /initialProcess\?: ProcessView \| null/);
  assert.match(dialog, /initialProcess\?\.name/);
  assert.match(dialog, /initialProcess\?\.env/);
  assert.match(dialog, /initialProcess\?\.restart_when_changed/);
  assert.match(dialog, /'config\.command_update'/);
  assert.match(dialog, /Saved changes apply the next time it starts; the current run is unchanged\./);
});

test('remove confirmation is honest for a running command and uses the durable endpoint', async () => {
  const app = await readFile(files.app, 'utf8');
  assert.match(app, /It is running and will be stopped first\./);
  assert.match(app, /'config\.command_delete'/);
  assert.match(app, /onRemove=\{\(process\) => void removeCommand\(process\)\}/);
});
