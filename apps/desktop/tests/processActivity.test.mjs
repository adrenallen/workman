import assert from 'node:assert/strict';
import test from 'node:test';

import {
  processActivity,
  processActivityTone,
  projectActivityRollup,
  projectKindActivity
} from '../src/lib/processActivity.ts';

function process(id, kind, status = 'running', attention = 'idle', overrides = {}) {
  const fixture = {
    id,
    kind,
    name: `${kind}-${id}`,
    status,
    exit_code: null,
    exit_signal: null,
    agent_state: {
      state: attention,
      working: attention === 'working',
      needs_input: attention === 'needs_input',
      idle: attention === 'idle' || attention === 'waiting',
      exited: attention === 'exited',
      last_output_at: null,
      last_content_change_at: null
    }
  };
  return {
    ...fixture,
    ...overrides,
    agent_state: { ...fixture.agent_state, ...overrides.agent_state }
  };
}

test('terminal is neutral at its shell and green only with a foreground job', () => {
  const terminal = process(1, 'terminal');

  assert.equal(processActivity(terminal, { foreground_active: false }).state, 'idle');
  assert.equal(processActivityTone('idle'), 'neutral');
  assert.equal(processActivity(terminal, { foreground_active: true }).state, 'working');
  assert.equal(processActivityTone('working'), 'success');
});

test('agent colors distinguish working, waiting, idle, and needs-input states', () => {
  assert.equal(processActivity(process(1, 'agent', 'running', 'working')).state, 'working');
  assert.equal(processActivity(process(2, 'agent', 'running', 'waiting')).state, 'waiting');
  assert.equal(processActivityTone('waiting'), 'waiting');
  assert.equal(processActivity(process(3, 'agent', 'running', 'idle')).state, 'idle');
  assert.equal(processActivity(process(4, 'agent', 'running', 'needs_input')).state, 'needs_input');
  assert.equal(processActivityTone('needs_input'), 'needs-input');
});

test('per-process roster tones cover every activity state without borrowing the kind rollup', () => {
  assert.deepEqual(
    ['working', 'needs_input', 'waiting', 'idle', 'stopped', 'crashed'].map(processActivityTone),
    ['success', 'needs-input', 'waiting', 'neutral', 'neutral', 'danger']
  );
  assert.equal(processActivity(process(5, 'agent', 'stopped', 'exited')).shortLabel, 'Stopped');
  assert.equal(processActivity(process(6, 'command', 'crashed')).shortLabel, 'Crashed');
});

test('project rollup stays green with one working agent and three idle terminals', () => {
  const processes = [
    process(1, 'agent', 'running', 'working'),
    process(2, 'terminal'),
    process(3, 'terminal'),
    process(4, 'terminal')
  ];
  const rollup = projectActivityRollup(processes, {
    2: { foreground_active: false },
    3: { foreground_active: false },
    4: { foreground_active: false }
  });

  assert.equal(rollup.state, 'working');
  assert.equal(rollup.active, 1);
  assert.equal(rollup.idle, 3);
});

test('project rollup honors established attention precedence', () => {
  const processes = [
    process(1, 'agent', 'running', 'working'),
    process(2, 'agent', 'running', 'waiting'),
    process(3, 'agent', 'crashed', 'exited'),
    process(4, 'agent', 'running', 'needs_input')
  ];

  assert.equal(projectActivityRollup(processes, {}).state, 'needs_input');
  assert.equal(projectActivityRollup(processes.slice(0, 3), {}).state, 'crashed');
  assert.equal(projectActivityRollup(processes.slice(0, 2), {}).state, 'working');
  assert.equal(projectActivityRollup(processes.slice(1, 2), {}).state, 'waiting');
});

