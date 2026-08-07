import type { TodoSummary } from './coordination';
import type { ProcessView } from './daemon';

type ClaimableTodo = Pick<TodoSummary, 'id' | 'project_id' | 'locked_by'>;

export interface TodoClaimant {
  actorId: string;
  name: string;
  process: ProcessView | null;
}

/**
 * Resolve a todo lease through the same process-linked claim list used by the
 * agent terminal overlay. Actors without a Workman process remain visible by
 * their actor identifier, but deliberately have no navigation target.
 */
export function resolveTodoClaimant(
  todo: ClaimableTodo,
  processes: ProcessView[]
): TodoClaimant | null {
  if (!todo.locked_by) return null;
  const process = processes.find((candidate) =>
    candidate.claimed_todos?.some(
      (claim) => claim.id === todo.id && claim.project_id === todo.project_id
    )
  ) ?? null;
  return {
    actorId: todo.locked_by,
    name: process?.name ?? todo.locked_by,
    process
  };
}
