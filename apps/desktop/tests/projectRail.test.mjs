import assert from 'node:assert/strict';
import test from 'node:test';

import { initialFlatProjectOrder } from '../src/lib/worktrees.ts';

function project(id, parentProjectId = null) {
  return { id, parent_project_id: parentProjectId };
}

test('seeds a flat rail with each parent followed by its existing worktrees once', () => {
  assert.deepEqual(
    initialFlatProjectOrder([
      project(1),
      project(2),
      project(3, 1),
      project(4, 1),
      project(5, 2)
    ]),
    [1, 3, 4, 2, 5]
  );
});

test('keeps orphaned worktrees visible and preserves stable sibling order', () => {
  assert.deepEqual(
    initialFlatProjectOrder([
      project(8, 99),
      project(1),
      project(4, 1),
      project(3, 1),
      project(2)
    ]),
    [8, 1, 4, 3, 2]
  );
});

test('is idempotent once the initial parent-followed order has been seeded', () => {
  assert.deepEqual(
    initialFlatProjectOrder([project(1), project(3, 1), project(4, 1), project(2), project(5, 2)]),
    [1, 3, 4, 2, 5]
  );
});
