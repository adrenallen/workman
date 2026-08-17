export type ProjectTreeItemKind =
  | 'todo'
  | 'agent'
  | 'terminal'
  | 'command'
  | 'scratchpad'
  | 'draft';

export type ProjectTreeGroup =
  | 'todos'
  | 'agents'
  | 'terminals'
  | 'commands'
  | 'scratchpads';

export interface ProjectTreeSelection {
  key: string;
  kind: ProjectTreeItemKind;
  id: number;
  projectId: number;
  label: string;
}

export function projectTreeKey(kind: ProjectTreeItemKind, id: number): string {
  return `${kind}:${id}`;
}

export function projectTreeSelection(
  kind: ProjectTreeItemKind,
  id: number,
  projectId: number,
  label: string
): ProjectTreeSelection {
  return { key: projectTreeKey(kind, id), kind, id, projectId, label };
}

export function isProcessSelection(selection: ProjectTreeSelection | null): boolean {
  return (
    selection?.kind === 'agent' ||
    selection?.kind === 'terminal' ||
    selection?.kind === 'command'
  );
}
