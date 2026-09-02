import type { ProcessKind } from './daemon';
import type { ProjectTreeItemKind, ProjectTreeSelection } from './projectTree';

export type ProjectPane =
  | { type: 'overview' }
  | { type: 'selection'; selection: ProjectTreeSelection }
  | { type: 'todos' }
  | { type: 'scratchpads' }
  | { type: 'processes'; kind: ProcessKind }
  | { type: 'settings' };

export type ProjectPaneMemory = Record<number, ProjectPane>;

export interface ProjectPaneInventory {
  processIds: ReadonlySet<number>;
  todoIds: ReadonlySet<number>;
  scratchpadIds: ReadonlySet<number>;
  feedbackIds: ReadonlySet<number>;
  draftIds: ReadonlySet<number>;
}

interface PaneStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

const storageKey = 'workman.project-panes.v1';
const selectionKinds = new Set<ProjectTreeItemKind>([
  'todo',
  'agent',
  'terminal',
  'command',
  'scratchpad',
  'feedback',
  'draft'
]);
const processKinds = new Set<ProcessKind>(['agent', 'terminal', 'command']);

export function loadProjectPaneMemory(storage: PaneStorage | null = browserStorage()): ProjectPaneMemory {
  if (!storage) return {};
  try {
    const parsed = JSON.parse(storage.getItem(storageKey) ?? '{}') as unknown;
    if (!isRecord(parsed)) return {};
    const memory: ProjectPaneMemory = {};
    for (const [rawProjectId, rawPane] of Object.entries(parsed)) {
      const projectId = Number(rawProjectId);
      if (!Number.isSafeInteger(projectId) || projectId <= 0) continue;
      const pane = parseProjectPane(rawPane, projectId);
      if (pane) memory[projectId] = pane;
    }
    return memory;
  } catch {
    return {};
  }
}

export function saveProjectPaneMemory(
  memory: ProjectPaneMemory,
  storage: PaneStorage | null = browserStorage()
): void {
  if (!storage) return;
  try {
    storage.setItem(storageKey, JSON.stringify(memory));
  } catch {
    // Navigation remains usable when webview storage is unavailable or full.
  }
}

export function sameProjectPane(left: ProjectPane | undefined, right: ProjectPane): boolean {
  if (!left || left.type !== right.type) return false;
  if (left.type === 'selection' && right.type === 'selection') {
    return left.selection.kind === right.selection.kind
      && left.selection.id === right.selection.id
      && left.selection.projectId === right.selection.projectId
      && left.selection.label === right.selection.label;
  }
  if (left.type === 'processes' && right.type === 'processes') {
    return left.kind === right.kind;
  }
  return true;
}

export function projectPaneSelectionExists(
  pane: ProjectPane,
  inventory: ProjectPaneInventory
): boolean {
  if (pane.type !== 'selection') return true;
  if (
    pane.selection.kind === 'agent'
    || pane.selection.kind === 'terminal'
    || pane.selection.kind === 'command'
  ) return inventory.processIds.has(pane.selection.id);
  if (pane.selection.kind === 'todo') return inventory.todoIds.has(pane.selection.id);
  if (pane.selection.kind === 'draft') return inventory.draftIds.has(pane.selection.id);
  if (pane.selection.kind === 'feedback') return inventory.feedbackIds.has(pane.selection.id);
  return inventory.scratchpadIds.has(pane.selection.id);
}

function parseProjectPane(value: unknown, projectId: number): ProjectPane | null {
  if (!isRecord(value) || typeof value.type !== 'string') return null;
  if (
    value.type === 'overview'
    || value.type === 'todos'
    || value.type === 'scratchpads'
    || value.type === 'settings'
  ) return { type: value.type };
  if (value.type === 'processes' && processKinds.has(value.kind as ProcessKind)) {
    return { type: 'processes', kind: value.kind as ProcessKind };
  }
  if (value.type !== 'selection' || !isRecord(value.selection)) return null;
  const kind = value.selection.kind;
  const id = value.selection.id;
  const label = value.selection.label;
  if (
    typeof kind !== 'string'
    || !selectionKinds.has(kind as ProjectTreeItemKind)
    || !Number.isSafeInteger(id)
    || (kind === 'draft' ? (id as number) >= 0 : (id as number) <= 0)
    || typeof label !== 'string'
  ) return null;
  return {
    type: 'selection',
    selection: {
      key: `${kind}:${id}`,
      kind: kind as ProjectTreeItemKind,
      id: id as number,
      projectId,
      label
    }
  };
}

function browserStorage(): PaneStorage | null {
  return typeof localStorage === 'undefined' ? null : localStorage;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}
