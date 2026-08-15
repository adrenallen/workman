import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

import {
  filterQuickPrompts,
  isQuickPromptPaletteShortcut,
  quickPromptPaletteAction,
  quickPromptPreview,
  sanitizeQuickPromptBody
} from '../src/lib/quickPromptPalette.ts';
import { QuickPromptsStore } from '../src/lib/quickPrompts.ts';

function prompt(id, name, body, sortOrder = id) {
  return {
    id,
    name,
    body,
    sort_order: sortOrder,
    created_at: 1,
    updated_at: 1
  };
}

test('palette actions distinguish insert, insert-and-send, and new prompt', () => {
  assert.equal(isQuickPromptPaletteShortcut({ key: 'p', metaKey: true, shiftKey: true }), true);
  assert.equal(isQuickPromptPaletteShortcut({ key: 'p', metaKey: true }), false);
  assert.equal(
    quickPromptPaletteAction({ key: 'Enter', metaKey: false }),
    'insert'
  );
  assert.equal(
    quickPromptPaletteAction({ key: 'Enter', metaKey: true }),
    'insert-and-send'
  );
  assert.equal(quickPromptPaletteAction({ key: 'n', metaKey: true }), 'new');
  assert.equal(
    quickPromptPaletteAction({ key: 'Enter', metaKey: false, shiftKey: true }),
    null
  );
  assert.equal(
    quickPromptPaletteAction({ key: 'Enter', metaKey: false, isComposing: true }),
    null
  );
  assert.equal(
    quickPromptPaletteAction({ key: 'Enter', metaKey: false, keyCode: 229 }),
    null
  );
});

test('palette actions own arrow and boundary navigation', () => {
  assert.equal(quickPromptPaletteAction({ key: 'ArrowDown', metaKey: false }), 'next');
  assert.equal(quickPromptPaletteAction({ key: 'ArrowUp', metaKey: false }), 'previous');
  assert.equal(quickPromptPaletteAction({ key: 'Home', metaKey: false }), 'first');
  assert.equal(quickPromptPaletteAction({ key: 'End', metaKey: false }), 'last');
  assert.equal(
    quickPromptPaletteAction({ key: 'ArrowDown', metaKey: true }),
    null
  );
  assert.equal(
    quickPromptPaletteAction({ key: 'ArrowDown', metaKey: false, isComposing: true }),
    null
  );
});

test('palette searches names and multiline bodies with compact previews', () => {
  const prompts = [
    prompt(1, 'Release', 'Run all checks'),
    prompt(2, 'Review', 'Find missing\nintegration coverage'),
    prompt(3, 'Summarize', 'Explain the change')
  ];
  assert.deepEqual(filterQuickPrompts(prompts, 'revi').map((candidate) => candidate.id), [2]);
  assert.deepEqual(filterQuickPrompts(prompts, 'int cov').map((candidate) => candidate.id), [2]);
  assert.equal(quickPromptPreview('one\n two\tthree'), 'one two three');
});

test('palette bounds cached body search and strips unsafe terminal controls', () => {
  const insidePrefix = prompt(1, 'First', `${'x'.repeat(1_990)}find-me`);
  const outsidePrefix = prompt(2, 'Second', `${'x'.repeat(2_000)}find-me`);
  assert.deepEqual(filterQuickPrompts([insidePrefix, outsidePrefix], 'find-me').map(({ id }) => id), [1]);
  assert.equal(
    sanitizeQuickPromptBody(`\0one\ttwo\nthree\x1b[201~\rfour\u0085five`),
    'one\ttwo\nthree[201~fourfive'
  );
});

