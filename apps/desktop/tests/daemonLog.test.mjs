import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

import {
  appendDaemonLogEntry,
  DaemonRequestTimeoutError,
  isDaemonRequestTimeoutError
} from '../src/lib/daemonLog.ts';

function event(id, occurredAt, title = 'notifications.list timed out') {
  return {
    id,
    tone: 'warning',
    title,
    detail: 'No response after 5 seconds; connection stayed online.',
    occurredAt
  };
}

test('daemon request timeouts carry the method and deadline for quiet logging', () => {
  const timeout = new DaemonRequestTimeoutError('notifications.list', 5_000);
  assert.equal(isDaemonRequestTimeoutError(timeout), true);
  assert.equal(timeout.message, 'The daemon did not answer in time');
  assert.equal(timeout.method, 'notifications.list');
  assert.equal(timeout.timeoutMs, 5_000);
  assert.equal(isDaemonRequestTimeoutError(new Error(timeout.message)), false);
});

test('repeated daemon notices coalesce while preserving a bounded newest-first log', () => {
  let entries = appendDaemonLogEntry([], event(1, 1_000), 2);
  entries = appendDaemonLogEntry(entries, event(2, 2_000), 2);
  assert.equal(entries.length, 1);
  assert.equal(entries[0].count, 2);
  assert.equal(entries[0].occurredAt, 2_000);

  entries = appendDaemonLogEntry(entries, event(3, 40_000), 2);
  entries = appendDaemonLogEntry(entries, event(4, 41_000, 'projects.list timed out'), 2);
  assert.deepEqual(entries.map((entry) => entry.title), [
    'projects.list timed out',
    'notifications.list timed out'
  ]);
});

test('timeout errors route to the daemon popover while ordinary errors stay prominent', async () => {
  const [app, daemon, statusBar] = await Promise.all([
    readFile(new URL('../src/App.svelte', import.meta.url), 'utf8'),
    readFile(new URL('../src/lib/daemon.ts', import.meta.url), 'utf8'),
    readFile(new URL('../src/lib/ProcessStatusBar.svelte', import.meta.url), 'utf8')
  ]);

  assert.match(daemon, /new DaemonRequestTimeoutError\(type, requestTimeout\)/);
  assert.match(app, /if \(isDaemonRequestTimeoutError\(cause\)\)[\s\S]*recordDaemonLog\([\s\S]*return;/);
  assert.match(app, /error = cause instanceof Error \? cause\.message : String\(cause\);/);
  assert.match(statusBar, /title="Open daemon log"/);
  assert.match(statusBar, /aria-label="Daemon activity log"/);
  assert.match(statusBar, /onClearDaemonEvents/);
});
