import type { TodoSummary } from './coordination';

export type TodoClaimState = 'open' | 'claimed' | 'blocked' | 'completed';

export function todoClaimState(todo: TodoSummary): TodoClaimState {
  if (todo.completed || todo.status === 'completed') return 'completed';
  if (todo.is_blocked) return 'blocked';
  if (todo.locked_by) return 'claimed';
  return 'open';
}

export function todoClaimLabel(todo: TodoSummary): string {
  switch (todoClaimState(todo)) {
    case 'completed':
      return 'Completed';
    case 'blocked':
      return `Blocked · ${todo.unresolved_blocker_count} unresolved blocker${todo.unresolved_blocker_count === 1 ? '' : 's'}`;
    case 'claimed':
      return `Claimed by ${todo.locked_by}`;
    default:
      return 'Open · unclaimed';
  }
}

export function todoClaimTone(todo: TodoSummary): 'success' | 'warning' | 'danger' | 'neutral' {
  switch (todoClaimState(todo)) {
    case 'blocked': return 'danger';
    case 'claimed': return 'warning';
    case 'open': return 'success';
    default: return 'neutral';
  }
}

export function shortTodoActor(actor: string): string {
  const parts = actor.split('-');
  return parts.length > 2 ? `${parts[0]}-${parts.at(-1)}` : actor;
}