test('quick prompt store publishes daemon CRUD and reorder results', async () => {
  const calls = [];
  let prompts = [prompt(1, 'Review', 'Review this.', 0)];
  const client = {
    async listQuickPrompts() {
      calls.push(['list']);
      return prompts;
    },
    async saveQuickPrompt(input) {
      calls.push(['save', input]);
      const saved = prompt(input.id ?? 2, input.name, input.body, input.id ? 0 : 1);
      prompts = input.id ? prompts.map((candidate) => candidate.id === input.id ? saved : candidate) : [...prompts, saved];
      return saved;
    },
    async deleteQuickPrompt(quickPromptId) {
      calls.push(['delete', quickPromptId]);
      prompts = prompts.filter((candidate) => candidate.id !== quickPromptId);
      return { quick_prompt_id: quickPromptId, deleted: true };
    },
    async reorderQuickPrompts(ids) {
      calls.push(['reorder', ids]);
      prompts = ids.map((id, index) => ({ ...prompts.find((candidate) => candidate.id === id), sort_order: index }));
      return prompts;
    }
  };
  const store = new QuickPromptsStore(client);
  const snapshots = [];
  store.subscribe((snapshot) => snapshots.push(snapshot));

  await store.refresh();
  await store.save({ name: 'Summarize', body: 'Summarize this.' });
  await store.reorder([2, 1]);
  await store.remove(1);

  assert.deepEqual(store.current().prompts.map((candidate) => candidate.id), [2]);
  assert.deepEqual(calls, [
    ['list'],
    ['save', { name: 'Summarize', body: 'Summarize this.' }],
    ['reorder', [2, 1]],
    ['delete', 1]
  ]);
  assert.ok(snapshots.length >= 5);
});

test('app, palette, terminal, and settings wire the complete quick prompt flow', async () => {
  const [app, palette, terminal, settings, sections, card, shortcuts] = await Promise.all([
    readFile(new URL('../src/App.svelte', import.meta.url), 'utf8'),
    readFile(new URL('../src/lib/QuickPromptPalette.svelte', import.meta.url), 'utf8'),
    readFile(new URL('../src/lib/TerminalView.svelte', import.meta.url), 'utf8'),
    readFile(new URL('../src/lib/SettingsPanel.svelte', import.meta.url), 'utf8'),
    readFile(new URL('../src/lib/settingsSections.ts', import.meta.url), 'utf8'),
    readFile(new URL('../src/lib/settings/QuickPromptsCard.svelte', import.meta.url), 'utf8'),
    readFile(new URL('../src/lib/KeyboardShortcuts.svelte', import.meta.url), 'utf8')
  ]);

  assert.match(app, /event\.metaKey && event\.shiftKey[\s\S]*event\.key\.toLowerCase\(\) === 'p'/);
  assert.match(app, /onQuickPrompts=\{openQuickPrompts\}/);
  assert.match(app, /<QuickPromptPalette[\s\S]*canInsert=\{terminalView !== null && selectedProcess\?\.kind === 'agent' && selectedProcess\.status === 'running'\}/);
  assert.match(app, /bind:this=\{terminalView\}/);
  assert.match(palette, /Select a running agent first/);
  assert.match(palette, /<Command\.Input[\s\S]*autofocus/);
  assert.match(palette, /aria-activedescendant=\{activePrompt/);
  assert.match(palette, /scrollIntoView\(\{ block: 'nearest' \}\)/);
  assert.match(palette, /↑↓ · navigate/);
  assert.match(palette, /Enter · insert/);
  assert.match(palette, /⌘Enter · insert &amp; send/);
  assert.match(palette, /<QuickPromptEditor/);
  assert.match(palette, />Retry<\/Button>/);
  assert.match(palette, />New quick prompt<\/Button>/);
  assert.match(terminal, /export function insertQuickPrompt/);
  assert.match(terminal, /attachCustomKeyEventHandler[\s\S]*onQuickPrompts && isQuickPromptPaletteShortcut\(event\)[\s\S]*onQuickPrompts\(\)[\s\S]*return false/);
  assert.match(terminal, /instance\.paste\(sanitizeQuickPromptBody\(text\)\)/);
  assert.match(terminal, /if \(submit\)[\s\S]*encoder\.encode\('\\r'\)/);
  assert.match(settings, /<QuickPromptsCard/);
  assert.match(sections, /id: 'quick-prompts'/);
  for (const action of ['New prompt', 'Edit', 'Delete', 'Move']) assert.match(card, new RegExp(action));
  for (const hint of [
    'Open quick prompts for the selected agent',
    'Move between prompts · Home/End first or last',
    'Insert the highlighted prompt without sending',
    'Insert and send',
    'Create a quick prompt from the palette',
    'Close the palette'
  ]) assert.match(card, new RegExp(hint));
  assert.match(shortcuts, /\['⌘', '⇧', 'P'\], label: 'Open quick prompts for the selected agent'/);
});
