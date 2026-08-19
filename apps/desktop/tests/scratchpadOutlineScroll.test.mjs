import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

const detailFile = new URL('../src/lib/ScratchpadDetailView.svelte', import.meta.url);

test('scratchpad outline exposes stable geometry hooks for the real-app harness', async () => {
  const detail = await readFile(detailFile, 'utf8');
  assert.match(detail, /data-scratchpad-outline=\{closeAfterSelect \? 'mobile' : 'desktop'\}/);
  assert.match(detail, /data-outline-id=\{item\.id\}/);
  assert.match(detail, /<div class="comments-section">/);
});

test('active desktop outline scrolling is scoped to the owned rail', async () => {
  const detail = await readFile(detailFile, 'utf8');
  const outlineTracking = detail.slice(
    detail.indexOf('const activeId = activeOutlineId'),
    detail.indexOf('$effect(() => () => clearSaveTimer')
  );
  assert.match(detail, /let desktopOutlineList = \$state<HTMLElement \| null>\(null\)/);
  assert.match(outlineTracking, /scrollOutlineItemWithinList\(list, active\)/);
  assert.match(outlineTracking, /list\.scrollTo\(\{ top \}\)/);
  assert.doesNotMatch(outlineTracking, /scrollIntoView/);
  assert.doesNotMatch(outlineTracking, /document\.querySelector<HTMLElement>/);
});