test('per-kind activity keeps agent, terminal, and command counts separate', () => {
  const processes = [
    process(1, 'agent', 'running', 'working'),
    process(2, 'agent', 'running', 'needs_input'),
    process(3, 'agent', 'running', 'waiting'),
    process(4, 'agent', 'crashed', 'exited'),
    process(5, 'agent', 'running', 'idle'),
    process(6, 'terminal', 'running'),
    process(7, 'terminal', 'starting'),
    process(8, 'terminal', 'crashed'),
    process(9, 'terminal', 'stopped'),
    process(10, 'command', 'running'),
    process(11, 'command', 'exited', 'idle', { exit_code: 7 }),
    process(12, 'command', 'stopped')
  ];

  const activity = projectKindActivity(processes, {
    6: { foreground_active: true }
  });

  assert.deepEqual(
    {
      running: activity.agent.running,
      needsInput: activity.agent.needsInput,
      crashed: activity.agent.crashed,
      waiting: activity.agent.waiting,
      idle: activity.agent.idle,
      stopped: activity.agent.stopped,
      total: activity.agent.total,
      tone: activity.agent.tone,
      label: activity.agent.label
    },
    {
      running: 1,
      needsInput: 1,
      crashed: 1,
      waiting: 1,
      idle: 1,
      stopped: 0,
      total: 5,
      tone: 'needs-input',
      label: '1 agent working · 1 needs input · 1 waiting · 1 crashed · 1 idle'
    }
  );
  assert.deepEqual(
    {
      running: activity.terminal.running,
      starting: activity.terminal.starting,
      crashed: activity.terminal.crashed,
      stopped: activity.terminal.stopped,
      total: activity.terminal.total,
      tone: activity.terminal.tone,
      label: activity.terminal.label
    },
    {
      running: 1,
      starting: 1,
      crashed: 1,
      stopped: 1,
      total: 4,
      tone: 'success',
      label: '1 terminal running · 1 starting · 1 crashed · 1 stopped'
    }
  );
  assert.equal(activity.command.running, 1);
  assert.equal(activity.command.crashed, 1);
  assert.equal(activity.command.stopped, 1);
  assert.equal(activity.command.total, 3);
  assert.equal(activity.command.label, '1 command running · 1 crashed · 1 stopped');
  assert.equal(activity.agent.active, 2);
  assert.equal(activity.agent.activeLabel, '1 agent working · 1 needs input');
  assert.equal(activity.terminal.active, 1);
  assert.equal(activity.terminal.activeLabel, '1 terminal live');
  assert.equal(activity.command.activeLabel, 'command-10 running');
});

test('per-kind tones prioritize input and live work before crashes and idle states', () => {
  const needsInput = process(1, 'agent', 'running', 'needs_input');
  const crashed = process(2, 'agent', 'crashed', 'exited');
  const working = process(3, 'agent', 'running', 'working');
  const waiting = process(4, 'agent', 'running', 'waiting');
  const idle = process(5, 'agent', 'running', 'idle');

  assert.equal(projectKindActivity([needsInput, crashed, working, waiting], {}).agent.tone, 'needs-input');
  assert.equal(projectKindActivity([crashed, working, waiting], {}).agent.tone, 'success');
  assert.equal(projectKindActivity([crashed, waiting], {}).agent.tone, 'danger');
  assert.equal(projectKindActivity([waiting], {}).agent.tone, 'idle');
  assert.equal(projectKindActivity([idle], {}).agent.tone, 'idle');
  assert.equal(projectKindActivity([], {}).agent.tone, 'idle');
});

test('non-zero and signaled exits are errors while clean exits are stopped', () => {
  const activity = projectKindActivity([
    process(1, 'agent', 'exited', 'exited', { exit_code: 0 }),
    process(2, 'agent', 'exited', 'exited', { exit_code: 9 }),
    process(3, 'terminal', 'exited', 'idle', { exit_signal: 15 }),
    process(4, 'command', 'exited', 'idle', { exit_code: 0 })
  ], {});

  assert.equal(activity.agent.crashed, 1);
  assert.equal(activity.agent.stopped, 1);
  assert.equal(activity.terminal.crashed, 1);
  assert.equal(activity.command.crashed, 0);
  assert.equal(activity.command.stopped, 1);
  assert.equal(activity.agent.tone, 'danger');
  assert.equal(activity.terminal.tone, 'danger');
  assert.equal(activity.command.tone, 'idle');
});

