import assert from 'node:assert/strict';
import test from 'node:test';

import {
  processActivity,
  processActivityTone,
  projectActivityRollup
} from '../src/lib/processActivity.ts';

function process(id, kind, status = 'running', attention = 'idle') {
  return {
    id,
    kind,
    name: `${kind}-${id}`,
    status,
    agent_state: {
      state: attention,
      working: attention === 'working',
      needs_input: attention === 'needs_input',
      idle: attention === 'idle' || attention === 'waiting'
    }
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
