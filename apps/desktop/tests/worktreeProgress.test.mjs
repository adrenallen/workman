import assert from 'node:assert/strict';
import test from 'node:test';
import { get } from 'svelte/store';

import {
  beginWorktreeOperation,
  dismissWorktreeOperation,
  replaceWorktreeOperations,
  resetWorktreeOperations,
  worktreeOperations
} from '../src/lib/worktreeProgress.ts';

function serverOperation(id, status = 'failed') {
  const now = Date.now();
  return {
    id,
    mode: 'create',
    source_project_id: 1,
    repository_id: 9,
    branch: `feature/${id}`,
    path: `/tmp/${id}`,
    label: id,
    status,
    steps: [],
    error: status === 'failed' ? 'fixture failure' : null,
    project: null,
    created_at: now,
    updated_at: now
  };
}

test('dismiss masks reconnect snapshots immediately and requests daemon cleanup', async () => {
  resetWorktreeOperations();
  beginWorktreeOperation({
    id: 'failed-create',
    mode: 'create',
    sourceProjectId: 1,
    repositoryId: 9,
    branch: 'feature/failed-create'
  });
  let remotelyDismissed = null;
  dismissWorktreeOperation('failed-create', async (id) => {
    remotelyDismissed = id;
  });
  replaceWorktreeOperations([serverOperation('failed-create')]);
  await Promise.resolve();

  assert.equal(remotelyDismissed, 'failed-create');
  assert.deepEqual(get(worktreeOperations), []);
  resetWorktreeOperations();
});

test('a new operation id clears only its own optimistic dismissal mask', () => {
  resetWorktreeOperations();
  dismissWorktreeOperation('retry-id');
  beginWorktreeOperation({
    id: 'retry-id',
    mode: 'fork',
    sourceProjectId: 1,
    repositoryId: 9,
    branch: 'feature/retry'
  });
  replaceWorktreeOperations([serverOperation('retry-id', 'running')]);

  assert.equal(get(worktreeOperations).length, 1);
  assert.equal(get(worktreeOperations)[0].id, 'retry-id');
  resetWorktreeOperations();
});
