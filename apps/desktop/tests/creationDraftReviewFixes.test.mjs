import assert from 'node:assert/strict';
import { access, readFile } from 'node:fs/promises';
import test from 'node:test';

const appUrl = new URL('../src/App.svelte', import.meta.url);

test('draft selection is project-scoped and agent removal waits for an optimistic start', async () => {
  const app = await readFile(appUrl, 'utf8');
  assert.match(app, /draft\.id === currentSelection\.id && draft\.projectId === currentSelection\.projectId/);
  assert.match(
    app,
    /if \(spawnAgent\(submission\.tool, submission\.input, submission\.template\)\) \{\s*removeCreationDraft\(draft\.id\);/
  );
  assert.match(app, /if \(!currentProject\) return false;/);
  assert.match(app, /if \(!project\) return false;/);
});

test('draft persistence is trailing-debounced, flushed on pagehide, and profile snapshots are stable', async () => {
  const app = await readFile(appUrl, 'utf8');
  assert.match(app, /creationDraftSaveTimer = setTimeout\(flushCreationDraftPersistence, 400\)/);
  assert.match(app, /window\.addEventListener\('pagehide', flushCreationDraftPersistence\)/);
  assert.match(app, /const profileIdBefore = await resolveActiveProfileId\(\);[\s\S]*const profileIdAfter = await resolveActiveProfileId\(\);/);
  assert.match(app, /if \(profileIdBefore === profileIdAfter\)/);
  assert.match(app, /if \(nextDrafts !== creationDrafts\) replaceCreationDrafts/);
  assert.doesNotMatch(app, /\$effect\(\(\) => \{\s*if \(!creationDraftsLoaded[\s\S]*saveCreationDrafts/);
});

test('add-action reuse requests focus, discard clears it, and drafts stay out of recents', async () => {
  const app = await readFile(appUrl, 'utf8');
  const openDraft = app.slice(
    app.indexOf('function openCreationDraft'),
    app.indexOf('function openCommandDraft')
  );
  assert.match(openDraft, /draftFocusRequestId = draft\.id;/);
  assert.match(app, /if \(draftFocusRequestId === draftId\) draftFocusRequestId = null;/);
  assert.match(app, /if \(next\.kind !== 'draft'\) \{[\s\S]*recordRecentNavigation/);
  assert.match(app, /patchCreationDraft\(draft\.id, \{ templateId: null, agentToolId: preferredToolId \}\);/);
});

test('nested submit buttons are ignored and the tree draft pill is decorative', async () => {
  const [formConventions, row] = await Promise.all([
    readFile(new URL('../src/lib/formInputConventions.ts', import.meta.url), 'utf8'),
    readFile(new URL('../src/lib/CreationDraftTreeRow.svelte', import.meta.url), 'utf8')
  ]);
  assert.match(formConventions, /find\(\(candidate\) => candidate\.form === form\)/);
  assert.match(row, /class="draft-pill" aria-hidden="true"/);
});

test('quick jump excludes unindexed drafts and orphan creation modals are removed', async () => {
  const quickJump = await readFile(
    new URL('../src/lib/QuickJumpPalette.svelte', import.meta.url),
    'utf8'
  );
  assert.match(quickJump, /Exclude<ProjectTreeItemKind, 'draft'>/);
  assert.doesNotMatch(quickJump, /case 'draft'/);
  await assert.rejects(access(new URL('../src/lib/AgentsPanel.svelte', import.meta.url)));
  await assert.rejects(access(new URL('../src/lib/NewAgentDialog.svelte', import.meta.url)));
});
