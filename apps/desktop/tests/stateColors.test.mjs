import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

const root = new URL('../', import.meta.url);

async function source(path) {
  return readFile(new URL(path, root), 'utf8');
}

function luminance(hex) {
  const channels = hex.match(/[a-f\d]{2}/gi).map((part) => Number.parseInt(part, 16) / 255);
  const [red, green, blue] = channels.map((channel) =>
    channel <= 0.04045 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4
  );
  return 0.2126 * red + 0.7152 * green + 0.0722 * blue;
}

function contrast(first, second) {
  const values = [luminance(first), luminance(second)].sort((left, right) => right - left);
  return (values[0] + 0.05) / (values[1] + 0.05);
}

test('waiting uses one orange state token and a transparent stroked clock', async () => {
  const styles = await source('src/styles.css');
  const indicator = await source('src/lib/components/ds/AgentStatusIndicator.svelte');
  const guide = await source('../../STYLE-GUIDE.md');

  assert.match(styles, /--agent-state-waiting: #e58a46;/);
  assert.match(styles, /--agent-state-waiting: #a94f1d;/);
  assert.match(indicator, /\[data-state='waiting'\] \{ color: var\(--agent-state-waiting\); \}/);
  assert.match(indicator, /background: transparent;/);
  assert.doesNotMatch(indicator, /background: var\(--information\)/);
  assert.match(guide, /orange = waiting\/timer/);
  assert.match(guide, /blue = unread\/notification only/);

  assert.ok(contrast('#e58a46', '#1a1e24') >= 4.5, 'dark popover contrast');
  assert.ok(contrast('#a94f1d', '#e9ebee') >= 4.5, 'light sidebar contrast');
});

test('unread blue and active-work green are not reused for waiting', async () => {
  const paths = [
    'src/App.svelte',
    'src/lib/AgentDoneToasts.svelte',
    'src/lib/ProjectTree.svelte',
    'src/lib/WorktreeOperationRow.svelte',
    'src/lib/WorktreeProgressPanel.svelte',
    'src/lib/OptimisticProcessPanel.svelte'
  ];
  const files = Object.fromEntries(await Promise.all(paths.map(async (path) => [path, await source(path)])));

  assert.match(files['src/App.svelte'], /var\(--notification-unread\)/);
  assert.match(files['src/lib/AgentDoneToasts.svelte'], /var\(--notification-unread\)/);
  assert.match(files['src/lib/ProjectTree.svelte'], /var\(--agent-state-waiting\)/);
  assert.match(files['src/lib/ProjectTree.svelte'], /var\(--notification-unread\)/);
  for (const path of paths) {
    assert.doesNotMatch(files[path], /#8fb8ff|#b9d2ff|var\(--information\)/, path);
  }
  for (const path of [
    'src/lib/WorktreeOperationRow.svelte',
    'src/lib/WorktreeProgressPanel.svelte',
    'src/lib/OptimisticProcessPanel.svelte'
  ]) {
    assert.match(files[path], /var\(--agent-state-working\)/, path);
  }
});
