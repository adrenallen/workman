import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

import { pullRequestDetail, pullRequestLabel } from '../src/lib/worktrees.ts';

function pullRequest(state, mergeable = 'mergeable') {
  return {
    number: 42,
    state,
    url: 'https://example.test/pull/42',
    checks: 'passing',
    mergeable
  };
}

test('open PR labels include mergeability while terminal labels report only terminal state', () => {
  assert.equal(pullRequestDetail(pullRequest('open')), 'Checks passing · Mergeable');
  assert.equal(
    pullRequestLabel(pullRequest('open')),
    'Pull request #42 open · Checks passing · Mergeable'
  );

  for (const state of ['merged', 'closed']) {
    const detail = pullRequestDetail(pullRequest(state));
    const label = pullRequestLabel(pullRequest(state));
    assert.equal(detail, 'Checks passing');
    assert.match(label, new RegExp(`Pull request #42 ${state}`));
    assert.doesNotMatch(label, /mergeable|mergeability|conflict/i);
  }
});

test('manual PR refresh forces a fresh daemon lookup and installs its direct response', async () => {
  const app = await readFile(new URL('../src/App.svelte', import.meta.url), 'utf8');
  const refresh = app.slice(
    app.indexOf('async function refreshWorktreeMetadata'),
    app.indexOf('async function refreshProjects')
  );
  const action = app.slice(
    app.indexOf("case 'refresh-worktrees':"),
    app.indexOf("case 'open-pull-request':")
  );

  assert.match(refresh, /client\.worktrees\(root\.id, refreshPullRequests\)/);
  assert.match(refresh, /worktreeLists = \{ \.\.\.worktreeLists, \[repositoryId\]: list \};/);
  assert.match(refresh, /refreshWorktreeMetadata\(projects, refreshPullRequests, true, root\.repository_id\)/);
  assert.match(action, /await refreshWorktreeRepository\(project, true\)/);
});

test('merged and closed PR URLs remain available from both row and overview surfaces', async () => {
  const row = await readFile(new URL('../src/lib/WorktreeRowMeta.svelte', import.meta.url), 'utf8');
  const overview = await readFile(new URL('../src/lib/ProjectOverview.svelte', import.meta.url), 'utf8');

  assert.match(row, /\{#if pullRequest\}/);
  assert.match(row, /openBrowserUrl\(pullRequest\.url\)/);
  assert.match(row, /pullRequest\.state === 'merged'[\s\S]*<GitMergeIcon/);
  assert.match(row, /pullRequest\.state === 'closed'[\s\S]*<GitPullRequestClosedIcon/);
  assert.match(overview, /if pullRequest/);
  assert.match(overview, /openBrowserUrl\(pullRequest!\.url\)/);
});
