import assert from 'node:assert/strict';
import test from 'node:test';

import {
  creationDraftHasContent,
  creationDraftLabel,
  creationDraftStorageKey,
  creationDraftsForCycle,
  createCreationDraft,
  findUntouchedCreationDraft,
  loadCreationDrafts,
  nextCreationDraftId,
  pruneCreationDraftsToProjects,
  saveCreationDrafts
} from '../src/lib/creationDrafts.ts';

function memoryStorage(initial = {}) {
  const values = new Map(Object.entries(initial));
  return {
    getItem(key) { return values.get(key) ?? null; },
    setItem(key, value) { values.set(key, value); },
    values
  };
}

test('finds only untouched drafts for the same project and kind', () => {
  const agent = createCreationDraft('agent', 7, -1, 10);
  const touchedAgent = { ...createCreationDraft('agent', 7, -2, 20), touched: true };
  const otherProject = createCreationDraft('agent', 8, -3, 30);
  const todo = createCreationDraft('todo', 7, -4, 40);

  assert.equal(findUntouchedCreationDraft([touchedAgent, otherProject, todo, agent], 7, 'agent')?.id, -1);
  assert.equal(findUntouchedCreationDraft([touchedAgent], 7, 'agent'), null);
  assert.ok(nextCreationDraftId([agent, touchedAgent, otherProject, todo], 1_000) < -1_000);
});

test('uses the touched flag consistently for reuse and discard confirmation', () => {
  const agent = createCreationDraft('agent', 7, -1);
  const command = createCreationDraft('command', 7, -2);
  const todo = createCreationDraft('todo', 7, -3);
  assert.equal(creationDraftHasContent(agent), false);
  assert.equal(creationDraftHasContent(command), false);
  assert.equal(creationDraftHasContent(todo), false);
  assert.equal(creationDraftHasContent({ ...agent, agentToolId: 4, touched: true }), true);
  assert.equal(creationDraftHasContent({ ...command, autoStart: false, touched: true }), true);
  assert.equal(creationDraftHasContent({ ...todo, priority: 'high', touched: true }), true);
});

test('allocates non-recycled time-based ids after all live drafts are removed', () => {
  const first = nextCreationDraftId([], 10_000);
  const second = nextCreationDraftId([], 10_001);
  assert.ok(second < first);
});

test('persistence round-trips each kind per profile and clones array fields', () => {
  const storage = memoryStorage();
  const drafts = [
    {
      ...createCreationDraft('agent', 7, -1, 10),
      prompt: 'Keep this prompt',
      attachments: ['/tmp/workman/image-one.png'],
      touched: true
    },
    { ...createCreationDraft('command', 7, -2, 20), name: 'Vite', command: 'npm run dev', saveMode: 'local', touched: true },
    { ...createCreationDraft('todo', 8, -3, 30), title: 'Ship it', blockerIds: [4, 5], touched: true }
  ];
  saveCreationDrafts(3, drafts, storage);
  assert.deepEqual(loadCreationDrafts(3, storage), drafts);
  assert.deepEqual(loadCreationDrafts(4, storage), []);

  const loaded = loadCreationDrafts(3, storage);
  loaded[0].attachments.push('/tmp/workman/image-two.png');
  loaded[2].blockerIds.push(99);
  assert.deepEqual(loadCreationDrafts(3, storage)[0].attachments, ['/tmp/workman/image-one.png']);
  assert.deepEqual(loadCreationDrafts(3, storage)[2].blockerIds, [4, 5]);
});

test('agent draft attachments start empty and restore only bounded absolute paths', () => {
  const empty = createCreationDraft('agent', 7, -1, 10);
  assert.deepEqual(empty.attachments, []);

  const storage = memoryStorage({
    [creationDraftStorageKey(3)]: JSON.stringify({
      version: 1,
      drafts: [
        { ...empty, attachments: ['/tmp/a.png', '/tmp/b.webp'], touched: true },
        { ...empty, id: -2, attachments: ['relative.png'], touched: true },
        { ...empty, id: -3, attachments: Array(9).fill('/tmp/a.png'), touched: true }
      ]
    })
  });
  assert.deepEqual(loadCreationDrafts(3, storage), [
    { ...empty, attachments: ['/tmp/a.png', '/tmp/b.webp'], touched: true }
  ]);
});

test('bad persisted data is ignored while valid drafts survive', () => {
  const valid = createCreationDraft('todo', 7, -2, 20);
  const storage = memoryStorage({
    [creationDraftStorageKey(3)]: JSON.stringify({
      version: 1,
      drafts: [
        valid,
        { ...valid, id: 2 },
        { ...valid, priority: 'urgent' },
        { ...valid, blockerIds: ['bad'] },
        null
      ]
    }),
    [creationDraftStorageKey(4)]: '{broken'
  });
  assert.deepEqual(loadCreationDrafts(3, storage), [valid]);
  assert.deepEqual(loadCreationDrafts(4, storage), []);
});

test('restoration drops duplicate ids and enforces count and text limits', () => {
  const storage = memoryStorage();
  const drafts = Array.from({ length: 105 }, (_, index) => ({
    ...createCreationDraft('agent', 7, -(index + 1), index + 1),
    prompt: `prompt ${index}`
  }));
  drafts.splice(1, 0, { ...drafts[0], prompt: 'duplicate' });
  drafts.splice(2, 0, { ...drafts[2], id: -999, prompt: 'x'.repeat(256_001) });
  storage.setItem(creationDraftStorageKey(3), JSON.stringify({ version: 1, drafts }));

  const loaded = loadCreationDrafts(3, storage);
  assert.equal(loaded.length, 100);
  assert.equal(new Set(loaded.map((draft) => draft.id)).size, loaded.length);
  assert.doesNotMatch(loaded.map((draft) => draft.prompt).join('\n'), /duplicate/);
});

test('project pruning preserves the array identity when no draft is removed', () => {
  const drafts = [
    createCreationDraft('agent', 7, -1),
    createCreationDraft('todo', 8, -2)
  ];
  assert.equal(pruneCreationDraftsToProjects(drafts, new Set([7, 8])), drafts);
  assert.deepEqual(
    pruneCreationDraftsToProjects(drafts, new Set([8])).map((draft) => draft.id),
    [-2]
  );
});

test('labels update from names and titles with stable fallbacks', () => {
  assert.equal(creationDraftLabel(createCreationDraft('agent', 7, -1)), 'New agent');
  assert.equal(creationDraftLabel({ ...createCreationDraft('agent', 7, -1), name: 'Reviewer' }), 'Reviewer');
  assert.equal(creationDraftLabel({ ...createCreationDraft('command', 7, -2), name: 'Dev server' }), 'Dev server');
  assert.equal(creationDraftLabel({ ...createCreationDraft('todo', 7, -3), title: 'Document API' }), 'Document API');
});

test('cycle inclusion is project-scoped, ordered, and excludes todo drafts', () => {
  const drafts = [
    createCreationDraft('command', 7, -4, 40),
    createCreationDraft('agent', 8, -3, 30),
    createCreationDraft('todo', 7, -2, 20),
    createCreationDraft('agent', 7, -1, 10),
    createCreationDraft('agent', 7, -5, 50)
  ];
  assert.deepEqual(creationDraftsForCycle(drafts, 7, 'agent').map((draft) => draft.id), [-1, -5]);
  assert.deepEqual(creationDraftsForCycle(drafts, 7, 'command').map((draft) => draft.id), [-4]);
});
