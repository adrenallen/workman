import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

const [detail, editor] = await Promise.all([
  readFile(new URL('../src/lib/ScratchpadDetailView.svelte', import.meta.url), 'utf8'),
  readFile(new URL('../src/lib/LiveMarkdownEditor.svelte', import.meta.url), 'utf8')
]);

test('new scratchpad comments open from the clicked selection point', () => {
  assert.match(editor, /onCommentSelection\?\.\(anchor, \{\s*x: event\.clientX \|\| x,\s*y: event\.clientY \|\| y/);
  assert.match(detail, /class="comment-composer-anchor"/);
  assert.match(detail, /customAnchor=\{commentComposerPopoverAnchor\}/);
  assert.match(detail, /onCommentSelection=\{\(anchor, point\) => beginComment\(anchor, point\)\}/);

  const sidebarComposer = detail.slice(
    detail.indexOf('{#snippet commentsPanelContent()}'),
    detail.indexOf('{#snippet commentComposerContent()}')
  );
  assert.doesNotMatch(sidebarComposer, /class="comment-composer"/,
    'the composer should float at its anchor instead of expanding the comments rail');
});

test('the scratchpad comment popover is resizable and keeps its editor usable', () => {
  assert.match(detail, /comment-composer-popover\.comment-composer-popover[^}]+resize: both/);
  assert.match(detail, /comment-composer :global\(textarea\)[^}]+height: 100%[^}]+resize: none/);
  assert.match(detail, /Drag corner to resize/);
  assert.match(detail, /data-scratchpad-comment-composer[^>]+autofocus/s);
  assert.match(detail, /if \(!open && !commentBusy\) cancelComment\(\)/);
});
