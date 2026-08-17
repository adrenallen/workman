import assert from 'node:assert/strict';
import test from 'node:test';
import ts from 'typescript';
import { ChangeSet } from '@codemirror/state';
import { readFile } from 'node:fs/promises';

const sourcePath = new URL('../src/lib/scratchpadAnchors.ts', import.meta.url);
const source = await readFile(sourcePath, 'utf8');
const output = ts.transpileModule(source, {
  compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2022 }
}).outputText;
const moduleUrl = `data:text/javascript;base64,${Buffer.from(output).toString('base64')}`;
const { mapScratchpadSelectionAnchor, resolveScratchpadAnchor, selectionAnchor } = await import(moduleUrl);

test('reanchors duplicate quotes with context using UTF-16 offsets', () => {
  const original = 'Alpha target Omega\nBeta target Gamma';
  const start = original.lastIndexOf('target');
  assert.deepEqual(
    resolveScratchpadAnchor(original, {
      quote: 'target',
      anchor_start: start,
      anchor_end: start + 6,
      anchor_prefix: 'Beta ',
      anchor_suffix: ' Gamma'
    }),
    {
      anchor_state: 'anchored',
      current_start: start,
      current_end: start + 6,
      current_start_line: 2,
      current_end_line: 2
    }
  );
  const shifted = '😀 heading\nAlpha target Omega\nBeta target Gamma';
  const resolved = resolveScratchpadAnchor(shifted, {
    quote: 'target',
    anchor_start: start,
    anchor_end: start + 6,
    anchor_prefix: 'Beta ',
    anchor_suffix: ' Gamma'
  });
  assert.equal(resolved.anchor_state, 'anchored');
  assert.equal(resolved.current_start, shifted.lastIndexOf('target'));
  assert.equal(resolved.current_start_line, 3);
});

test('orphaned and whole-document vectors agree with the Rust resolver', () => {
  assert.equal(resolveScratchpadAnchor('target then target', {
    quote: 'target', anchor_start: null, anchor_end: null, anchor_prefix: null, anchor_suffix: null
  }).anchor_state, 'orphaned');
  assert.equal(resolveScratchpadAnchor('the sentence was removed', {
    quote: 'target', anchor_start: 0, anchor_end: 6, anchor_prefix: '', anchor_suffix: ''
  }).anchor_state, 'orphaned');
  assert.equal(resolveScratchpadAnchor('anything', {
    quote: null, anchor_start: null, anchor_end: null, anchor_prefix: null, anchor_suffix: null
  }).anchor_state, 'unanchored');
});

test('selection anchors preserve CodeMirror UTF-16 positions and cap Unicode context', () => {
  const content = `${'😀'.repeat(70)} selected ${'é'.repeat(70)}`;
  const start = content.indexOf('selected');
  const anchor = selectionAnchor(content, start, start + 'selected'.length);
  assert.equal(anchor.quote, 'selected');
  assert.equal(anchor.anchor_start, 140 + 1);
  assert.equal(Array.from(anchor.anchor_prefix).length, 64);
  assert.equal(Array.from(anchor.anchor_suffix).length, 64);
});

test('maps a live anchor through CodeMirror changes', () => {
  const content = 'Alpha target Omega';
  const anchor = selectionAnchor(content, 6, 12);
  const changes = ChangeSet.of([{ from: 0, insert: 'Intro\n' }], content.length);
  const next = `Intro\n${content}`;
  const mapped = mapScratchpadSelectionAnchor(anchor, next, changes);
  assert.equal(mapped.quote, 'target');
  assert.equal(mapped.anchor_start, 12);
  assert.equal(mapped.anchor_end, 18);
});
