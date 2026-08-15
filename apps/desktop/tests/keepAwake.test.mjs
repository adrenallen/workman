import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

import {
  armKeepAwake,
  evaluateKeepAwake,
  evaluateKeepAwakeAtCurrentTime,
  evaluateKeepAwakeConnection,
  KEEP_AWAKE_SETTLE_MS,
  shouldSubscribeProcessStatuses
} from '../src/lib/keepAwake.ts';

const files = {
  app: new URL('../src/App.svelte', import.meta.url),
  control: new URL('../src/lib/KeepAwakeControl.svelte', import.meta.url),
  navigation: new URL('../src/lib/navigation.ts', import.meta.url),
  palette: new URL('../src/lib/QuickJumpPalette.svelte', import.meta.url),
  shortcuts: new URL('../src/lib/KeyboardShortcuts.svelte', import.meta.url)
};

function agent(id, attention, status = 'running', lastOutputAt = 1) {
  return {
    id,
    kind: 'agent',
    status,
    agent_state: {
      state: attention,
      working: attention === 'working',
      needs_input: attention === 'needs_input',
      waiting: attention === 'waiting',
      idle: attention === 'idle' || attention === 'waiting',
      last_output_at: lastOutputAt
    }
  };
}

test('waiting, needs input, and working are not full idle', () => {
  for (const attention of ['waiting', 'needs_input', 'working']) {
    const processes = [agent(1, attention)];
    const state = armKeepAwake('all', null);
    const evaluation = evaluateKeepAwake(state, processes, 1_000);
    assert.deepEqual(evaluation.waitingAgentIds, [1]);
    assert.equal(evaluation.releaseInSeconds, null);
    assert.equal(evaluation.shouldRelease, false);
    assert.equal(evaluation.state.idleSince, null);
  }
});

test('all agents must stay idle for the full settle window', () => {
  const initial = armKeepAwake('all', null);
  const idle = [agent(1, 'idle'), agent(2, 'idle')];
  const settling = evaluateKeepAwake(initial, idle, 5_000);

  assert.equal(settling.releaseInSeconds, 60);
  assert.equal(evaluateKeepAwake(settling.state, idle, 5_000 + KEEP_AWAKE_SETTLE_MS - 1).shouldRelease, false);
  assert.equal(evaluateKeepAwake(settling.state, idle, 5_000 + KEEP_AWAKE_SETTLE_MS).shouldRelease, true);
});

test('a status push releases against the current clock without waiting for a UI tick', () => {
  const realDateNow = Date.now;
  let currentTime = 1_000;
  Date.now = () => currentTime;
  try {
    const initial = armKeepAwake('all', null);
    const settling = evaluateKeepAwakeAtCurrentTime(initial, [agent(1, 'idle')], 0);
    currentTime += KEEP_AWAKE_SETTLE_MS;
    const statusPush = evaluateKeepAwakeAtCurrentTime(
      settling.state,
      [{ ...agent(1, 'idle'), agent_state: { ...agent(1, 'idle').agent_state } }],
      0
    );
    assert.equal(statusPush.shouldRelease, true);
  } finally {
    Date.now = realDateNow;
  }
});

test('working inside the settle window resets the idle timer', () => {
  const initial = armKeepAwake('all', null);
  const settling = evaluateKeepAwake(initial, [agent(1, 'idle')], 1_000);
  const flapped = evaluateKeepAwake(settling.state, [agent(1, 'working')], 30_000);
  const restarted = evaluateKeepAwake(flapped.state, [agent(1, 'idle')], 31_000);

  assert.equal(flapped.state.idleSince, null);
  assert.equal(restarted.state.idleSince, 31_000);
  assert.equal(evaluateKeepAwake(restarted.state, [agent(1, 'idle')], 61_000).shouldRelease, false);
});

test('a closed or exited specific agent counts as satisfied', () => {
  const initial = armKeepAwake('specific', 7);
  const closed = evaluateKeepAwake(initial, [], 1_000);
  assert.equal(closed.releaseInSeconds, 60);
  assert.equal(evaluateKeepAwake(closed.state, [], 61_000).shouldRelease, true);
  assert.equal(evaluateKeepAwake(initial, [agent(7, 'exited', 'exited')], 1_000).releaseInSeconds, 60);
});

test('all-agent mode with no running agents settles before release', () => {
  const initial = armKeepAwake('all', null);
  const settling = evaluateKeepAwake(initial, [], 500);
  assert.equal(settling.releaseInSeconds, 60);
  assert.equal(evaluateKeepAwake(settling.state, [], 60_500).shouldRelease, true);
});

