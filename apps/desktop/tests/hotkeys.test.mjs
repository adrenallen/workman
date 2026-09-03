import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

import {
  allHotkeyActions,
  defaultHotkeyPreferences,
  findHotkeyAction,
  hotkeyAriaLabel,
  hotkeyDefinitions,
  hotkeyDisplayLabel,
  hotkeyFromKeyboardEvent,
  hotkeyPreferences,
  hotkeyStorageKey,
  loadHotkeyPreferences,
  nativeHotkeyAccelerator,
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

test('defaults every command into one configurable command map', () => {
  const preferences = defaultHotkeyPreferences();
  assert.deepEqual(Object.keys(preferences), [...allHotkeyActions]);
  assert.equal(hotkeyDefinitions.length, allHotkeyActions.length);
  assert.equal(findHotkeyAction(primaryKeyboardEvent('Backquote'), preferences), 'previous-view');
  assert.equal(findHotkeyAction(primaryKeyboardEvent('KeyK'), preferences), 'quick-jump');
  assert.equal(findHotkeyAction(primaryKeyboardEvent('KeyF'), preferences), 'search-terminal');
  assert.equal(findHotkeyAction(primaryKeyboardEvent('ArrowUp'), preferences), 'previous-process');
  assert.equal(findHotkeyAction(primaryKeyboardEvent('Digit1'), preferences), 'project-1');
  assert.equal(findHotkeyAction(primaryKeyboardEvent('Digit9'), preferences), 'project-9');
  assert.equal(findHotkeyAction(primaryKeyboardEvent('KeyN'), preferences), 'new-agent');
  assert.equal(findHotkeyAction(primaryKeyboardEvent('KeyT'), preferences), 'new-terminal');
  assert.equal(
    findHotkeyAction(primaryKeyboardEvent('KeyF', { shiftKey: true }), preferences),
    'start-feedback'
  );
  assert.deepEqual(preferences['new-quick-prompt'], preferences['new-agent']);
});

test('capture requires a safe modifier and reserves only operating-system chords', () => {
  assert.equal(hotkeyFromKeyboardEvent(keyboardEvent('KeyT')), null);
  assert.deepEqual(hotkeyFromKeyboardEvent(keyboardEvent('KeyT', { altKey: true })), {
    code: 'KeyT',
    primary: false,
    secondary: false,
    alt: true,
    shift: false
  });
  assert.deepEqual(hotkeyFromKeyboardEvent(keyboardEvent('F10', { shiftKey: true })), {
    code: 'F10', primary: false, secondary: false, alt: false, shift: true
  });
  for (const code of ['KeyK', 'Backquote', 'KeyF']) {
    assert.equal(reservedHotkeyLabel(hotkeyFromKeyboardEvent(primaryKeyboardEvent(code))), null);
  }
  assert.equal(reservedHotkeyLabel(hotkeyFromKeyboardEvent(primaryKeyboardEvent('KeyQ'))), 'Quit');
  assert.equal(reservedHotkeyLabel(hotkeyFromKeyboardEvent(primaryKeyboardEvent('KeyC'))), 'Copy');
});

test('assigning a used chord moves it to the new action', () => {
  resetHotkeyBindings();
  const agentChord = currentPreferences()['new-agent'];
  assert.equal(setHotkeyBinding('new-terminal', agentChord), 'new-agent');
  assert.equal(currentPreferences()['new-agent'], null);
  assert.equal(currentPreferences()['new-quick-prompt'], null);
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
  assert.match(storage.values.get(hotkeyStorageKey), /"version":3/);

  const malformed = memoryStorage({ [hotkeyStorageKey]: JSON.stringify({ version: 1, bindings: {
    'new-agent': { code: 'KeyN', primary: false, secondary: false, alt: false, shift: false }
  } }) });
  assert.deepEqual(loadHotkeyPreferences(malformed), defaultHotkeyPreferences());
});

test('version one preferences gain primary-T unless that chord was customized', () => {
  const legacy = defaultHotkeyPreferences();
  legacy['new-terminal'] = null;
  const storage = memoryStorage({
    [hotkeyStorageKey]: JSON.stringify({ version: 1, bindings: legacy })
  });
  assert.equal(
    findHotkeyAction(primaryKeyboardEvent('KeyT'), loadHotkeyPreferences(storage)),
    'new-terminal'
  );

  legacy['new-todo'] = {
    code: 'KeyT', primary: true, secondary: false, alt: false, shift: false
  };
  const customized = memoryStorage({
    [hotkeyStorageKey]: JSON.stringify({ version: 1, bindings: legacy })
  });
  const preferences = loadHotkeyPreferences(customized);
  assert.equal(findHotkeyAction(primaryKeyboardEvent('KeyT'), preferences), 'new-todo');
  assert.equal(preferences['new-terminal'], null);
});

test('display labels use the active platform modifier notation', () => {
  assert.equal(
    hotkeyDisplayLabel({ code: 'KeyN', primary: true, secondary: false, alt: false, shift: true }),
    primaryModifierLabel === '⌘'
      ? `${primaryModifierLabel}${shiftModifierLabel}N`
      : `${primaryModifierLabel}+${shiftModifierLabel}+N`
  );
  const chord = { code: 'ArrowUp', primary: true, secondary: false, alt: true, shift: false };
  assert.equal(
    hotkeyAriaLabel(chord),
    primaryModifierLabel === '⌘' ? 'Meta+Alt+ArrowUp' : 'Control+Alt+ArrowUp'
  );
  assert.equal(nativeHotkeyAccelerator(chord), 'CmdOrCtrl+Alt+ArrowUp');
});

test('app and terminal route workspace shortcuts through the shared action resolver', async () => {
  const app = await readFile(new URL('../src/App.svelte', import.meta.url), 'utf8');
  const configured = app.indexOf('if (handleAppShortcut(event)) return;');
  const terminalGuard = app.indexOf('if (isTerminalInputTarget(target)) return;');
  assert.ok(configured >= 0 && terminalGuard > configured);
  for (const action of [
    'previous-view', 'quick-jump', 'keyboard-reference', 'open-settings',
    'toggle-project-rail', 'toggle-project-tree', 'quick-prompts',
    'navigate-left', 'navigate-right', 'previous-process', 'next-process',
    'unfocus-terminal', 'search-terminal'
  ]) assert.match(app, new RegExp(`case '${action}'`));
  assert.match(app, /case 'start-feedback':[\s\S]*openFeedbackPreflight\(\)/);
  assert.match(app, /onAppShortcut=\{handleAppShortcut\}/);
  assert.match(app, /'new-agent': \{ type: 'new-agent', projectId \}/);
  assert.match(app, /'new-terminal': \{ type: 'new-terminal', projectId \}/);
  assert.match(app, /'new-command': \{ type: 'add-command', projectId \}/);
  assert.match(app, /'new-scratchpad': \{ type: 'new-scratchpad', projectId \}/);
  assert.match(app, /'new-todo': \{ type: 'new-todo', projectId \}/);
  assert.match(app, /projectHotkeyHintsVisible = projectHotkeyModifiersActive\(event\)/);
  assert.match(app, /projectHotkeyActions\.some\(\(action\) =>/);
  assert.match(app, /onkeyup=\{handleShortcutKeyup\}/);
  assert.match(app, /onblur=\{hideProjectHotkeyHints\}/);
  assert.match(app, /class:visible=\{projectHotkeyHintsVisible\}/);
  assert.match(app, /\.project-hotkey\.visible \{/);
  assert.match(app, /@media \(prefers-reduced-motion: reduce\)[\s\S]*\.project-hotkey/);
  assert.match(app, /async function spawnTerminal[\s\S]*await tick\(\);[\s\S]*terminalView\?\.focusInput\(\)/);

  const terminal = await readFile(
    new URL('../src/lib/TerminalView.svelte', import.meta.url),
    'utf8'
  );
  assert.match(terminal, /onAppShortcut\?\.\(event\)/);
  assert.match(terminal, /new SearchAddon\(\)/);
  assert.match(terminal, /export function openSearch\(\)/);

  const settings = await readFile(
    new URL('../src/lib/settings/HotkeysCard.svelte', import.meta.url),
    'utf8'
  );
  assert.match(settings, /setHotkeyBinding\(definition\.id, chord\)/);
  assert.match(settings, /resetHotkeyBindings\(\)/);
  assert.match(settings, /suspendNativeMenuAccelerators\(\)/);
  assert.match(settings, /hotkeyDefinitions\.filter/);
  assert.doesNotMatch(settings, /Built-in workspace shortcuts/);
});
