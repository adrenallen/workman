import type { ProcessView } from './daemon';

export interface AgentAttentionRollup {
  total: number;
  needsInput: number;
  working: number;
  waiting: number;
  unread: number;
  crashed: number;
}

export interface AgentLineageRow {
  process: ProcessView;
  depth: number;
  rollup: AgentAttentionRollup;
}

/**
 * Project one flat process list into stable depth-first agent rows.
 *
 * Missing/non-agent parents are deliberately treated as roots, so a child is promoted as soon as
 * its parent closes or falls outside the selected project. Cycles are also promoted instead of
 * disappearing from the UI.
 */
export function agentLineageRows(agents: ProcessView[], query: string): AgentLineageRow[] {
  const byId = new Map(agents.map((agent) => [agent.id, agent]));
  const parentById = new Map<number, number>();
  for (const agent of agents) {
    const parentId = validParentId(agent, byId);
    if (parentId !== null) parentById.set(agent.id, parentId);
  }

  const childrenById = new Map<number, ProcessView[]>();
  for (const agent of agents) {
    const parentId = parentById.get(agent.id);
    if (parentId === undefined) continue;
    const children = childrenById.get(parentId) ?? [];
    children.push(agent);
    childrenById.set(parentId, children);
  }

  const needle = query.trim().toLowerCase();
  const included = new Set<number>();
  for (const agent of agents) {
    if (!needle || agent.name.toLowerCase().includes(needle)) {
      included.add(agent.id);
      let parentId = parentById.get(agent.id);
      while (parentId !== undefined && !included.has(parentId)) {
        included.add(parentId);
        parentId = parentById.get(parentId);
      }
    }
  }

  const rows: AgentLineageRow[] = [];
  const visited = new Set<number>();
  const append = (agent: ProcessView, depth: number) => {
    if (visited.has(agent.id) || !included.has(agent.id)) return;
    visited.add(agent.id);
    rows.push({
      process: agent,
      depth,
      rollup: attentionRollup(agent.id, childrenById)
    });
    for (const child of childrenById.get(agent.id) ?? []) append(child, depth + 1);
  };

  for (const agent of agents) {
    if (!parentById.has(agent.id)) append(agent, 0);
  }
  // Defensive promotion for malformed cycles or records changed while the stream was sampled.
  for (const agent of agents) append(agent, 0);
  return rows;
}

export function validParentId(agent: ProcessView, byId: Map<number, ProcessView>): number | null {
  const parentId = agent.spawned_by_process_id;
  if (parentId === null || parentId === agent.id || !byId.has(parentId)) return null;

  const seen = new Set([agent.id]);
  let cursor: number | null = parentId;
  while (cursor !== null) {
    if (seen.has(cursor)) return null;
    seen.add(cursor);
    cursor = byId.get(cursor)?.spawned_by_process_id ?? null;
    if (cursor !== null && !byId.has(cursor)) break;
  }
  return parentId;
}

function attentionRollup(
  processId: number,
  childrenById: Map<number, ProcessView[]>
): AgentAttentionRollup {
  const rollup: AgentAttentionRollup = {
    total: 0,
    needsInput: 0,
    working: 0,
    waiting: 0,
    unread: 0,
    crashed: 0
  };
  const pending = [...(childrenById.get(processId) ?? [])];
  const visited = new Set<number>();
  while (pending.length > 0) {
    const child = pending.shift();
    if (!child || visited.has(child.id)) continue;
    visited.add(child.id);
    rollup.total += 1;
    rollup.needsInput += Number(child.agent_state.needs_input);
    rollup.working += Number(child.agent_state.working);
    rollup.waiting += Number(String(child.agent_state.state) === 'waiting');
    rollup.unread += Number(child.agent_state.unread === true);
    rollup.crashed += Number(child.status === 'crashed');
    pending.push(...(childrenById.get(child.id) ?? []));
  }
  return rollup;
}
