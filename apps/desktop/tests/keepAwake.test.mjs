import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

import {
  armKeepAwake,
  evaluateKeepAwake,
  KEEP_AWAKE_SETTLE_MS
} from '../src/lib/keepAwake.ts';

const files = {
  app: new URL('../src/App.svelte', import.meta.url),
  control: new URL('../src/lib/KeepAwakeControl.svelte', import.meta.url),
  navigation: new URL('../src/lib/navigation.ts', import.meta.url),
  palette: new URL('../src/lib/QuickJumpPalette.svelte', import.meta.url),
  shortcuts: new URL('../src/lib/KeyboardShortcuts.svelte', import.meta.url)
};

function agent(id, attention, status = 'running') {
  return {
    id,
    kind: 'agent',
    status,
    agent_state: {
      state: attention,
      working: attention === 'working',
      needs_input: attention === 'needs_input',
      waiting: attention === 'waiting',
      idle: attention === 'idle' || attention === 'waiting'
    }
  };
}

test('waiting, needs input, and working are not full idle', () => {
  for (const attention of ['waiting', 'needs_input', 'working']) {
    const processes = [agent(1, attention)];
    const state = armKeepAwake('all', null, processes);
    const evaluation = evaluateKeepAwake(state, processes, 1_000);
    assert.deepEqual(evaluation.waitingAgentIds, [1]);
    assert.equal(evaluation.releaseInSeconds, null);
    assert.equal(evaluation.shouldRelease, false);
    assert.equal(evaluation.state.idleSince, null);
  }
});

test('all watched agents must stay idle for the full settle window', () => {
  const initial = armKeepAwake('all', null, [agent(1, 'working'), agent(2, 'waiting')]);
  const idle = [agent(1, 'idle'), agent(2, 'idle')];
  const settling = evaluateKeepAwake(initial, idle, 5_000);

  assert.equal(settling.releaseInSeconds, 60);
  assert.equal(evaluateKeepAwake(settling.state, idle, 5_000 + KEEP_AWAKE_SETTLE_MS - 1).shouldRelease, false);
  assert.equal(evaluateKeepAwake(settling.state, idle, 5_000 + KEEP_AWAKE_SETTLE_MS).shouldRelease, true);
});

test('working inside the settle window resets the idle timer', () => {
  const initial = armKeepAwake('all', null, [agent(1, 'working')]);
  const settling = evaluateKeepAwake(initial, [agent(1, 'idle')], 1_000);
  const flapped = evaluateKeepAwake(settling.state, [agent(1, 'working')], 30_000);
  const restarted = evaluateKeepAwake(flapped.state, [agent(1, 'idle')], 31_000);

  assert.equal(flapped.state.idleSince, null);
  assert.equal(restarted.state.idleSince, 31_000);
  assert.equal(evaluateKeepAwake(restarted.state, [agent(1, 'idle')], 61_000).shouldRelease, false);
});

test('a closed or exited specific agent counts as satisfied', () => {
  const initial = armKeepAwake('specific', 7, [agent(7, 'working')]);
  const closed = evaluateKeepAwake(initial, [], 1_000);
  assert.equal(closed.releaseInSeconds, 60);
  assert.equal(evaluateKeepAwake(closed.state, [], 61_000).shouldRelease, true);
  assert.equal(evaluateKeepAwake(initial, [agent(7, 'exited', 'exited')], 1_000).releaseInSeconds, 60);
});

test('all-agent mode with no running agents settles before release', () => {
  const initial = armKeepAwake('all', null, []);
  const settling = evaluateKeepAwake(initial, [], 500);
  assert.equal(settling.releaseInSeconds, 60);
  assert.equal(evaluateKeepAwake(settling.state, [], 60_500).shouldRelease, true);
});

test('header, quick jump, and shortcuts expose keep awake', async () => {
  const [app, control, navigation, palette, shortcuts] = await Promise.all(
    Object.values(files).map((file) => readFile(file, 'utf8'))
  );

  assert.match(app, /<KeepAwakeControl/);
  assert.match(control, /CoffeeIcon/);
  assert.match(control, /Until all agents are idle/);
  assert.match(control, /Until a specific agent is idle/);
  assert.match(control, /keep_awake_start/);
  assert.match(control, /keep_awake_stop/);
  assert.match(navigation, /type: 'keep-awake'/);
  assert.match(palette, /label: 'Keep awake…'/);
  assert.match(shortcuts, /Open Keep awake… in quick jump/);
});
