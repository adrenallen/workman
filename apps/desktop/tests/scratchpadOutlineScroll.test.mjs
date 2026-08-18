import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

const detailFile = new URL('../src/lib/ScratchpadDetailView.svelte', import.meta.url);

test('desktop scratchpad outline is viewport-bounded and independently scrollable', async () => {
  const detail = await readFile(detailFile, 'utf8');
  assert.match(detail, /\.outline-rail \{[^}]*position: sticky/);
  assert.match(detail, /\.outline-rail \{[^}]*max-height: calc\(100vh - 36px\)/);
  assert.match(detail, /grid-template-rows: minmax\(112px, 1fr\) auto/);
  assert.match(detail, /\.outline-list\[data-scratchpad-outline='desktop'\] \{[^}]*overflow-y: auto/);
  assert.match(detail, /scrollbar-gutter: stable/);
  assert.match(detail, /<div class="comments-section">/);
});

test('active desktop outline item scrolls into view with nearest alignment', async () => {
  const detail = await readFile(detailFile, 'utf8');
  assert.match(detail, /data-scratchpad-outline=\{closeAfterSelect \? 'mobile' : 'desktop'\}/);
  assert.match(detail, /data-outline-id=\{item\.id\}/);
  assert.match(detail, /\$effect\(\(\) => \{[\s\S]*const activeId = activeOutlineId/);
  assert.match(detail, /active\?\.scrollIntoView\(\{ block: 'nearest' \}\)/);
});
