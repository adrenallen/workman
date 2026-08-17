import assert from 'node:assert/strict';
import test from 'node:test';

import {
  defaultProjectTitleFromPath,
  defaultWorktreeTitle,
  registrationTitleForPath,
  resolvedProjectTitle,
  syncProjectTitleDefault
} from '../src/lib/projectTitles.ts';

test('registered project titles keep the raw folder basename', () => {
  assert.equal(defaultProjectTitleFromPath('/tmp/client_portal-v2'), 'client_portal-v2');
  assert.equal(defaultProjectTitleFromPath('/tmp/client_portal-v2/'), 'client_portal-v2');
  assert.equal(defaultProjectTitleFromPath('C:\\Code\\client_portal-v2'), 'client_portal-v2');
  assert.equal(defaultProjectTitleFromPath('/tmp/a\\b'), 'a\\b');
});

test('worktree titles preserve the full branch and adopt paths fall back to their folder', () => {
  assert.equal(defaultWorktreeTitle('feat/inline-drafts'), 'feat/inline-drafts');
  assert.equal(defaultWorktreeTitle('release'), 'release');
  assert.equal(defaultWorktreeTitle('refs/heads/feat/inline-drafts'), 'feat/inline-drafts');
  assert.equal(defaultWorktreeTitle('feat/trailing/', '/tmp/ignored/'), 'feat/trailing');
  assert.equal(defaultWorktreeTitle('', '/tmp/existing-checkout'), 'existing-checkout');
  assert.equal(defaultWorktreeTitle(''), '');
});

test('empty title edits resolve to a non-empty default', () => {
  assert.equal(resolvedProjectTitle('  Custom title  ', 'folder-name'), 'Custom title');
  assert.equal(resolvedProjectTitle('   ', 'folder-name'), 'folder-name');
  assert.equal(resolvedProjectTitle('', ''), 'Project');
});

test('registration defaults to an existing custom title for the exact known path', () => {
  const projects = [
    { path: '/tmp/client', name: 'client', display_name: '  Client workspace  ' },
    { path: '/tmp/plain', name: 'plain', display_name: null }
  ];
  assert.equal(registrationTitleForPath('/tmp/client', projects), 'Client workspace');
  assert.equal(registrationTitleForPath('/tmp/plain', projects), 'plain');
  assert.equal(registrationTitleForPath('/tmp/new-folder', projects), 'new-folder');
});

test('a user-edited title survives later branch default changes', () => {
  let title = syncProjectTitleDefault('', 'feat/first', false);
  assert.equal(title, 'feat/first');
  title = 'My checkout';
  title = syncProjectTitleDefault(title, 'fix/later', true);
  assert.equal(title, 'My checkout');
});
