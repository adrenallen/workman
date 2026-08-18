import assert from 'node:assert/strict';
import test from 'node:test';
import { readFile } from 'node:fs/promises';

const editor = await readFile(
  new URL('../src/lib/LiveMarkdownEditor.svelte', import.meta.url),
  'utf8'
);

test('scratchpad selections override CodeMirror focused light-theme defaults with tokens', () => {
  assert.match(
    editor,
    /&\.cm-focused > \.cm-scroller > \.cm-selectionLayer \.cm-selectionBackground'/
  );
  assert.match(
    editor,
    /backgroundColor: 'color-mix\(in srgb, var\(--ring\) 22%, transparent\) !important'/
  );
  assert.doesNotMatch(editor, /#d7d4f0|background(?:Color)?: ['"]Highlight/);
});

test('scratchpad comment states preserve foreground and cap translucent fills', () => {
  const commentStyles = editor.slice(
    editor.indexOf("'.cm-comment-highlight':"),
    editor.indexOf("'.cm-placeholder':")
  );
  assert.match(commentStyles, /color: 'var\(--foreground\)'/);
  assert.match(commentStyles, /'\.cm-comment-highlight\.cm-comment-focused'/);
  assert.match(commentStyles, /'\.cm-comment-highlight\.cm-comment-resolved'/);
  const fills = [...commentStyles.matchAll(/\) (\d+)%, transparent\)/g)]
    .map((match) => Number(match[1]));
  assert.deepEqual(fills, [12, 19, 27, 8]);
  assert.ok(fills.every((percentage) => percentage <= 30));
});
