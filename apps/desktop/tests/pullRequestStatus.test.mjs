import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

import {
  pullRequestDetail,
  pullRequestInteraction,
  pullRequestLabel,
  pullRequestVisual,
  pullRequestsForWorktree
} from '../src/lib/worktrees.ts';

function pullRequest(state, mergeable = 'mergeable') {
  return {
    number: 42,
    title: 'Ship the feature',
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

test('PR controls open one result directly and use a list for branch history', () => {
  const open = pullRequest('open');
  const merged = { ...pullRequest('merged'), number: 41, title: 'Earlier merged attempt' };

  assert.equal(pullRequestInteraction([]), 'none');
  assert.equal(pullRequestInteraction([open]), 'direct');
  assert.equal(pullRequestInteraction([open, merged]), 'list');
  assert.deepEqual(
    pullRequestsForWorktree({ pull_request: open, pull_requests: [open, merged] }),
    [open, merged]
  );
  assert.deepEqual(
    pullRequestsForWorktree({ pull_request: open, pull_requests: [] }),
    [open],
    'the singular field remains a compatibility fallback'
  );
});

test('PR states map to distinct standard icons and semantic colors', () => {
  assert.deepEqual(pullRequestVisual('merged'), {
    icon: 'git-merge',
    color: 'var(--pull-request-merged)'
  });
  assert.deepEqual(pullRequestVisual('open'), {
    icon: 'git-pull-request',
    color: 'var(--success)'
  });
  assert.deepEqual(pullRequestVisual('closed'), {
    icon: 'git-pull-request-closed',
    color: 'var(--destructive)'
  });
  assert.deepEqual(pullRequestVisual('draft'), {
    icon: 'git-pull-request-draft',
    color: 'var(--muted-foreground)'
  });
});

test('merged purple uses GitHub light color and a legible dark-theme variant', async () => {
  const styles = await readFile(new URL('../src/styles.css', import.meta.url), 'utf8');
  assert.match(styles, /:root \{[\s\S]*--pull-request-merged: #a371f7;/);
  assert.match(styles, /:root\[data-theme='light'\] \{[\s\S]*--pull-request-merged: #8250df;/);
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

test('every PR status surface uses the shared state icon treatment', async () => {
  const row = await readFile(new URL('../src/lib/WorktreeRowMeta.svelte', import.meta.url), 'utf8');
  const overview = await readFile(new URL('../src/lib/ProjectOverview.svelte', import.meta.url), 'utf8');
  const quickJump = await readFile(new URL('../src/lib/QuickJumpPalette.svelte', import.meta.url), 'utf8');
  const contextMenu = await readFile(new URL('../src/lib/ContextMenu.svelte', import.meta.url), 'utf8');
  const terminalStatus = await readFile(new URL('../src/lib/ProcessStatusBar.svelte', import.meta.url), 'utf8');
  const icon = await readFile(new URL('../src/lib/PullRequestStateIcon.svelte', import.meta.url), 'utf8');
  const list = await readFile(new URL('../src/lib/PullRequestList.svelte', import.meta.url), 'utf8');

  assert.match(row, /\{#if pullRequest\}/);
  assert.match(row, /pullRequestInteraction\(pullRequests\)/);
  assert.match(row, /\{#if pullRequestMode === 'direct'\}[\s\S]*openPullRequest\(pullRequest\)[\s\S]*\{:else\}[\s\S]*<PullRequestList/);
  assert.match(row, /openBrowserUrl\(target\.url\)/);
  assert.match(row, /<PullRequestStateIcon state=\{pullRequest\.state\}/);
  assert.match(overview, /if pullRequest/);
  assert.match(overview, /openBrowserUrl\(pullRequest!\.url\)/);
  assert.match(overview, /\{#if pullRequestMode === 'direct'\}[\s\S]*\{:else\}[\s\S]*<PullRequestList/);
  assert.match(overview, /<PullRequestStateIcon state=\{pullRequest\.state\}/);
  assert.match(quickJump, /pullRequests: Record<number, PullRequestStatus\[\]>/);
  assert.match(quickJump, /<PullRequestStateIcon state=\{entryPullRequest\.state\}/);
  assert.match(contextMenu, /<PullRequestStateIcon state=\{item\.pullRequestState\}/);
  assert.match(terminalStatus, /openBrowserUrl\(target\.url\)/);
  assert.match(terminalStatus, /\{#if pullRequestMode === 'direct'\}[\s\S]*\{:else\}[\s\S]*<PullRequestList/);
  assert.match(terminalStatus, /<PullRequestStateIcon state=\{pullRequest\.state\}/);
  assert.match(list, /\{#each pullRequests as pullRequest \(pullRequest\.number\)\}/);
  assert.match(list, /<PullRequestStateIcon state=\{pullRequest\.state\}/);
  assert.match(list, /#\{pullRequest\.number\}[\s\S]*\{title\(pullRequest\)\}/);
  assert.match(icon, /visual\.icon === 'git-merge'[\s\S]*<GitMergeIcon/);
  assert.match(icon, /visual\.icon === 'git-pull-request-closed'[\s\S]*<GitPullRequestClosedIcon/);
});
