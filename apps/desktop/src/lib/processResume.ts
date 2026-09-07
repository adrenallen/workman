import type { ProcessView } from './daemon';
import { isTerminalInputTarget, isTextEditingTarget } from './keyboardNavigation.ts';

export function canResumeProcess(process: Pick<ProcessView, 'status'>): boolean {
  return process.status === 'stopped' || process.status === 'exited' || process.status === 'crashed';
}

/** Resolve actual keyboard focus, never a stale selection hidden behind another pane. */
export function focusedResumeProcess(target: EventTarget | null, processes: readonly ProcessView[]): ProcessView | null {
  if (!(target instanceof HTMLElement)) return null;
  if (isTextEditingTarget(target) && !isTerminalInputTarget(target)) return null;
  if (target.closest('[role="dialog"], [role="alertdialog"], [role="menu"]')) return null;
  const owner = target.closest<HTMLElement>('[data-resume-process-id], [data-context-kind][data-context-id]');
  if (!owner) return null;
  if (!owner.dataset.resumeProcessId && !['agent', 'terminal', 'command'].includes(owner.dataset.contextKind ?? '')) return null;
  const id = Number(owner.dataset.resumeProcessId ?? owner.dataset.contextId);
  const process = processes.find(process => process.id === id);
  return process && canResumeProcess(process) ? process : null;
}
