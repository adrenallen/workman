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
  shortcuts: new URL('../src/lib/KeyboardShortcuts.svelte', import.meta.url),
  native: new URL('../src-tauri/src/lib.rs', import.meta.url)
};

const connected = { connected: true };

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
  let evaluation = evaluateKeepAwake(state, processes, start, connected);
  for (let elapsed = stepMs; elapsed <= durationMs; elapsed += stepMs) {
    evaluation = evaluateKeepAwake(
      evaluation.state,
      processes,
      start + elapsed,
      connected
    );
  }
  return evaluation;
}

function observeDisconnectFor(durationMs, unreachableMs) {
  let evaluation = evaluateKeepAwakeConnection(
    initialKeepAwakeConnectionState(),
    'disconnected',
    1_000,
    unreachableMs
  );
  for (let elapsed = 1_000; elapsed <= durationMs; elapsed += 1_000) {
    evaluation = evaluateKeepAwakeConnection(
      evaluation.state,
      (elapsed / 1_000) % 2 === 0 ? 'connecting' : 'disconnected',
      1_000 + elapsed,
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
      connected
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
    connected
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
    connected,
    1_000
  );
  const released = evaluateKeepAwakeAtCurrentTime(
    first.state,
    idle,
    5_000,
    connected,
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
    connected
  );
  const restarted = evaluateKeepAwake(flapped.state, idle, 33_000, connected);

  assert.equal(flapped.state.idleObservedMs, 0);
  assert.equal(flapped.state.lastIdleObservationAt, null);
  assert.equal(restarted.state.idleObservedMs, 0);
  assert.equal(restarted.releaseInSeconds, 60);
});

test('a closed or exited specific agent settles before release', () => {
  const initial = armKeepAwake('specific', 7);
  assert.equal(evaluateKeepAwake(initial, [], 1_000, connected).releaseInSeconds, 60);
  assert.equal(observeIdleFor(initial, [], 1_000, KEEP_AWAKE_SETTLE_MS).shouldRelease, true);
  assert.equal(
    evaluateKeepAwake(initial, [agent(7, 'exited', 'exited')], 1_000, connected)
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
    connected
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
    connected
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
    connected
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
    122_000
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

test('a sleep-sized clock gap is clamped instead of counted as suspended time', () => {
  const idle = [agent(1, 'idle')];
  const beforeSleep = observeIdleFor(armKeepAwake('all', null), idle, 1_000, 4_000);
  const afterSleep = evaluateKeepAwake(
    beforeSleep.state,
    idle,
    60 * 60_000,
    connected
  );
  assert.equal(afterSleep.shouldRelease, false);
  assert.equal(afterSleep.state.idleObservedMs, 9_000);
  assert.equal(afterSleep.releaseInSeconds, 51);

  const disconnectBeforeSleep = observeDisconnectFor(4_000, 10_000);
  const disconnectAfterSleep = evaluateKeepAwakeConnection(
    disconnectBeforeSleep.state,
    'disconnected',
    60 * 60_000,
    10_000
  );
  assert.equal(disconnectAfterSleep.daemonUnreachable, false);
  assert.equal(disconnectAfterSleep.state.disconnectedObservedMs, 9_000);
});

test('hidden but delivered idle observations still release keep awake', () => {
  const idle = [agent(1, 'idle')];
  const hidden = observeIdleFor(
    armKeepAwake('all', null),
    idle,
    1_000,
    KEEP_AWAKE_SETTLE_MS
  );
  assert.equal(hidden.shouldRelease, true);
});

test('periodic slow ticks preserve progress and eventually release', () => {
  const idle = [agent(1, 'idle')];
  let evaluation = evaluateKeepAwake(armKeepAwake('all', null), idle, 1_000, connected);
  let now = 1_000;
  for (let tick = 1; tick <= 20 && !evaluation.shouldRelease; tick += 1) {
    now += tick % 10 === 0 ? 6_000 : 1_000;
    evaluation = evaluateKeepAwake(evaluation.state, idle, now, connected);
  }
  assert.equal(evaluation.shouldRelease, false);
  assert.equal(evaluation.state.idleObservedMs, 28_000);

  for (let tick = 21; tick <= 50 && !evaluation.shouldRelease; tick += 1) {
    now += tick % 10 === 0 ? 6_000 : 1_000;
    evaluation = evaluateKeepAwake(evaluation.state, idle, now, connected);
  }
  assert.equal(evaluation.shouldRelease, true);
});

test('disconnect pauses idle progress and reconnect resumes it', () => {
  const idle = [agent(1, 'idle')];
  const settling = observeIdleFor(armKeepAwake('all', null), idle, 1_000, 30_000);
  const disconnected = evaluateKeepAwake(
    settling.state,
    idle,
    40_000,
    { connected: false }
  );
  assert.equal(disconnected.state.idleObservedMs, 30_000);
  assert.equal(disconnected.state.lastIdleObservationAt, null);
  assert.equal(disconnected.releaseInSeconds, null);

  const reconnected = observeIdleFor(disconnected.state, idle, 50_000, 30_000);
  assert.equal(reconnected.shouldRelease, true);
});

test('disconnect observations do not depend on document visibility', () => {
  const disconnected = observeDisconnectFor(10_000, 10_000);
  assert.equal(disconnected.daemonUnreachable, true);
});

test('armed keep awake retains process statuses while the document is hidden', () => {
  assert.equal(shouldSubscribeProcessStatuses(true, false), true);
  assert.equal(shouldSubscribeProcessStatuses(false, true), true);
  assert.equal(shouldSubscribeProcessStatuses(false, false), false);
});

test('control exposes resilient, truthful keep-awake status and copy', async () => {
  const [app, control, navigation, palette, shortcuts, native] = await Promise.all(
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
  assert.doesNotMatch(control, /await invoke<NativeKeepAwakeStatus>\('keep_awake_start'\)[\s\S]*keep_awake_start/);
  assert.match(control, /Daemon reconnecting — still keeping Mac awake/);
  assert.match(control, /Daemon unreachable — still keeping Mac awake/);
  assert.match(control, /Keep awake assertion restored/);
  assert.match(control, /Keep awake will not auto-release until the daemon reconnects/);
  assert.match(control, /Assertion PID \$\{status\.assertion_pid\} held/);
  assert.match(control, /Released because all watched agents became idle/);
  assert.match(control, /Released by you/);
  assert.match(control, /Prevents idle sleep on AC or battery/);
  assert.match(control, /Workman quits; closing the lid still sleeps this Mac/);
  assert.doesNotMatch(control, /released — daemon disconnected/i);
  assert.match(control, /var\(--font-mono\)/);
  assert.match(navigation, /type: 'keep-awake'/);
  assert.match(palette, /if \(keepAwakeSupported\)/);
  assert.match(palette, /label: 'Keep awake…'/);
  assert.match(shortcuts, /keepAwakeSupported \? ', including Keep awake…' : ''/);
  assert.doesNotMatch(shortcuts, /keys: \['⌘', 'K'\], label: 'Open Keep awake/);
  assert.match(native, /run_keep_awake_watchdog/);
  assert.match(native, /if !inner\.armed \|\| inner\.child\.is_some\(\)/);
  assert.match(native, /KEEP_AWAKE_MAX_RETRY_DELAY/);
  assert.match(native, /respawn_count/);
});
