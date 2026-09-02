import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

import {
  defaultHotkeyPreferences,
  findHotkeyAction,
  matchesHotkeyAction
} from '../src/lib/hotkeys.ts';
import { primaryModifierLabel } from '../src/lib/primaryModifier.ts';

// The cycle chord follows the platform primary modifier: Command on macOS,
// Control elsewhere. The excluded modifier is the other platform's primary.
const primary = primaryModifierLabel === '⌘' ? 'metaKey' : 'ctrlKey';
const secondary = primary === 'metaKey' ? 'ctrlKey' : 'metaKey';

function keyEvent(key, overrides = {}) {
  return {
    key,
    code: key,
    altKey: false,
    ctrlKey: false,
    metaKey: false,
    shiftKey: false,
    [primary]: true,
    ...overrides
  };
}

test('process cycling follows the configurable previous and next bindings', () => {
  const preferences = defaultHotkeyPreferences();
  assert.equal(findHotkeyAction(keyEvent('ArrowUp'), preferences), 'previous-process');
  assert.equal(findHotkeyAction(keyEvent('ArrowDown'), preferences), 'next-process');

  for (const modifier of ['altKey', secondary, 'shiftKey']) {
    assert.equal(matchesHotkeyAction(keyEvent('ArrowUp', { [modifier]: true }), 'previous-process', preferences), false);
    assert.equal(matchesHotkeyAction(keyEvent('ArrowDown', { [modifier]: true }), 'next-process', preferences), false);
  }

  assert.equal(matchesHotkeyAction(keyEvent('ArrowLeft'), 'previous-process', preferences), false);
  assert.equal(matchesHotkeyAction(keyEvent('PageDown'), 'next-process', preferences), false);
  assert.equal(matchesHotkeyAction(keyEvent('ArrowDown', { [primary]: false }), 'next-process', preferences), false);
});

test('terminal routes configured app shortcuts before user-key tracking and encoding', async () => {
  const terminal = await readFile(
    new URL('../src/lib/TerminalView.svelte', import.meta.url),
    'utf8'
  );
  const handler = terminal.indexOf('instance.attachCustomKeyEventHandler');
  const shortcut = terminal.indexOf('onAppShortcut?.(event)', handler);
  const userKey = terminal.indexOf('let userKeyToken', handler);
  const encode = terminal.indexOf('encodeTerminalKey(event', handler);

  assert.ok(handler >= 0 && shortcut > handler && userKey > shortcut && encode > userKey);
  assert.match(terminal, /event\.type === 'keydown' && onAppShortcut\?\.\(event\)[\s\S]*return false/);
});

test('app maps configured process actions back to the main frame', async () => {
  const [app, keyboard] = await Promise.all([
    readFile(new URL('../src/App.svelte', import.meta.url), 'utf8'),
    readFile(new URL('../src/lib/keyboardNavigation.ts', import.meta.url), 'utf8')
  ]);

  assert.match(app, /case 'previous-process':[\s\S]*case 'next-process':[\s\S]*cycleProcess\(action === 'previous-process' \? -1 : 1, panelForTarget\(target\)\)/);
  assert.match(app, /onAppShortcut=\{handleAppShortcut\}/);
  assert.match(app, /returnPanel === 'main'[\s\S]*tick\(\)\.then[\s\S]*focusPanel\('main'\)[\s\S]*terminalView\?\.focusInput\(\)/);
  assert.match(app, /const draftTarget = target\?\.closest\('\[data-creation-draft\]'\) !== null[\s\S]*action === 'previous-process' \|\| action === 'next-process'[\s\S]*&& !draftTarget/);
  const configuredHandler = app.slice(
    app.indexOf('function handleConfiguredHotkey'),
    app.indexOf('function projectHotkeyLabel')
  );
  assert.doesNotMatch(configuredHandler, /event\.key === 'Arrow(?:Up|Down|Left|Right)'/);
  assert.match(configuredHandler, /case 'navigate-left':[\s\S]*case 'navigate-right':[\s\S]*focusAdjacentPanel/);
  assert.match(app, /draftFocusRequestId = null;[\s\S]*selectTreeItem\(nextSelection\)/);
  assert.match(keyboard, /'\.draft-row\.selected, \.tree-row\.selected'/);
});
