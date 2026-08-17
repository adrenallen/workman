import assert from 'node:assert/strict';
import test from 'node:test';

import {
  defaultProjectTitleFromPath,
  defaultWorktreeTitle,
  resolvedProjectTitle
} from '../src/lib/projectTitles.ts';

test('registered project titles keep the raw folder basename', () => {
  assert.equal(defaultProjectTitleFromPath('/tmp/client_portal-v2'), 'client_portal-v2');
  assert.equal(defaultProjectTitleFromPath('/tmp/client_portal-v2/'), 'client_portal-v2');
  assert.equal(defaultProjectTitleFromPath('C:\\Code\\client_portal-v2'), 'client_portal-v2');
});

test('worktree titles use the final branch segment and adopt paths fall back to their folder', () => {
  assert.equal(defaultWorktreeTitle('feat/inline-drafts'), 'inline-drafts');
  assert.equal(defaultWorktreeTitle('release'), 'release');
  assert.equal(defaultWorktreeTitle('', '/tmp/existing-checkout'), 'existing-checkout');
});

test('empty title edits resolve to a non-empty default', () => {
  assert.equal(resolvedProjectTitle('  Custom title  ', 'folder-name'), 'Custom title');
  assert.equal(resolvedProjectTitle('   ', 'folder-name'), 'folder-name');
  assert.equal(resolvedProjectTitle('', ''), 'Project');
});
