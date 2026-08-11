import assert from 'node:assert/strict';
import test from 'node:test';

import { liveAgentDescendants, planAgentCascade } from '../src/lib/agentCascade.ts';

function process(id, parentId, status = 'running', kind = 'agent') {
  return { id, spawned_by_process_id: parentId, status, kind };
}

test('finds recursive live agent descendants without swallowing composed sibling order', () => {
  const descendants = liveAgentDescendants([
    process(1, null),
    process(2, 1),
    process(3, 1),
    process(4, 2),
    process(5, 1, 'running', 'terminal'),
  ], 1);

  assert.deepEqual(descendants.map(({ id }) => id), [2, 4, 3]);
});

test('traverses stopped lineage nodes to find live grandchildren', () => {
  const descendants = liveAgentDescendants([
    process(1, null),
    process(2, 1, 'stopped'),
    process(3, 2),
  ], 1);

  assert.deepEqual(descendants.map(({ id }) => id), [3]);
});

test('breaks malformed lineage cycles', () => {
  const descendants = liveAgentDescendants([
    process(1, 2),
    process(2, 1),
  ], 1);

  assert.deepEqual(descendants.map(({ id }) => id), [2]);
});

test('plans selected parent and child as one daemon root without double-counting impact', () => {
  const processes = [
    process(1, null),
    process(2, 1),
    process(3, 2),
    process(4, null),
  ];
  const plan = planAgentCascade(processes, [processes[0], processes[1], processes[3]]);

  assert.deepEqual(plan.selected.map(({ id }) => id), [1, 2, 4]);
  assert.deepEqual(plan.actionRoots.map(({ id }) => id), [1, 4]);
  assert.deepEqual(plan.additionalDescendants.map(({ id }) => id), [3]);
});

test('close plans include stopped descendants because their stored entries are removed', () => {
  const processes = [process(1, null), process(2, 1, 'stopped'), process(3, 2)];
  const plan = planAgentCascade(processes, [processes[0]], true);
  assert.deepEqual(plan.additionalDescendants.map(({ id }) => id), [2, 3]);
});
