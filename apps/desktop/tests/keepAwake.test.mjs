import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

import {
  armKeepAwake,
  evaluateKeepAwake,
  evaluateKeepAwakeAtCurrentTime,
  evaluateKeepAwakeConnection,
  initialKeepAwakeConnectionState,
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

const connectedAndVisible = { connected: true, visible: true };

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

function observeIdleFor(state, processes, start, durationMs, stepMs = 1_000) {
  let evaluation = evaluateKeepAwake(state, processes, start, connectedAndVisible);
  for (let elapsed = stepMs; elapsed <= durationMs; elapsed += stepMs) {
    evaluation = evaluateKeepAwake(
      evaluation.state,
      processes,
      start + elapsed,
      connectedAndVisible
    );
  }
  return evaluation;
}

function observeDisconnectFor(durationMs, unreachableMs) {
  let evaluation = evaluateKeepAwakeConnection(
    initialKeepAwakeConnectionState(),
    'disconnected',
    1_000,
    true,
    unreachableMs
  );
  for (let elapsed = 1_000; elapsed <= durationMs; elapsed += 1_000) {
    evaluation = evaluateKeepAwakeConnection(
      evaluation.state,
      (elapsed / 1_000) % 2 === 0 ? 'connecting' : 'disconnected',
      1_000 + elapsed,
      true,
      unreachableMs
    );
  }
  return evaluation;
}

test('waiting, needs input, and working are not full idle', () => {
  for (const attention of ['waiting', 'needs_input', 'working']) {
    const state = armKeepAwake('all', null);
    const evaluation = evaluateKeepAwake(
      state,
      [agent(1, attention)],
      1_000,
      connectedAndVisible
    );
    assert.deepEqual(evaluation.waitingAgentIds, [1]);
    assert.equal(evaluation.releaseInSeconds, null);
    assert.equal(evaluation.shouldRelease, false);
    assert.equal(evaluation.state.lastIdleObservationAt, null);
  }
});

test('all agents must be actively observed idle for the full settle window', () => {
  const idle = [agent(1, 'idle'), agent(2, 'idle')];
  const almostSettled = observeIdleFor(
    armKeepAwake('all', null),
    idle,
    5_000,
    KEEP_AWAKE_SETTLE_MS - 1_000
  );
  assert.equal(almostSettled.shouldRelease, false);
  assert.equal(almostSettled.releaseInSeconds, 1);

  const settled = evaluateKeepAwake(
    almostSettled.state,
    idle,
    5_000 + KEEP_AWAKE_SETTLE_MS,
    connectedAndVisible
  );
  assert.equal(settled.shouldRelease, true);
});

test('the UI-tick helper uses its supplied monotonic observation timestamp', () => {
  const idle = [agent(1, 'idle')];
  const initial = armKeepAwake('all', null);
  const first = evaluateKeepAwakeAtCurrentTime(
    initial,
    idle,
    4_000,
    connectedAndVisible,
    1_000
  );
  const released = evaluateKeepAwakeAtCurrentTime(
    first.state,
    idle,
    5_000,
    connectedAndVisible,
    1_000
  );
  assert.equal(released.shouldRelease, true);
});

test('working inside the settle window resets active idle observation', () => {
  const idle = [agent(1, 'idle')];
  const settling = observeIdleFor(armKeepAwake('all', null), idle, 1_000, 30_000);
  const flapped = evaluateKeepAwake(
    settling.state,
    [agent(1, 'working')],
    32_000,
    connectedAndVisible
  );
  const restarted = evaluateKeepAwake(flapped.state, idle, 33_000, connectedAndVisible);

  assert.equal(flapped.state.idleObservedMs, 0);
  assert.equal(flapped.state.lastIdleObservationAt, null);
  assert.equal(restarted.state.idleObservedMs, 0);
  assert.equal(restarted.releaseInSeconds, 60);
});

test('a closed or exited specific agent settles before release', () => {
  const initial = armKeepAwake('specific', 7);
  assert.equal(evaluateKeepAwake(initial, [], 1_000, connectedAndVisible).releaseInSeconds, 60);
  assert.equal(observeIdleFor(initial, [], 1_000, KEEP_AWAKE_SETTLE_MS).shouldRelease, true);
  assert.equal(
    evaluateKeepAwake(initial, [agent(7, 'exited', 'exited')], 1_000, connectedAndVisible)
      .releaseInSeconds,
    60
  );
});

test('all-agent mode with no running agents settles before release', () => {
  const evaluation = observeIdleFor(
    armKeepAwake('all', null),
    [],
    500,
    KEEP_AWAKE_SETTLE_MS
  );
  assert.equal(evaluation.shouldRelease, true);
});

test('all-agent mode watches agents spawned after arming', () => {
  const initial = armKeepAwake('all', null);
  const settling = observeIdleFor(initial, [agent(1, 'idle')], 1_000, 30_000);
  const spawned = evaluateKeepAwake(
    settling.state,
    [agent(1, 'idle'), agent(2, 'working')],
    32_000,
    connectedAndVisible
  );

  assert.deepEqual(initial.watchedAgentIds, []);
  assert.deepEqual(spawned.waitingAgentIds, [2]);
  assert.equal(spawned.state.lastIdleObservationAt, null);
  assert.equal(spawned.shouldRelease, false);
});

test('an agent appearing after an empty snapshot resets idle observation', () => {
  const initial = armKeepAwake('all', null);
  const empty = observeIdleFor(initial, [], 1_000, 30_000);
  const appeared = evaluateKeepAwake(
    empty.state,
    [agent(3, 'waiting')],
    32_000,
    connectedAndVisible
  );

  assert.deepEqual(appeared.waitingAgentIds, [3]);
  assert.equal(appeared.releaseInSeconds, null);
  assert.equal(appeared.state.idleObservedMs, 0);
});

test('an idle agent with no output is not ready to release', () => {
  const evaluation = evaluateKeepAwake(
    armKeepAwake('all', null),
    [agent(4, 'idle', 'starting', null)],
    1_000,
    connectedAndVisible
  );
  assert.deepEqual(evaluation.waitingAgentIds, [4]);
  assert.equal(evaluation.state.lastIdleObservationAt, null);
});

test('transient daemon disconnects never request disarm and reconnect resets observation', () => {
  const disconnected = observeDisconnectFor(120_000, 10 * 60_000);
  assert.equal(disconnected.daemonUnreachable, false);
  assert.equal('shouldDisarm' in disconnected, false);

  const reconnected = evaluateKeepAwakeConnection(
    disconnected.state,
    'connected',
    122_000,
    true
  );
  assert.deepEqual(reconnected.state, initialKeepAwakeConnectionState());
  assert.equal(reconnected.daemonUnreachable, false);
});

test('a long actively observed disconnect warns but does not disarm', () => {
  const disconnected = observeDisconnectFor(10_000, 10_000);
  assert.equal(disconnected.daemonUnreachable, true);
  assert.equal(disconnected.recheckInMs, null);
  assert.equal('shouldDisarm' in disconnected, false);
});

test('a sleep-sized clock gap is not counted as idle or disconnected observation', () => {
  const idle = [agent(1, 'idle')];
  const beforeSleep = observeIdleFor(armKeepAwake('all', null), idle, 1_000, 4_000);
  const afterSleep = evaluateKeepAwake(
    beforeSleep.state,
    idle,
    60 * 60_000,
    connectedAndVisible
  );
  assert.equal(afterSleep.shouldRelease, false);
  assert.equal(afterSleep.state.idleObservedMs, 0);
  assert.equal(afterSleep.releaseInSeconds, 60);

  const disconnectBeforeSleep = observeDisconnectFor(4_000, 10_000);
  const disconnectAfterSleep = evaluateKeepAwakeConnection(
    disconnectBeforeSleep.state,
    'disconnected',
    60 * 60_000,
    true,
    10_000
  );
  assert.equal(disconnectAfterSleep.daemonUnreachable, false);
  assert.equal(disconnectAfterSleep.state.disconnectedObservedMs, 0);
});

test('hidden visibility freezes evaluation and visible return starts a fresh observation', () => {
  const idle = [agent(1, 'idle')];
  const settling = observeIdleFor(armKeepAwake('all', null), idle, 1_000, 4_000);
  const hidden = evaluateKeepAwake(
    settling.state,
    idle,
    6_000,
    { connected: true, visible: false }
  );
  assert.equal(hidden.state.idleObservedMs, 0);
  assert.equal(hidden.state.lastIdleObservationAt, null);

  const visibleAgain = evaluateKeepAwake(
    hidden.state,
    idle,
    60_000,
    connectedAndVisible
  );
  assert.equal(visibleAgain.shouldRelease, false);
  assert.equal(visibleAgain.releaseInSeconds, 60);
});

test('armed keep awake retains process statuses while the document is hidden', () => {
  assert.equal(shouldSubscribeProcessStatuses(true, false), true);
  assert.equal(shouldSubscribeProcessStatuses(false, true), true);
  assert.equal(shouldSubscribeProcessStatuses(false, false), false);
});

test('control exposes resilient, truthful keep-awake status and copy', async () => {
  const [app, control, navigation, palette, shortcuts] = await Promise.all(
    Object.values(files).map((file) => readFile(file, 'utf8'))
  );

  assert.match(app, /<KeepAwakeControl/);
  assert.match(app, /visible=\{documentVisible\}/);
  assert.match(app, /bind:armed=\{keepAwakeArmed\}/);
  assert.match(app, /shouldSubscribeProcessStatuses\(documentVisible, keepAwakeArmed\)/);
  assert.match(control, /CoffeeIcon/);
  assert.match(control, /Until all agents are idle/);
  assert.match(control, /Until a specific agent is idle/);
  assert.match(control, /keep_awake_start/);
  assert.match(control, /keep_awake_stop/);
  assert.match(control, /keep_awake_status/);
  assert.match(control, /!status\.armed \|\| !status\.active/);
  assert.match(control, /Daemon reconnecting — still keeping Mac awake/);
  assert.match(control, /Daemon unreachable — still keeping Mac awake/);
  assert.match(control, /Assertion PID \$\{status\.assertion_pid\} held/);
  assert.match(control, /Released because all watched agents became idle/);
  assert.match(control, /Released by you/);
  assert.match(control, /Closing the lid still sleeps this Mac/);
  assert.doesNotMatch(control, /released — daemon disconnected/i);
  assert.match(control, /var\(--font-mono\)/);
  assert.match(navigation, /type: 'keep-awake'/);
  assert.match(palette, /if \(keepAwakeSupported\)/);
  assert.match(palette, /label: 'Keep awake…'/);
  assert.match(shortcuts, /keepAwakeSupported \? ', including Keep awake…' : ''/);
  assert.doesNotMatch(shortcuts, /keys: \['⌘', 'K'\], label: 'Open Keep awake/);
});
