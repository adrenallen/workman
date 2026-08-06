import type { TodoSummary } from './coordination';

export type TodoClaimState = 'open' | 'claimed' | 'blocked' | 'completed';
type TodoClaimFields = Pick<
  TodoSummary,
  'completed' | 'status' | 'is_blocked' | 'locked_by' | 'unresolved_blocker_count'
>;

export function todoClaimState(todo: TodoClaimFields): TodoClaimState {
  if (todo.completed || todo.status === 'completed') return 'completed';
  if (todo.is_blocked) return 'blocked';
  if (todo.locked_by || todo.status === 'in_progress') return 'claimed';
  return 'open';
}

export function todoClaimLabel(todo: TodoClaimFields): string {
  switch (todoClaimState(todo)) {
    case 'completed':
      return 'Completed';
    case 'blocked':
      return `Blocked · ${todo.unresolved_blocker_count} unresolved blocker${todo.unresolved_blocker_count === 1 ? '' : 's'}`;
    case 'claimed':
      return todo.locked_by ? `Claimed by ${todo.locked_by}` : 'In progress';
    default:
      return 'Open · unclaimed';
  }
}

export function shortTodoActor(actor: string): string {
  const parts = actor.split('-');
  return parts.length > 2 ? `${parts[0]}-${parts.at(-1)}` : actor;
}
