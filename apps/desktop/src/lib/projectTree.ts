export type ProjectTreeItemKind =
  | 'todo'
  | 'agent'
  | 'terminal'
  | 'command'
  | 'scratchpad'
  | 'feedback'
  | 'draft';

export type ProjectTreeGroup =
  | 'todos'
  | 'agents'
  | 'terminals'
  | 'commands'
  | 'feedback'
  | 'scratchpads';

export const projectTreeGroupOrderStorageKey = 'workman.tree.group-order.v1';

export const defaultProjectTreeGroupOrder: readonly ProjectTreeGroup[] = [
  'todos',
  'agents',
  'terminals',
  'commands',
  'scratchpads',
  'feedback'
];

/** Keep every known group exactly once while accepting older saved layouts. */
export function normalizeProjectTreeGroupOrder(value: unknown): ProjectTreeGroup[] {
  const known = new Set<ProjectTreeGroup>(defaultProjectTreeGroupOrder);
  const saved = Array.isArray(value)
    ? value.filter((group): group is ProjectTreeGroup => (
        typeof group === 'string'
        && known.has(group as ProjectTreeGroup)
      ))
    : [];
  const unique = [...new Set(saved)];
  return [
    ...unique,
    ...defaultProjectTreeGroupOrder.filter((group) => !unique.includes(group))
  ];
}

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
