import assert from 'node:assert/strict';
import test from 'node:test';

import {
  emptyWorkspaceViewHistory,
  recordWorkspaceView,
  sameWorkspaceView,
  swapWorkspaceViews
} from '../src/lib/workspaceViewHistory.ts';

const overview = (projectId) => ({ projectId, pane: { type: 'overview' } });
const terminal = (projectId, id, label = `Terminal ${id}`) => ({
  projectId,
  pane: {
    type: 'selection',
    selection: { key: `terminal:${id}`, kind: 'terminal', id, projectId, label }
  }
});

test('records the two most recently active workspace views', () => {
  let history = recordWorkspaceView(emptyWorkspaceViewHistory, overview(1));
  history = recordWorkspaceView(history, terminal(1, 4));
  history = recordWorkspaceView(history, overview(2));

  assert.deepEqual(history.current, overview(2));
  assert.deepEqual(history.previous, terminal(1, 4));
});

test('swapping twice toggles between the same project and pane snapshots', () => {
  let history = recordWorkspaceView(emptyWorkspaceViewHistory, terminal(1, 4));
  history = recordWorkspaceView(history, overview(2));

  history = swapWorkspaceViews(history);
  assert.deepEqual(history.current, terminal(1, 4));
  assert.deepEqual(history.previous, overview(2));

  history = swapWorkspaceViews(history);
  assert.deepEqual(history.current, overview(2));
  assert.deepEqual(history.previous, terminal(1, 4));
});

test('label refreshes update the current snapshot without replacing history', () => {
  let history = recordWorkspaceView(emptyWorkspaceViewHistory, overview(1));
  history = recordWorkspaceView(history, terminal(1, 4, 'Old title'));
  history = recordWorkspaceView(history, terminal(1, 4, 'Renamed terminal'));

  assert.equal(history.current.pane.selection.label, 'Renamed terminal');
  assert.deepEqual(history.previous, overview(1));
  assert.equal(sameWorkspaceView(terminal(1, 4, 'A'), terminal(1, 4, 'B')), true);
});

test('the same pane in different projects is a distinct workspace view', () => {
  assert.equal(sameWorkspaceView(overview(1), overview(2)), false);
});
