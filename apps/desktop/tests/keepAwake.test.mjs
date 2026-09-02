import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

import {
  activeKeepAwakeAgents,
  armKeepAwake,
  evaluateAutoKeepAwake,
  evaluateKeepAwake,
  evaluateKeepAwakeAtCurrentTime,
  evaluateKeepAwakeConnection,
  initialAutoKeepAwakeState,
  initialKeepAwakeConnectionState,
  KEEP_AWAKE_SETTLE_MS,
  loadAutoKeepAwakePreference,
  loadPersistedKeepAwakeState,
  nativeAutoKeepAwakeNeedsReconciliation,
  reconcileKeepAwakeIntent,
  runningAgents,
  saveAutoKeepAwakePreference,
  savePersistedKeepAwakeState,
  shouldHoldAutoKeepAwake,
  suppressAutoKeepAwake,
  shouldSubscribeProcessStatuses
} from '../src/lib/keepAwake.ts';

const files = {
  app: new URL('../src/App.svelte', import.meta.url),
  control: new URL('../src/lib/KeepAwakeControl.svelte', import.meta.url),
  navigation: new URL('../src/lib/navigation.ts', import.meta.url),
  palette: new URL('../src/lib/QuickJumpPalette.svelte', import.meta.url),
  shortcuts: new URL('../src/lib/KeyboardShortcuts.svelte', import.meta.url),
  native: new URL('../src-tauri/src/lib.rs', import.meta.url),
  config: new URL('../src-tauri/tauri.conf.json', import.meta.url)
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
      last_output_at: lastOutputAt,
      waiting_on: attention === 'waiting'
        ? [{
            timer_id: 1,
            kind: 'delay',
            due_at: 30_000,
            max_wait_ms: 30_000,
            remaining_ms: 20_000,
            paused: false,
            watch_processes: []
          }]
        : []
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

test('auto activity follows agent attention rather than a merely live process', () => {
  for (const attention of ['waiting', 'needs_input', 'working']) {
    assert.deepEqual(activeKeepAwakeAgents([agent(1, attention)]).map(({ id }) => id), [1]);
  }
  const thinking = agent(2, 'idle');
  thinking.agent_state.thinking = true;
  assert.deepEqual(activeKeepAwakeAgents([thinking]).map(({ id }) => id), [2]);
  const planning = agent(5, 'idle');
  planning.agent_state.planning = true;
  assert.deepEqual(activeKeepAwakeAgents([planning]).map(({ id }) => id), [5]);
  assert.deepEqual(activeKeepAwakeAgents([agent(6, 'idle', 'starting', null)]).map(({ id }) => id), [6]);
  assert.deepEqual(activeKeepAwakeAgents([agent(3, 'idle')]), []);
  assert.deepEqual(activeKeepAwakeAgents([agent(4, 'working', 'exited')]), []);
  assert.deepEqual(runningAgents([agent(3, 'idle')]).map(({ id }) => id), [3]);
});

test('auto mode requests an all-agent arm from the first active snapshot', () => {
  const evaluation = evaluateAutoKeepAwake(
    initialAutoKeepAwakeState(),
    [agent(1, 'working'), agent(2, 'idle')],
    true,
    1_000
  );
  assert.equal(evaluation.activityEdge, false);
  assert.equal(evaluation.shouldArm, true);
  assert.deepEqual(evaluation.activeAgentIds, [1]);
  assert.equal(armKeepAwake('all', null, 'auto').armSource, 'auto');
});

test('auto should-hold is computed from current state, preference, and suppression', () => {
  const active = [agent(1, 'working')];
  assert.equal(shouldHoldAutoKeepAwake(active, true), true);
  assert.equal(shouldHoldAutoKeepAwake(active, false), false);
  assert.equal(shouldHoldAutoKeepAwake(active, true, true), false);
  assert.equal(shouldHoldAutoKeepAwake([agent(1, 'idle')], true), false);
});

test('manual disarm suppresses auto re-arm until a fresh agent activity edge', () => {
  const working = [agent(1, 'working')];
  const observed = evaluateAutoKeepAwake(initialAutoKeepAwakeState(), working, true, 1_000);
  const suppressed = suppressAutoKeepAwake(observed.state, working, 1_000);
  const sameActivity = evaluateAutoKeepAwake(suppressed, working, true, 2_000);
  assert.equal(sameActivity.shouldArm, false);
  assert.equal(sameActivity.state.suppressedUntilActivityEdge, true);

  const freshAgent = evaluateAutoKeepAwake(
    sameActivity.state,
    [agent(1, 'working'), agent(2, 'needs_input')],
    true,
    3_000
  );
  assert.equal(freshAgent.activityEdge, true);
  assert.equal(freshAgent.shouldArm, true);
  assert.equal(freshAgent.state.suppressedUntilActivityEdge, false);
});

test('the same agent becoming active again is a fresh edge after idle', () => {
  const observed = evaluateAutoKeepAwake(
    initialAutoKeepAwakeState(),
    [agent(1, 'working')],
    true,
    1_000
  );
  const suppressed = suppressAutoKeepAwake(observed.state, [agent(1, 'working')], 1_000);
  const idle = evaluateAutoKeepAwake(suppressed, [agent(1, 'idle')], true, 2_000);
  assert.equal(idle.shouldArm, false);
  assert.equal(idle.state.suppressedUntilActivityEdge, true);
  const resumed = evaluateAutoKeepAwake(idle.state, [agent(1, 'waiting')], true, 3_000);
  assert.equal(resumed.activityEdge, true);
  assert.equal(resumed.shouldArm, true);
});

test('auto preference persists as a per-machine boolean', () => {
  const values = new Map();
  const storage = {
    getItem: (key) => values.get(key) ?? null,
    setItem: (key, value) => values.set(key, value)
  };
  assert.equal(loadAutoKeepAwakePreference(storage), false);
  saveAutoKeepAwakePreference(true, storage);
  assert.equal(loadAutoKeepAwakePreference(storage), true);
  saveAutoKeepAwakePreference(false, storage);
  assert.equal(loadAutoKeepAwakePreference(storage), false);
});

test('auto config reconciliation retries whenever native intent differs', () => {
  assert.equal(nativeAutoKeepAwakeNeedsReconciliation(true, false), true);
  assert.equal(nativeAutoKeepAwakeNeedsReconciliation(false, true), true);
  assert.equal(nativeAutoKeepAwakeNeedsReconciliation(true, true), false);
  assert.equal(nativeAutoKeepAwakeNeedsReconciliation(false, false), false);
});

test('manual suppression and hold ownership survive reload without a fake activity edge', () => {
  const values = new Map();
  const storage = {
    getItem: (key) => values.get(key) ?? null,
    setItem: (key, value) => values.set(key, value)
  };
  savePersistedKeepAwakeState({
    autoState: { activeAgentIds: [7], suppressedUntilActivityEdge: true },
    preferredMode: 'specific',
    preferredSpecificAgentId: 7,
    activeHold: { mode: 'specific', armSource: 'manual', watchedAgentIds: [7] }
  }, storage);
  const persisted = loadPersistedKeepAwakeState(storage);
  assert.deepEqual(persisted.autoState, {
    activeAgentIds: [7],
    suppressedUntilActivityEdge: true
  });
  assert.deepEqual(persisted.activeHold, {
    mode: 'specific',
    armSource: 'manual',
    watchedAgentIds: [7]
  });
  const restored = {
    ...initialAutoKeepAwakeState(),
    ...persisted.autoState
  };
  const firstSnapshot = evaluateAutoKeepAwake(restored, [agent(7, 'working')], true, 1_000);
  assert.equal(firstSnapshot.activityEdge, false);
  assert.equal(firstSnapshot.shouldArm, false);
  assert.equal(firstSnapshot.state.suppressedUntilActivityEdge, true);
});

test('observation gaps and reconnects rebase activity without clearing suppression', () => {
  const active = [agent(1, 'working')];
  const observed = evaluateAutoKeepAwake(initialAutoKeepAwakeState(), active, true, 1_000);
  const suppressed = suppressAutoKeepAwake(observed.state, active, 1_000);
  const empty = evaluateAutoKeepAwake(suppressed, [], true, 2_000);
  const afterGap = evaluateAutoKeepAwake(empty.state, [agent(2, 'working')], true, 10_000);
  assert.equal(afterGap.activityEdge, false);
  assert.equal(afterGap.shouldArm, false);
  assert.equal(afterGap.state.suppressedUntilActivityEdge, true);

  const disconnected = evaluateAutoKeepAwake(afterGap.state, [], true, 11_000, { connected: false });
  const reconnected = evaluateAutoKeepAwake(
    disconnected.state,
    [agent(3, 'needs_input')],
    true,
    12_000
  );
  assert.equal(reconnected.activityEdge, false);
  assert.equal(reconnected.shouldArm, false);
});

test('waiting counts only while a finite timer is live and unpaused', () => {
  const live = agent(1, 'waiting');
  const paused = agent(2, 'waiting');
  paused.agent_state.waiting_on[0].paused = true;
  const expired = agent(3, 'waiting');
  expired.agent_state.waiting_on[0].remaining_ms = 0;
  const indefinite = agent(4, 'waiting');
  indefinite.agent_state.waiting_on = [];

  assert.deepEqual(activeKeepAwakeAgents([live, paused, expired, indefinite]).map(({ id }) => id), [1]);
  for (const parked of [paused, expired, indefinite]) {
    const settled = observeIdleFor(
      armKeepAwake('all', null, 'auto'),
      [parked],
      1_000,
      KEEP_AWAKE_SETTLE_MS
    );
    assert.equal(settled.shouldRelease, true);
  }
});

test('native disarm demotes a stale armed UI state truthfully', () => {
  const armed = armKeepAwake('all', null, 'auto');
  const lost = reconcileKeepAwakeIntent(armed, false);
  assert.equal(lost.holdLost, true);
  assert.equal(lost.state.armed, false);
  assert.equal(lost.state.armSource, null);
  assert.equal(reconcileKeepAwakeIntent(armed, true).state, armed);
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
  assert.equal(shouldSubscribeProcessStatuses(false, false, true), true);
  assert.equal(shouldSubscribeProcessStatuses(false, false), false);
});

test('control exposes resilient, truthful keep-awake status and copy', async () => {
  const [app, control, navigation, palette, shortcuts, native, config] = await Promise.all(
    Object.values(files).map((file) => readFile(file, 'utf8'))
  );

  assert.match(app, /<KeepAwakeControl/);
  assert.match(app, /visible=\{documentVisible\}/);
  assert.match(app, /bind:armed=\{keepAwakeArmed\}/);
  assert.match(app, /shouldSubscribeProcessStatuses\([\s\S]*autoKeepAwakeEnabled/);
  assert.match(app, /bind:autoEnabled=\{autoKeepAwakeEnabled\}/);
  assert.match(control, /CoffeeIcon/);
  assert.match(control, /Until all agents are idle/);
  assert.match(control, /Until a specific agent is idle/);
  assert.match(control, /Auto keep awake while agents are running/);
  assert.match(control, /Manual disarm pauses auto mode until fresh agent activity/);
  assert.match(control, /Keeping Mac awake — auto/);
  assert.match(control, /loadAutoKeepAwakePreference/);
  assert.match(control, /saveAutoKeepAwakePreference/);
  assert.match(control, /keep_awake_start/);
  assert.match(control, /keep_awake_stop/);
  assert.match(control, /keep_awake_status/);
  assert.match(control, /keep_awake_auto_configure/);
  assert.equal(control.match(/invoke<NativeKeepAwakeStatus>\('keep_awake_start'\)/g)?.length, 1);
  assert.match(control, /Daemon reconnecting — still keeping Mac awake/);
  assert.match(control, /Daemon unreachable — still keeping Mac awake/);
  assert.match(control, /Keep awake assertion restored/);
  assert.match(control, /Auto keep awake will release if agent state stays stale/);
  assert.match(control, /Auto keep awake released because no fresh agent state arrived/);
  assert.match(control, /Assertion PID \$\{status\.assertion_pid\} held/);
  assert.match(control, /machine\.armed && nativeStatus\.armed && nativeStatus\.active/);
  assert.match(control, /data-armed=\{verifiedArmed\}/);
  assert.match(control, /Released because all watched agents became idle/);
  assert.match(control, /Released by you/);
  assert.match(control, /Prevents idle sleep on AC or battery/);
  assert.match(control, /Workman quits; closing the lid still sleeps this Mac/);
  assert.doesNotMatch(control, /released — daemon disconnected/i);
  assert.match(control, /var\(--font-mono\)/);
  assert.match(navigation, /type: 'keep-awake'/);
  assert.match(palette, /if \(keepAwakeSupported\)/);
  assert.match(palette, /label: 'Keep awake…'/);
  assert.match(shortcuts, /definition\.id === 'quick-jump' && keepAwakeSupported/);
  assert.match(shortcuts, /including Keep awake…/);
  assert.doesNotMatch(shortcuts, /keys: \['⌘', 'K'\], label: 'Open Keep awake/);
  assert.match(native, /run_keep_awake_watchdog/);
  assert.match(native, /observe_auto_keep_awake_snapshot/);
  assert.match(native, /evaluate_auto_keep_awake_tick/);
  assert.match(native, /auto_keep_awake_active_agent_ids_from_message/);
  assert.match(native, /if !inner\.armed/);
  assert.match(native, /KEEP_AWAKE_MAX_RETRY_DELAY/);
  assert.match(native, /KEEP_AWAKE_MAX_SNAPSHOT_AGE/);
  assert.match(native, /PersistedKeepAwakePreference/);
  assert.match(native, /status_subscription_due/);
  assert.match(native, /emit_status_if_changed/);
  assert.match(native, /respawn_count/);
  assert.doesNotMatch(native, /RunEvent::Resumed/);
  assert.match(control, /nativeAutoKeepAwakeNeedsReconciliation\(autoEnabled, status\.auto_enabled\)[\s\S]*scheduleNativeAutoConfiguration/);
  assert.match(control, /invokeWithTimeout<NativeKeepAwakeStatus>/);
  assert.match(control, /nativeAutoConfigRetryDelay/);
  assert.match(control, /seedSuppressedUntilActivityEdge/);
  assert.match(control, /suppressAuto: manualOverride/);
  assert.match(control, /suppressAuto: false/);
  assert.match(config, /"acceptFirstMouse": true/);
});
