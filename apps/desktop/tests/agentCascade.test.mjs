import assert from 'node:assert/strict';
import test from 'node:test';

import { liveAgentDescendants } from '../src/lib/agentCascade.ts';

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
