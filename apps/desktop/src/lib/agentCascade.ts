import type { ProcessView } from './daemon';

export type AgentCascadeAction = 'stop' | 'kill' | 'close';

export interface AgentCascadeRequest {
  processes: ProcessView[];
  actionRoots: ProcessView[];
  action: AgentCascadeAction;
  descendants: ProcessView[];
}

export interface AgentCascadePlan {
  selected: ProcessView[];
  actionRoots: ProcessView[];
  additionalDescendants: ProcessView[];
}

function isLive(process: ProcessView): boolean {
  return process.status === 'starting' || process.status === 'running';
}

/** Return live descendant agents in stable parent-before-child display order. */
export function liveAgentDescendants(
  processes: ProcessView[],
  parentId: number
): ProcessView[] {
  const children = new Map<number, ProcessView[]>();
  for (const process of processes) {
    if (process.kind !== 'agent' || process.spawned_by_process_id === null) continue;
    const siblings = children.get(process.spawned_by_process_id) ?? [];
    siblings.push(process);
    children.set(process.spawned_by_process_id, siblings);
  }

  const descendants: ProcessView[] = [];
  const visited = new Set<number>([parentId]);
  const visit = (processId: number): void => {
    for (const child of children.get(processId) ?? []) {
      if (visited.has(child.id)) continue;
      visited.add(child.id);
      if (isLive(child)) descendants.push(child);
      visit(child.id);
    }
  };
  visit(parentId);
  return descendants;
}

/** Collapse selected parent/child agents to safe daemon action roots and count extra impact once. */
export function planAgentCascade(
  processes: ProcessView[],
  selectedProcesses: ProcessView[],
  includeStoppedDescendants = false
): AgentCascadePlan {
  const selected = [...new Map(selectedProcesses.map((process) => [process.id, process])).values()];
  const selectedIds = new Set(selected.map((process) => process.id));
  const parentById = new Map(
    processes
      .filter((process) => process.kind === 'agent')
      .map((process) => [process.id, process.spawned_by_process_id])
  );

  const hasSelectedAncestor = (process: ProcessView): boolean => {
    const visited = new Set<number>([process.id]);
    let parentId = process.spawned_by_process_id;
    while (parentId !== null && !visited.has(parentId)) {
      if (selectedIds.has(parentId)) return true;
      visited.add(parentId);
      parentId = parentById.get(parentId) ?? null;
    }
    return false;
  };

  const actionRoots = selected.filter(
    (process) => process.kind !== 'agent' || !hasSelectedAncestor(process)
  );
  const additional = new Map<number, ProcessView>();
  for (const root of actionRoots) {
    if (root.kind !== 'agent') continue;
    const descendants = includeStoppedDescendants
      ? allAgentDescendants(processes, root.id)
      : liveAgentDescendants(processes, root.id);
    for (const descendant of descendants) {
      if (!selectedIds.has(descendant.id)) additional.set(descendant.id, descendant);
    }
  }
  return {
    selected,
    actionRoots,
    additionalDescendants: [...additional.values()]
  };
}

function allAgentDescendants(processes: ProcessView[], parentId: number): ProcessView[] {
  const children = new Map<number, ProcessView[]>();
  for (const process of processes) {
    if (process.kind !== 'agent' || process.spawned_by_process_id === null) continue;
    const siblings = children.get(process.spawned_by_process_id) ?? [];
    siblings.push(process);
    children.set(process.spawned_by_process_id, siblings);
  }
  const descendants: ProcessView[] = [];
  const visited = new Set<number>([parentId]);
  const visit = (processId: number): void => {
    for (const child of children.get(processId) ?? []) {
      if (visited.has(child.id)) continue;
      visited.add(child.id);
      descendants.push(child);
      visit(child.id);
    }
  };
  visit(parentId);
  return descendants;
}
