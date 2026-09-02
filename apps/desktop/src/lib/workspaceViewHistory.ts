import type { ProjectPane } from './projectPaneMemory';

export interface WorkspaceViewState {
  projectId: number;
  pane: ProjectPane;
}

export interface WorkspaceViewHistory {
  current: WorkspaceViewState | null;
  previous: WorkspaceViewState | null;
}

export const emptyWorkspaceViewHistory: WorkspaceViewHistory = {
  current: null,
  previous: null
};

export function recordWorkspaceView(
  history: WorkspaceViewHistory,
  next: WorkspaceViewState
): WorkspaceViewHistory {
  if (!history.current) {
    return { current: cloneWorkspaceView(next), previous: null };
  }
  if (sameWorkspaceView(history.current, next)) {
    return sameWorkspaceViewSnapshot(history.current, next)
      ? history
      : { ...history, current: cloneWorkspaceView(next) };
  }
  return {
    current: cloneWorkspaceView(next),
    previous: cloneWorkspaceView(history.current)
  };
}

export function swapWorkspaceViews(history: WorkspaceViewHistory): WorkspaceViewHistory {
  if (!history.current || !history.previous) return history;
  return {
    current: cloneWorkspaceView(history.previous),
    previous: cloneWorkspaceView(history.current)
  };
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
