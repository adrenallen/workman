import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

import {
  beginOptimisticNavigation,
  selectProjectOptimistically
} from '../src/lib/optimisticNavigation.ts';

function project(id, selected = false) {
  return { id, selected };
}

test('project selection is reflected locally without waiting for daemon hydration', async () => {
  let selectedId = 1;
  let hydrationStarted = false;
  let finishHydration;
  const hydration = new Promise((resolve) => {
    finishHydration = resolve;
  });

  beginOptimisticNavigation(
    () => {
      selectedId = 2;
    },
    async () => {
      hydrationStarted = true;
      await hydration;
    },
    assert.fail
  );

  assert.equal(selectedId, 2, 'the visible selection changes in the click stack');
  assert.equal(hydrationStarted, false, 'daemon work is deferred behind the local transition');
  await Promise.resolve();
  assert.equal(hydrationStarted, true);
  assert.equal(selectedId, 2, 'an unresolved daemon request cannot hold back the page state');

  finishHydration();
  await hydration;
});

test('optimistic project selection updates only changed rows', () => {
  const first = project(1, true);
  const second = project(2, false);
  const third = project(3, false);
  const next = selectProjectOptimistically([first, second, third], 2);

  assert.deepEqual(next.map((candidate) => candidate.selected), [false, true, false]);
  assert.equal(next[2], third, 'unaffected rows keep their identity');
});

test('every App navigation target applies after synchronous project activation', async () => {
  const app = await readFile(new URL('../src/App.svelte', import.meta.url), 'utf8');
  const resolver = app.slice(
    app.indexOf('async function resolveNavigationRequest'),
    app.indexOf('function navigationProjectId')
  );
  const projectClick = app.slice(
    app.indexOf('function selectProject('),
    app.indexOf('function handleProjectDrop')
  );

  assert.match(resolver, /if \(projectId !== null && !activateProject\(projectId\)\) return;/);
  assert.doesNotMatch(resolver, /await activateProject/);
  assert.doesNotMatch(projectClick, /if \((?:busy|projectReorderBusy)/);
});
