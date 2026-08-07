import type { ProcessView } from './daemon';

export type AgentCascadeAction = 'stop' | 'kill' | 'close';

export interface AgentCascadeRequest {
  process: ProcessView;
  action: AgentCascadeAction;
  descendants: ProcessView[];
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
