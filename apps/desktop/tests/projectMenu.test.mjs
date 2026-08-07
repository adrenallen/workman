import assert from 'node:assert/strict';
import test from 'node:test';

import { projectFrequentActions } from '../src/lib/projectMenu.ts';

test('frequent project actions stay pinned in the requested order', () => {
  const items = projectFrequentActions({
    editorLabel: 'Open in Visual Studio Code',
    pullRequest: { number: 47, state: 'open' },
    siteUrl: 'https://checkout-polish.test'
  });

  assert.deepEqual(items.map((item) => item.id), [
    'open-pull-request',
    'open-in-editor',
    'open-in-finder',
    'open-herd-site'
  ]);
  assert.deepEqual(items.map((item) => item.label), [
    'Open PR #47 on GitHub',
    'Open in Visual Studio Code',
    'Open in Finder',
    'Open app'
  ]);
});

test('closed PRs and missing app URLs do not create false actions', () => {
  const items = projectFrequentActions({
    editorLabel: 'Open in Zed',
    pullRequest: { number: 47, state: 'closed' },
    siteUrl: null
  });

  assert.deepEqual(items, [
    { id: 'open-in-editor', label: 'Open in Zed' },
    { id: 'open-in-finder', label: 'Open in Finder' }
  ]);
});

test('draft pull requests are still open and truthfully labeled', () => {
  const [pullRequest] = projectFrequentActions({
    editorLabel: 'Open in editor',
    pullRequest: { number: 8, state: 'draft' }
  });

  assert.deepEqual(pullRequest, {
    id: 'open-pull-request',
    label: 'Open PR #8 on GitHub',
    detail: 'Draft pull request'
  });
});