test('jump targets prefer input and foreground work, then output or process start recency', () => {
  const agents = [
    process(1, 'agent', 'running', 'working', {
      agent_state: { last_content_change_at: 900 }
    }),
    process(2, 'agent', 'running', 'needs_input', {
      agent_state: { last_content_change_at: 700 }
    }),
    process(3, 'agent', 'running', 'needs_input', {
      agent_state: { last_content_change_at: 800 }
    })
  ];
  const terminals = [process(4, 'terminal'), process(5, 'terminal')];
  const commands = [process(6, 'command'), process(7, 'command')];
  const activity = projectKindActivity([...agents, ...terminals, ...commands], {
    4: { foreground_active: false, uptime_seconds: 5 },
    5: { foreground_active: true, uptime_seconds: 50 },
    6: { foreground_active: false, uptime_seconds: 100 },
    7: { foreground_active: false, uptime_seconds: 10 }
  });

  assert.equal(activity.agent.targetProcessId, 3);
  assert.equal(activity.terminal.targetProcessId, 5);
  assert.equal(activity.command.targetProcessId, 7);
  assert.deepEqual(activity.agent.activeProcessIds, [3, 2, 1]);
  assert.deepEqual(activity.terminal.activeProcessIds, [5]);
  assert.deepEqual(activity.command.activeProcessIds, [7, 6]);
  assert.deepEqual(activity.agent.processIds, [3, 2, 1]);
  assert.equal(projectKindActivity([agents[0]], {}).agent.targetProcessId, 1);
  assert.equal(projectKindActivity([process(8, 'terminal', 'stopped')], {}).terminal.targetProcessId, null);
});

test('mixed rosters keep live terminals, commands, and fresh agents above exited processes', () => {
  const now = Date.now();
  const activity = projectKindActivity([
    process(1, 'agent', 'running', 'working'),
    process(2, 'agent', 'exited', 'exited', {
      exited_at: now,
      agent_state: { last_output_at: now }
    }),
    process(3, 'terminal', 'running'),
    process(4, 'terminal', 'exited', 'idle', { exited_at: now }),
    process(5, 'command', 'running'),
    process(6, 'command', 'exited', 'idle', { exited_at: now })
  ], {
    1: { uptime_seconds: 1 },
    3: { foreground_active: true, uptime_seconds: 2 },
    5: { uptime_seconds: 3 }
  });

  assert.deepEqual(activity.agent.processIds, [1, 2]);
  assert.deepEqual(activity.terminal.processIds, [3, 4]);
  assert.deepEqual(activity.command.processIds, [5, 6]);
});

test('per-kind activity hides idle shells and includes agent and command startup', () => {
  const idleTerminal = process(1, 'terminal', 'running');
  const activeTerminal = process(2, 'terminal', 'running');
  const startingTerminal = process(3, 'terminal', 'starting');
  const startingAgent = process(4, 'agent', 'starting', 'idle');
  const startingCommand = process(5, 'command', 'starting');
  const activity = projectKindActivity(
    [idleTerminal, activeTerminal, startingTerminal, startingAgent, startingCommand],
    {
      1: { foreground_active: false },
      2: { foreground_active: true },
      3: { foreground_active: false, uptime_seconds: 2 },
      4: { uptime_seconds: 3 },
      5: { uptime_seconds: 4 }
    }
  );

  assert.equal(activity.terminal.active, 1);
  assert.equal(activity.terminal.idle, 1);
  assert.equal(activity.terminal.starting, 1);
  assert.deepEqual(activity.terminal.activeProcessIds, [2]);
  assert.equal(activity.agent.active, 1);
  assert.equal(activity.agent.activeLabel, '1 agent starting');
  assert.deepEqual(activity.agent.activeProcessIds, [4]);
  assert.equal(activity.command.active, 1);
  assert.equal(activity.command.activeLabel, 'command-5 starting');
});

test('per-kind labels cover empty, plural, and quiet states', () => {
  const empty = projectKindActivity([], {});
  assert.equal(empty.agent.label, 'no agents');
  assert.equal(empty.terminal.label, 'no terminals');
  assert.equal(empty.command.label, 'no commands');

  const quiet = projectKindActivity([
    process(1, 'agent', 'running', 'idle'),
    process(2, 'agent', 'running', 'idle'),
    process(3, 'terminal', 'stopped'),
    process(4, 'terminal', 'stopped')
  ], {});
  assert.equal(quiet.agent.label, '2 agents idle');
  assert.equal(quiet.terminal.label, '2 terminals stopped');
  assert.deepEqual(quiet.agent.processIds, [2, 1]);
  assert.deepEqual(quiet.terminal.processIds, [4, 3]);
});
