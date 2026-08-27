import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

import {
  defaultHotkeyPreferences,
  findHotkeyAction,
  hotkeyDisplayLabel,
  hotkeyFromKeyboardEvent,
  hotkeyPreferences,
  hotkeyStorageKey,
  loadHotkeyPreferences,
  reservedHotkeyLabel,
  resetHotkeyBindings,
  saveHotkeyPreferences,
  setHotkeyBinding
} from '../src/lib/hotkeys.ts';
import {
  primaryModifierLabel,
  shiftModifierLabel
} from '../src/lib/primaryModifier.ts';

function keyboardEvent(code, overrides = {}) {
  return {
    code,
    key: code.replace(/^Key/, '').replace(/^Digit/, ''),
    ctrlKey: false,
    metaKey: false,
    altKey: false,
    shiftKey: false,
    ...overrides
  };
}

function primaryKeyboardEvent(code, overrides = {}) {
  return keyboardEvent(code, {
    [primaryModifierLabel === '⌘' ? 'metaKey' : 'ctrlKey']: true,
    ...overrides
  });
}

function memoryStorage(initial = {}) {
  const values = new Map(Object.entries(initial));
  return {
    getItem(key) { return values.get(key) ?? null; },
    setItem(key, value) { values.set(key, value); },
    values
  };
}

function currentPreferences() {
  let value;
  const unsubscribe = hotkeyPreferences.subscribe((next) => { value = next; });
  unsubscribe();
  return value;
}

test('defaults project slots to primary-number and new agent to primary-N', () => {
  const preferences = defaultHotkeyPreferences();
  assert.equal(findHotkeyAction(primaryKeyboardEvent('Digit1'), preferences), 'project-1');
  assert.equal(findHotkeyAction(primaryKeyboardEvent('Digit9'), preferences), 'project-9');
  assert.equal(findHotkeyAction(primaryKeyboardEvent('KeyN'), preferences), 'new-agent');
  assert.equal(preferences['new-terminal'], null);
});

test('capture requires an app-level modifier and fixed workspace chords stay reserved', () => {
  assert.equal(hotkeyFromKeyboardEvent(keyboardEvent('KeyT')), null);
  assert.deepEqual(hotkeyFromKeyboardEvent(keyboardEvent('KeyT', { altKey: true })), {
    code: 'KeyT',
    primary: false,
    secondary: false,
    alt: true,
    shift: false
  });
  const quickJump = hotkeyFromKeyboardEvent(primaryKeyboardEvent('KeyK'));
  assert.equal(reservedHotkeyLabel(quickJump), 'Quick jump');
  const terminalSearch = hotkeyFromKeyboardEvent(primaryKeyboardEvent('KeyF'));
  assert.equal(reservedHotkeyLabel(terminalSearch), 'Search terminal buffer');
  const reorder = hotkeyFromKeyboardEvent(keyboardEvent('ArrowUp', { altKey: true }));
  assert.equal(reservedHotkeyLabel(reorder), 'Reorder focused item');
});

test('assigning a used chord moves it to the new action', () => {
  resetHotkeyBindings();
  const agentChord = currentPreferences()['new-agent'];
  assert.equal(setHotkeyBinding('new-terminal', agentChord), 'new-agent');
  assert.equal(currentPreferences()['new-agent'], null);
  assert.deepEqual(currentPreferences()['new-terminal'], agentChord);
  resetHotkeyBindings();
});

test('preferences persist cleared and custom bindings and reject malformed storage', () => {
  const storage = memoryStorage();
  const preferences = defaultHotkeyPreferences();
  preferences['new-agent'] = null;
  preferences['new-todo'] = {
    code: 'KeyT', primary: true, secondary: false, alt: true, shift: false
  };
  saveHotkeyPreferences(preferences, storage);
  assert.deepEqual(loadHotkeyPreferences(storage), preferences);
  assert.match(storage.values.get(hotkeyStorageKey), /"new-todo"/);

  const malformed = memoryStorage({ [hotkeyStorageKey]: JSON.stringify({ version: 1, bindings: {
    'new-agent': { code: 'KeyN', primary: false, secondary: false, alt: false, shift: false }
  } }) });
  assert.deepEqual(loadHotkeyPreferences(malformed), defaultHotkeyPreferences());
});

test('display labels use the active platform modifier notation', () => {
  assert.equal(
    hotkeyDisplayLabel({ code: 'KeyN', primary: true, secondary: false, alt: false, shift: true }),
    primaryModifierLabel === '⌘'
      ? `${primaryModifierLabel}${shiftModifierLabel}N`
      : `${primaryModifierLabel}+${shiftModifierLabel}+N`
  );
});

test('app and terminal route configured shortcuts through the shared action resolver', async () => {
  const app = await readFile(new URL('../src/App.svelte', import.meta.url), 'utf8');
  const configured = app.indexOf('if (handleConfiguredHotkey(event)) return;');
  const terminalGuard = app.indexOf('if (isTerminalInputTarget(target)) return;');
  assert.ok(configured >= 0 && terminalGuard > configured);
  assert.match(app, /'new-agent': \{ type: 'new-agent', projectId \}/);
  assert.match(app, /'new-terminal': \{ type: 'new-terminal', projectId \}/);
  assert.match(app, /'new-command': \{ type: 'add-command', projectId \}/);
  assert.match(app, /'new-scratchpad': \{ type: 'new-scratchpad', projectId \}/);
  assert.match(app, /'new-todo': \{ type: 'new-todo', projectId \}/);
  assert.match(app, /projectHotkeyHintsVisible = primaryModifier\(event\)/);
  assert.match(app, /onkeyup=\{handleShortcutKeyup\}/);
  assert.match(app, /onblur=\{hideProjectHotkeyHints\}/);
  assert.match(app, /class:visible=\{projectHotkeyHintsVisible\}/);
  assert.match(app, /\.project-hotkey\.visible \{/);
  assert.match(app, /@media \(prefers-reduced-motion: reduce\)[\s\S]*\.project-hotkey/);

  const terminal = await readFile(
    new URL('../src/lib/TerminalView.svelte', import.meta.url),
    'utf8'
  );
  assert.match(terminal, /onAppShortcut\?\.\(event\)/);

  const settings = await readFile(
    new URL('../src/lib/settings/HotkeysCard.svelte', import.meta.url),
    'utf8'
  );
  assert.match(settings, /setHotkeyBinding\(definition\.id, chord\)/);
  assert.match(settings, /resetHotkeyBindings\(\)/);
});
