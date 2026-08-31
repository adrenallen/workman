import assert from 'node:assert/strict';
import test from 'node:test';
import { get } from 'svelte/store';

import {
  beginWorktreeOperation,
  dismissWorktreeOperation,
  replaceWorktreeOperations,
  resetWorktreeOperations,
  standaloneWorktreeOperations,
  worktreeOperationForProject,
  worktreeOperationStateLabel,
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
    error_code: status === 'failed' ? 'fixture_failure' : null,
    project: null,
    removal: null,
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

test('remove operations use removal phases and preserve terminal removal details', () => {
  resetWorktreeOperations();
  const local = beginWorktreeOperation({
    id: 'remove-project',
    mode: 'remove',
    sourceProjectId: 7,
    repositoryId: null,
    path: '/tmp/plain-project',
    label: 'Plain project'
  });
  assert.equal(local.label, 'Plain project');
  assert.deepEqual(local.steps.map((step) => step.id), [
    'processes',
    'worktree',
    'files',
    'prune',
    'registered'
  ]);

  replaceWorktreeOperations([{
    ...serverOperation('remove-project', 'completed'),
    mode: 'remove',
    source_project_id: 7,
    repository_id: null,
    path: '/tmp/plain-project',
    label: 'plain-project',
    removal: {
      project_id: 7,
      path: '/tmp/plain-project',
      branch: 'Plain project',
      removed: true,
      project_unregistered: true,
      deleted_from_disk: false,
      metadata_pruned: false,
      branch_kept: true,
      delete_from_disk: true,
      files_removed: false,
      files_untouched: true,
      registration_issue: 'broken registration',
      post_delete_warning: null
    }
  }]);
  assert.equal(get(worktreeOperations)[0].label, 'Plain project');
  assert.equal(get(worktreeOperations)[0].removal.registration_issue, 'broken registration');
  assert.equal(get(worktreeOperations)[0].removal.files_untouched, true);
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

test('remove operations attach to their existing project instead of adding a rail row', () => {
  const project = {
    id: 7,
    path: '/tmp/plain-project'
  };
  const removal = {
    ...serverOperation('remove-project', 'running'),
    mode: 'remove',
    source_project_id: project.id,
    repository_id: 9,
    path: project.path
  };

  assert.equal(worktreeOperationForProject([removal], project)?.id, removal.id);
  assert.deepEqual(standaloneWorktreeOperations([removal], [project]), []);
  assert.equal(worktreeOperationStateLabel(removal), 'Removing…');
});

test('create operations become part of the project row once the target path is registered', () => {
  const source = { id: 1, path: '/tmp/repository' };
  const created = { id: 8, path: '/tmp/repository-feature' };
  const creation = {
    ...serverOperation('create-project', 'running'),
    mode: 'create',
    source_project_id: source.id,
    repository_id: 9,
    path: `${created.path}/`
  };

  assert.equal(worktreeOperationForProject([creation], source), null);
  assert.equal(worktreeOperationForProject([creation], created)?.id, creation.id);
  assert.deepEqual(standaloneWorktreeOperations([creation], [source, created]), []);
  assert.equal(worktreeOperationStateLabel(creation), 'Creating…');
});

test('operations without a registered target remain one temporary rail row', () => {
  const source = { id: 1, path: '/tmp/repository' };
  const creation = {
    ...serverOperation('create-project', 'running'),
    mode: 'fork',
    source_project_id: source.id,
    repository_id: 9,
    path: '/tmp/repository-feature'
  };

  assert.deepEqual(standaloneWorktreeOperations([creation], [source]), [creation]);
  assert.equal(worktreeOperationStateLabel(creation), 'Forking…');
});
