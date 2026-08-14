import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

import {
  deleteProjectTimer,
  listProjectTimers,
  timerKindLabel,
  timerStateLabel,
  updateProjectTimer
} from '../src/lib/timerManagement.ts';

const panelUrl = new URL('../src/lib/TimerPanel.svelte', import.meta.url);
const statusBarUrl = new URL('../src/lib/ProcessStatusBar.svelte', import.meta.url);

function timer(overrides = {}) {
  return {
    id: 7,
    owner_process_id: 41,
    owner_process_name: 'orchestrator',
    owner_label: 'orchestrator',
    delivery_process_id: 41,
    body: 'Review worker status',
    kind: 'delay',
    watch_process_ids: [],
    interval_ms: null,
    repeating: false,
    max_wait_deadline: 20_000,
    paused: false,
    fired: false,
    fired_at: null,
    created_at: 1_000,
    due_at: 20_000,
    paused_at: null,
    ...overrides
  };
}

test('timer control client wires list, update, pause, and delete to project-scoped RPCs', async () => {
  const calls = [];
  const client = {
    async control(method, params) {
      calls.push([method, params]);
      if (method === 'timer.list') return { project_id: 6, timers: [timer()] };
      if (method === 'timer.update') return { project_id: 6, timer: timer({ ...params }) };
      return { project_id: 6, timer_id: params.timer_id, deleted: true };
    }
  };

  assert.equal((await listProjectTimers(client, 6))[0].id, 7);
  assert.equal(
    (await updateProjectTimer(client, 6, 7, { delay_ms: 30_000, body: 'New body' })).body,
    'New body'
  );
  assert.equal((await updateProjectTimer(client, 6, 7, { paused: true })).paused, true);
  await deleteProjectTimer(client, 6, 7);

  assert.deepEqual(calls, [
    ['timer.list', { project_id: 6 }],
    ['timer.update', { project_id: 6, timer_id: 7, delay_ms: 30_000, body: 'New body' }],
    ['timer.update', { project_id: 6, timer_id: 7, paused: true }],
    ['timer.delete', { project_id: 6, timer_id: 7 }]
  ]);
});

test('timer rows expose readable kind and state labels', () => {
  assert.equal(timerKindLabel(timer()), 'One-shot');
  assert.equal(timerKindLabel(timer({ repeating: true })), 'Recurring');
  assert.equal(timerKindLabel(timer({ kind: 'idle_any' })), 'Idle · any');
  assert.equal(timerStateLabel(timer({ paused: true })), 'Paused');
  assert.equal(timerStateLabel(timer({ kind: 'idle_all' })), 'Watching');
  assert.equal(timerStateLabel(timer({ fired: true })), 'Fired');
});

test('timer panel renders project metadata and wires safe in-app actions', async () => {
  const [panel, statusBar] = await Promise.all([
    readFile(panelUrl, 'utf8'),
    readFile(statusBarUrl, 'utf8')
  ]);

  for (const label of ['Owner', 'Delivery', 'Watching', 'Interval', 'Pause', 'Resume', 'Edit', 'Delete']) {
    assert.match(panel, new RegExp(label));
  }
  assert.match(panel, /ConfirmationDialog/);
  assert.match(panel, /Delete timer/);
  assert.doesNotMatch(panel, /window\.confirm|\bconfirm\(/);
  assert.match(panel, /updateProjectTimer/);
  assert.match(panel, /deleteProjectTimer/);
  assert.match(statusBar, /title="Manage project timers"/);
  assert.match(statusBar, /<TimerPanel/);
});