test('all-agent mode watches agents spawned after arming', () => {
  const initial = armKeepAwake('all', null);
  const settling = evaluateKeepAwake(initial, [agent(1, 'idle')], 1_000);
  const spawned = evaluateKeepAwake(
    settling.state,
    [agent(1, 'idle'), agent(2, 'working')],
    30_000
  );

  assert.deepEqual(initial.watchedAgentIds, []);
  assert.deepEqual(spawned.waitingAgentIds, [2]);
  assert.equal(spawned.state.idleSince, null);
  assert.equal(spawned.shouldRelease, false);
});

test('all-agent mode waits when agents appear after an empty first snapshot', () => {
  const initial = armKeepAwake('all', null);
  const empty = evaluateKeepAwake(initial, [], 1_000);
  const appeared = evaluateKeepAwake(empty.state, [agent(3, 'waiting')], 40_000);

  assert.equal(empty.releaseInSeconds, 60);
  assert.deepEqual(appeared.waitingAgentIds, [3]);
  assert.equal(appeared.releaseInSeconds, null);
  assert.equal(appeared.state.idleSince, null);
});

test('all-agent mode treats an idle agent with no output as not ready to release', () => {
  const evaluation = evaluateKeepAwake(
    armKeepAwake('all', null),
    [agent(4, 'idle', 'starting', null)],
    1_000
  );
  assert.deepEqual(evaluation.waitingAgentIds, [4]);
  assert.equal(evaluation.state.idleSince, null);
});

test('disconnect duration survives connecting and disconnected oscillation', () => {
  let lastConnectedAt = 1_000;
  for (const [status, now, shouldDisarm] of [
    ['disconnected', 10_000, false],
    ['connecting', 30_000, false],
    ['disconnected', 59_000, false],
    ['connecting', 61_000, true]
  ]) {
    const evaluation = evaluateKeepAwakeConnection(lastConnectedAt, status, now);
    lastConnectedAt = evaluation.lastConnectedAt;
    assert.equal(evaluation.shouldDisarm, shouldDisarm);
  }

  const reconnected = evaluateKeepAwakeConnection(lastConnectedAt, 'connected', 70_000);
  assert.equal(reconnected.lastConnectedAt, 70_000);
  assert.equal(
    evaluateKeepAwakeConnection(reconnected.lastConnectedAt, 'disconnected', 80_000)
      .shouldDisarm,
    false
  );
});

test('armed keep awake retains process statuses while the document is hidden', () => {
  assert.equal(shouldSubscribeProcessStatuses(true, false), true);
  assert.equal(shouldSubscribeProcessStatuses(false, true), true);
  assert.equal(shouldSubscribeProcessStatuses(false, false), false);
});

test('header, quick jump, and shortcuts expose keep awake', async () => {
  const [app, control, navigation, palette, shortcuts] = await Promise.all(
    Object.values(files).map((file) => readFile(file, 'utf8'))
  );

  assert.match(app, /<KeepAwakeControl/);
  assert.match(app, /bind:armed=\{keepAwakeArmed\}/);
  assert.match(app, /shouldSubscribeProcessStatuses\(documentVisible, keepAwakeArmed\)/);
  assert.match(control, /CoffeeIcon/);
  assert.match(control, /Until all agents are idle/);
  assert.match(control, /Until a specific agent is idle/);
  assert.match(control, /keep_awake_start/);
  assert.match(control, /keep_awake_stop/);
  assert.match(control, /evaluateKeepAwakeAtCurrentTime\(machine, processes, clockTick\)/);
  assert.match(control, /evaluateKeepAwakeConnection/);
  assert.match(control, /!machine\.armed && status\.active/);
  assert.match(control, /machineGeneration === generation/);
  assert.match(control, /if \(busy \|\| autoReleasePending \|\| !machine\.armed\) return/);
  assert.match(control, /Keep awake released — daemon disconnected/);
  assert.match(control, /var\(--font-mono\)/);
  assert.match(navigation, /type: 'keep-awake'/);
  assert.match(palette, /if \(keepAwakeSupported\)/);
  assert.match(palette, /label: 'Keep awake…'/);
  assert.match(shortcuts, /keepAwakeSupported \? ', including Keep awake…' : ''/);
  assert.doesNotMatch(shortcuts, /keys: \['⌘', 'K'\], label: 'Open Keep awake/);
});
