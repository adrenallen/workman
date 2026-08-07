import assert from 'node:assert/strict';
import test from 'node:test';

import { moveOrderedId } from '../src/lib/reorder.ts';
import { initialFlatProjectOrder, worktreeParentLabel } from '../src/lib/worktrees.ts';

function project(id, parentProjectId = null, name = `project-${id}`, displayName = null) {
  return { id, parent_project_id: parentProjectId, name, display_name: displayName };
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

test('flat reordering can separate a worktree from its parent without moving a block', () => {
  assert.deepEqual(moveOrderedId([1, 3, 4, 2, 5], 3, 2, 'after'), [1, 4, 2, 3, 5]);
});

test('labels a flat worktree with its parent and keeps an orphan fallback', () => {
  const parent = project(1, null, 'repository', 'Client site');
  assert.equal(worktreeParentLabel(project(2, 1, 'repository: topic'), [parent]), 'Client site');
  assert.equal(worktreeParentLabel(project(3, 99, 'repository: orphan'), [], 'Repository'), 'Repository');
  assert.equal(worktreeParentLabel(parent, [parent]), null);
});
