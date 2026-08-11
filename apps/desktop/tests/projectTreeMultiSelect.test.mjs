import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

import {
  bulkFailureMessage,
  updateProjectTreeMultiSelection
} from '../src/lib/projectTreeMultiSelect.ts';

const orderedIds = [10, 20, 30, 40, 50];

function gesture(id, overrides = {}) {
  return {
    group: 'todos',
    id,
    orderedIds,
    anchorId: null,
    toggle: true,
    range: false,
    ...overrides
  };
}

test('command/control toggles build and subtract a display-ordered selection', () => {
  let selection = updateProjectTreeMultiSelection(null, gesture(30));
  selection = updateProjectTreeMultiSelection(selection, gesture(10));
  assert.deepEqual(selection, { group: 'todos', ids: [10, 30] });

  selection = updateProjectTreeMultiSelection(selection, gesture(30));
  assert.deepEqual(selection, { group: 'todos', ids: [10] });
});

test('shift click extends a contiguous range from the last-clicked anchor', () => {
  const selection = updateProjectTreeMultiSelection(null, gesture(40, {
    anchorId: 20,
    toggle: false,
    range: true
  }));
  assert.deepEqual(selection, { group: 'todos', ids: [20, 30, 40] });
});

test('changing groups never combines unlike tree items', () => {
  const todos = updateProjectTreeMultiSelection(null, gesture(10));
  const agents = updateProjectTreeMultiSelection(todos, gesture(30, { group: 'agents' }));
  assert.deepEqual(agents, { group: 'agents', ids: [30] });
});

test('plain clicks clear the modifier selection', () => {
  const selected = updateProjectTreeMultiSelection(null, gesture(10));
  const cleared = updateProjectTreeMultiSelection(selected, gesture(20, {
    toggle: false,
    range: false
  }));
  assert.equal(cleared, null);
});

test('partial failures say what succeeded and identify every failed item', () => {
  assert.equal(
    bulkFailureMessage('were completed', 'complete', 3, [
      { label: '#20 Locked', message: 'todo is locked by another actor' }
    ]),
    '2 of 3 selected items were completed; 1 failed. #20 Locked: todo is locked by another actor'
  );
});

test('every requested tree surface uses the shared modifier gesture and bulk action bar', async () => {
  const source = await readFile(new URL('../src/lib/ProjectTree.svelte', import.meta.url), 'utf8');
  for (const group of ['todos', 'agents', 'terminals', 'scratchpads']) {
    assert.match(source, new RegExp(`selectGroupItem\\(event, '${group}'`));
    assert.match(source, new RegExp(`multiSelected\\('${group}'`));
  }
  for (const action of ['stop', 'close', 'complete', 'archive', 'delete']) {
    assert.match(source, new RegExp(`onBulkAction\\('${action}'\\)`));
  }
  assert.match(source, /event\.ctrlKey/);
  assert.match(source, /event\.metaKey/);
  assert.match(source, /event\.shiftKey/);
});

test('app bulk handlers preserve the existing daemon and coordination action paths', async () => {
  const source = await readFile(new URL('../src/App.svelte', import.meta.url), 'utf8');
  assert.match(source, /client\.stopProcess\(process\.id\)/);
  assert.match(source, /client\.closeProcess\(process\.id\)/);
  assert.match(source, /client\.coordinationTodoComplete\(projectId, todo\.id, true\)/);
  assert.match(source, /'coordination\.todo_delete'/);
  assert.match(source, /'coordination\.scratchpad_archive'/);
  assert.match(source, /'coordination\.scratchpad_delete'/);
  assert.match(source, /bulkFailureMessage/);
  assert.match(source, /selectedDetailWasRemoved/);
  assert.match(source, /await loadTodo\(selection\.id\)/);
});
