import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

import {
  loadProjectPaneMemory,
  projectPaneSelectionExists,
  sameProjectPane,
  saveProjectPaneMemory
} from '../src/lib/projectPaneMemory.ts';
import { projectTreeSelection } from '../src/lib/projectTree.ts';

function storage(initial = {}) {
  const values = new Map(Object.entries(initial));
  return {
    getItem: (key) => values.get(key) ?? null,
    setItem: (key, value) => values.set(key, value),
    value: (key) => values.get(key)
  };
}

test('persists independent pane selections and rebuilds trusted project keys', () => {
  const target = storage();
  const memory = {
    7: {
      type: 'selection',
      selection: projectTreeSelection('agent', 41, 7, 'builder')
    },
    9: { type: 'processes', kind: 'terminal' },
    11: { type: 'overview' }
  };

  saveProjectPaneMemory(memory, target);
  const loaded = loadProjectPaneMemory(target);

  assert.deepEqual(loaded, memory);
  assert.equal(loaded[7].selection.key, 'agent:41');
  assert.equal(loaded[7].selection.projectId, 7);
});

test('drops malformed and transient selections without losing valid panes', () => {
  const target = storage({
    'workman.project-panes.v1': JSON.stringify({
      1: { type: 'selection', selection: { kind: 'agent', id: -1, label: 'starting' } },
      2: { type: 'processes', kind: 'unknown' },
      3: { type: 'scratchpads' },
      4: { type: 'feedback' },
      nope: { type: 'settings' }
    })
  });

  assert.deepEqual(loadProjectPaneMemory(target), {
    3: { type: 'scratchpads' },
    4: { type: 'feedback' }
  });
});

test('persists negative draft selections while rejecting transient negative processes', () => {
  const target = storage({
    'workman.project-panes.v1': JSON.stringify({
      1: { type: 'selection', selection: { kind: 'draft', id: -4, label: 'New agent' } },
      2: { type: 'selection', selection: { kind: 'agent', id: -1, label: 'starting' } }
    })
  });
  assert.deepEqual(loadProjectPaneMemory(target), {
    1: {
      type: 'selection',
      selection: projectTreeSelection('draft', -4, 1, 'New agent')
    }
  });
});

test('stale remembered items fail closed to the overview inventory path', () => {
  const pane = {
    type: 'selection',
    selection: projectTreeSelection('terminal', 22, 5, 'shell')
  };
  const missing = {
    processIds: new Set(), todoIds: new Set(), scratchpadIds: new Set(), draftIds: new Set()
  };
  const present = { ...missing, processIds: new Set([22]) };

  assert.equal(projectPaneSelectionExists(pane, missing), false);
  assert.equal(projectPaneSelectionExists(pane, present), true);
  assert.equal(projectPaneSelectionExists({ type: 'overview' }, missing), true);
  assert.equal(projectPaneSelectionExists({
    type: 'selection',
    selection: projectTreeSelection('draft', -3, 5, 'New command')
  }, { ...missing, draftIds: new Set([-3]) }), true);
});

test('pane equality notices selection labels but ignores object identity', () => {
  const first = {
    type: 'selection',
    selection: projectTreeSelection('todo', 8, 3, 'Ship it')
  };
  assert.equal(sameProjectPane(first, {
    type: 'selection',
    selection: projectTreeSelection('todo', 8, 3, 'Ship it')
  }), true);
  assert.equal(sameProjectPane(first, {
    type: 'selection',
    selection: projectTreeSelection('todo', 8, 3, 'Renamed')
  }), false);
});

test('project switches restore memory inside the optimistic activation path', async () => {
  const app = await readFile(new URL('../src/App.svelte', import.meta.url), 'utf8');
  const activation = app.slice(
    app.indexOf('function applyOptimisticProjectActivation'),
    app.indexOf('async function hydrateProjectActivation')
  );
  const projectCase = app.slice(
    app.indexOf("case 'project':"),
    app.indexOf("case 'item':")
  );

  assert.match(activation, /applyProjectActivationState\(projectId\)/);
  assert.match(activation, /applyRememberedProjectPane\(projectId\)/);
  assert.match(projectCase, /if \(!switchingProjects\)/);
  assert.doesNotMatch(projectCase, /await/);
});
