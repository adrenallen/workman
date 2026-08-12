import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

import { singleLineTitle } from '../src/lib/wrappingTitle.ts';

const detailViews = [
  new URL('../src/lib/TodoDetailView.svelte', import.meta.url),
  new URL('../src/lib/ScratchpadDetailView.svelte', import.meta.url)
];

test('todo and scratchpad detail titles wrap and grow with their content', async () => {
  for (const view of await Promise.all(detailViews.map((url) => readFile(url, 'utf8')))) {
    assert.match(view, /<textarea[\s\S]*?class="title"[\s\S]*?rows="1"[\s\S]*?use:autoGrowTextarea=\{titleDraft\}/);
    assert.match(view, /\.title \{[^}]*overflow: hidden;[^}]*resize: none;[^}]*overflow-wrap: anywhere;[^}]*word-break: break-word;[^}]*white-space: pre-wrap;/);
  }
});

test('detail title editors preserve single-line title semantics when text is pasted', () => {
  assert.equal(singleLineTitle('one\r\ntwo\nthree'), 'one two three');
  assert.equal(singleLineTitle('x'.repeat(240)), 'x'.repeat(240));
});
