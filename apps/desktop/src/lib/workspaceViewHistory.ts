import type { ProjectPane } from './projectPaneMemory';

export interface WorkspaceViewState {
  projectId: number;
  pane: ProjectPane;
}

export interface RecentWorkspaceView {
  view: WorkspaceViewState;
  visitedAt: number;
}

export const recentViewLifetime = 10 * 60 * 1000;
export const recentViewLimit = 10;

export interface WorkspaceViewHistory {
  current: WorkspaceViewState | null;
  previous: WorkspaceViewState | null;
  recent: RecentWorkspaceView[];
}

export const emptyWorkspaceViewHistory: WorkspaceViewHistory = {
  current: null,
  previous: null,
  recent: []
};

export function recordWorkspaceView(
  history: WorkspaceViewHistory,
  next: WorkspaceViewState,
  now = Date.now()
): WorkspaceViewHistory {
  if (sameWorkspaceView(history.current, next)) {
    return sameWorkspaceViewSnapshot(history.current!, next)
      ? history
      : { ...history, current: cloneWorkspaceView(next) };
  }
  // A long stay still counts as recent when leaving the current view.
  const recent = history.current
    ? [{ view: cloneWorkspaceView(history.current), visitedAt: now }, ...history.recent]
    : history.recent;
  const unique = recent.filter((entry, index) => !sameWorkspaceView(entry.view, next)
    && now - entry.visitedAt < recentViewLifetime
    && recent.findIndex(other => sameWorkspaceView(other.view, entry.view)) === index);
  return {
    current: cloneWorkspaceView(next),
    previous: unique[0]?.view ?? null,
    recent: unique.slice(0, recentViewLimit - 1)
  };
}

export function recentWorkspaceViews(
  history: WorkspaceViewHistory,
  available: (view: WorkspaceViewState) => boolean,
  now = Date.now()
): WorkspaceViewState[] {
  return [
    ...(history.current ? [history.current] : []),
    ...history.recent.filter(entry => now - entry.visitedAt < recentViewLifetime).map(entry => entry.view)
  ].filter(available).slice(0, recentViewLimit).map(cloneWorkspaceView);
}

export function swapWorkspaceViews(history: WorkspaceViewHistory): WorkspaceViewHistory {
  return history.previous ? recordWorkspaceView(history, history.previous) : history;
}

export function sameWorkspaceView(
  left: WorkspaceViewState | null,
  right: WorkspaceViewState | null
): boolean {
  if (!left || !right || left.projectId !== right.projectId) return left === right;
  if (left.pane.type !== right.pane.type) return false;
  if (left.pane.type === 'selection' && right.pane.type === 'selection') {
    return left.pane.selection.kind === right.pane.selection.kind
      && left.pane.selection.id === right.pane.selection.id;
  }
  if (left.pane.type === 'processes' && right.pane.type === 'processes') {
    return left.pane.kind === right.pane.kind;
  }
  return true;
}

function sameWorkspaceViewSnapshot(left: WorkspaceViewState, right: WorkspaceViewState): boolean {
  if (!sameWorkspaceView(left, right)) return false;
  if (left.pane.type === 'selection' && right.pane.type === 'selection') {
    return left.pane.selection.key === right.pane.selection.key
      && left.pane.selection.projectId === right.pane.selection.projectId
      && left.pane.selection.label === right.pane.selection.label;
  }
  return true;
}

function cloneWorkspaceView(view: WorkspaceViewState): WorkspaceViewState {
  return {
    projectId: view.projectId,
    pane: view.pane.type === 'selection'
      ? { type: 'selection', selection: { ...view.pane.selection } }
      : { ...view.pane }
  };
}
