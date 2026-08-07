import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

const app = readFileSync(new URL('../src/App.svelte', import.meta.url), 'utf8');

test('expanded project rail uses the wide logo and ends with notifications', () => {
  assert.match(app, /workman-logo-wide-transparent\.png/);
  assert.doesNotMatch(app, /local workspaces|brand-copy/);

  const header = app.slice(app.indexOf('<header class="brand"'), app.indexOf('</header>', app.indexOf('<header class="brand"')));
  assert.ok(header.indexOf('class="brand-collapse') < header.indexOf('class="notification-slot"'));
});

test('collapsed header stacks the mark, bell, and discoverable expand control', () => {
  assert.match(app, /grid-template-rows: 28px 28px 24px/);
  assert.match(app, /\.brand-mark \{ grid-row: 1;/);
  assert.match(app, /\.notification-slot \{ grid-row: 2;/);
  assert.match(app, /\.brand-collapse\) \{ grid-row: 3;/);
});
