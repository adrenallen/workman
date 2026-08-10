import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

import { stoppedOutputSnapshotKey } from '../src/lib/stoppedOutput.ts';

const terminalViewUrl = new URL('../src/lib/TerminalView.svelte', import.meta.url);

function process(overrides = {}) {
  return {
    id: 7,
    status: 'stopped',
    exited_at: 1_000,
    ...overrides
  };
}

test('repeated stopped-process broadcasts resolve to one immutable output snapshot', () => {
  const first = process();
  const refreshed = process({
    agent_state: { state: 'exited', idle_seconds: 30 },
    claimed_todos: [{ id: 67 }]
  });

  assert.equal(stoppedOutputSnapshotKey(first, true), '7:1000');
  assert.equal(stoppedOutputSnapshotKey(refreshed, true), '7:1000');
  assert.equal(stoppedOutputSnapshotKey(process({ status: 'running' }), true), null);
  assert.equal(stoppedOutputSnapshotKey(first, false), null);
  assert.equal(stoppedOutputSnapshotKey(process({ exited_at: 2_000 }), true), '7:2000');
});

test('stopped output is gated before state is cleared or fetched', async () => {
  const source = await readFile(terminalViewUrl, 'utf8');
  const effectStart = source.indexOf('const snapshotKey = stoppedOutputSnapshotKey(process, connected);');
  const effectEnd = source.indexOf('function handleTerminalFrame', effectStart);
  const effect = source.slice(effectStart, effectEnd);

  assert.ok(effectStart >= 0 && effectEnd > effectStart);
  const guard = effect.indexOf('if (snapshotKey === retainedOutputSnapshotKey) return;');
  const clear = effect.indexOf("retainedOutput = '';");
  const fetch = effect.indexOf('client.renderedProcessOutput(processId)');
  assert.ok(guard >= 0 && clear > guard && fetch > clear);
});
