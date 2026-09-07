import assert from 'node:assert/strict';
import test from 'node:test';

import {
  emptyWorkspaceViewHistory,
  recordWorkspaceView,
  recentWorkspaceViews,
  recentViewLifetime,
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

test('recent views are unique, newest first, and capped at ten including current', () => {
  let history = emptyWorkspaceViewHistory;
  for (let id = 1; id <= 15; id++) history = recordWorkspaceView(history, terminal(1, id), id);
  history = recordWorkspaceView(history, terminal(1, 10), 16);
  assert.deepEqual(recentWorkspaceViews(history, () => true, 16).map(view => view.pane.selection.id),
    [10, 15, 14, 13, 12, 11, 9, 8, 7, 6]);
});

test('expired and closed views are excluded; a long stay refreshes the view being left', () => {
  let history = recordWorkspaceView(emptyWorkspaceViewHistory, terminal(1, 1), 0);
  history = recordWorkspaceView(history, terminal(1, 2), 1);
  history = recordWorkspaceView(history, terminal(1, 3), 2);
  assert.deepEqual(recentWorkspaceViews(history, view => view.pane.selection.id !== 2, 3),
    [terminal(1, 3), terminal(1, 1)]);
  history = recordWorkspaceView(history, overview(1), recentViewLifetime + 3);
  assert.deepEqual(recentWorkspaceViews(history, () => true, recentViewLifetime + 3),
    [overview(1), terminal(1, 3)]);
  assert.deepEqual(recentWorkspaceViews(history, () => true, 2 * recentViewLifetime + 3), [overview(1)]);
});

test('a switcher snapshot keeps its order until a choice is committed', () => {
  let history = recordWorkspaceView(emptyWorkspaceViewHistory, overview(1), 0);
  history = recordWorkspaceView(history, terminal(1, 2), 1);
  history = recordWorkspaceView(history, terminal(1, 3), 2);
  const snapshot = recentWorkspaceViews(history, () => true, 3);
  history = recordWorkspaceView(history, snapshot[2], 4);
  assert.deepEqual(snapshot, [terminal(1, 3), terminal(1, 2), overview(1)]);
  assert.deepEqual(recentWorkspaceViews(history, () => true, 4), [overview(1), terminal(1, 3), terminal(1, 2)]);
});
