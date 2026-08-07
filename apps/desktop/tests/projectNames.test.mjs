import assert from 'node:assert/strict';
import test from 'node:test';

import {
  projectBranchLabel,
  projectDisplayName,
  projectRepositoryTitle
} from '../src/lib/worktrees.ts';

function project(overrides = {}) {
  return {
    id: 7,
    name: 'repo: feature/default-name',
    display_name: null,
    branch: 'feature/default-name',
    ...overrides
  };
}

test('an explicit project name wins over worktree-derived labels everywhere', () => {
  const renamed = project({ display_name: '  Checkout polish  ' });
  const repository = { name: 'repo' };

  assert.equal(projectDisplayName(renamed), 'Checkout polish');
  assert.equal(projectBranchLabel(renamed), 'Checkout polish');
  assert.equal(projectRepositoryTitle(renamed, repository), 'Checkout polish');
});

test('worktree branch and repository labels remain the defaults without a rename', () => {
  const derived = project();

  assert.equal(projectDisplayName(derived), 'repo: feature/default-name');
  assert.equal(projectBranchLabel(derived), 'feature/default-name');
  assert.equal(projectRepositoryTitle(derived, { name: 'repo' }), 'repo: feature/default-name');
});
