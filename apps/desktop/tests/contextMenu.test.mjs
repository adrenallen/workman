import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

import {
  CONTEXT_ACTION_IDS,
  DESTRUCTIVE_CONTEXT_ACTION_IDS,
  contextActionIcon
} from '../src/lib/contextMenuIcons.ts';
import { terminalContextMenuItems } from '../src/lib/terminalContextMenu.ts';

test('every context action has an explicit icon', () => {
  for (const id of CONTEXT_ACTION_IDS) {
    assert.equal(typeof contextActionIcon(id), 'string', `missing icon for ${id}`);
  }
});

test('project creation actions use their matching section icons', () => {
  assert.deepEqual(
    Object.fromEntries([
      'project-settings',
      'new-agent',
      'new-terminal',
      'add-command',
      'new-todo',
      'new-scratchpad'
    ].map((id) => [id, contextActionIcon(id)])),
    {
      'project-settings': 'settings',
      'new-agent': 'bot',
      'new-terminal': 'square-terminal',
      'add-command': 'play',
      'new-todo': 'circle-check',
      'new-scratchpad': 'notebook-text'
    }
  );
});

test('trash is reserved for destructive actions', () => {
  const trashActions = CONTEXT_ACTION_IDS.filter((id) => contextActionIcon(id) === 'trash-2');
  assert.deepEqual(trashActions, [...DESTRUCTIVE_CONTEXT_ACTION_IDS]);
});

test('terminal surface menu is distinct from the sidebar process menu', () => {
  const terminalMenu = terminalContextMenuItems({
    hasSelection: false,
    link: null,
    pasteEnabled: true
  });

  assert.deepEqual(
    terminalMenu.map((item) => item.id),
    ['terminal-copy', 'terminal-paste', 'terminal-select-all']
  );
  assert.equal(terminalMenu[0].disabled, true);
  assert.equal(terminalMenu[1].shortcut, '⌘V');
  assert.equal(terminalMenu.some((item) => item.id === 'kill'), false);
});

test('terminal link actions appear only for the URL under the pointer', () => {
  const items = terminalContextMenuItems({
    hasSelection: true,
    link: 'https://example.com/docs?q=workman',
    pasteEnabled: true
  });

  assert.deepEqual(
    items.map((item) => item.id),
    [
      'terminal-copy',
      'terminal-paste',
      'terminal-open-link',
      'terminal-copy-link',
      'terminal-select-all'
    ]
  );
  assert.equal(items[0].disabled, false);
});

test('xterm surface owns contextmenu while sidebar rows keep process targets', async () => {
  const terminal = await readFile(new URL('../src/lib/TerminalView.svelte', import.meta.url), 'utf8');
  const tree = await readFile(new URL('../src/lib/ProjectTree.svelte', import.meta.url), 'utf8');
  const app = await readFile(new URL('../src/App.svelte', import.meta.url), 'utf8');

  assert.match(terminal, /oncontextmenu=\{showTerminalContextMenu\}/);
  assert.match(terminal, /contextMenuRequest\(event, \{\s*kind: 'terminal'/);
  assert.match(app, /onContextMenu=\{showContextMenu\}/);
  assert.match(
    tree,
    /oncontextmenu=\{\(event\) => openSelectablePointerMenu\(event, processTarget\(process\), 'agents'/
  );
  assert.match(
    tree,
    /oncontextmenu=\{\(event\) => openSelectablePointerMenu\(event, processTarget\(process\), 'terminals'/
  );
});
