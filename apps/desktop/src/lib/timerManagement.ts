import type { DaemonClient } from './daemon';
import type { TimerView } from './timerLifecycle';

export interface TimerEditInput {
  body?: string;
  due_at?: number;
  delay_ms?: number;
  interval_ms?: number;
  paused?: boolean;
}

export async function listProjectTimers(
  client: DaemonClient,
  projectId: number
): Promise<TimerView[]> {
  const result = await client.control<{ project_id: number; timers: TimerView[] }>('timer.list', {
    project_id: projectId
  });
  return result.timers;
}

export async function updateProjectTimer(
  client: DaemonClient,
  projectId: number,
  timerId: number,
  edit: TimerEditInput
): Promise<TimerView> {
  const result = await client.control<{ project_id: number; timer: TimerView }>('timer.update', {
    project_id: projectId,
    timer_id: timerId,
    ...edit
  });
  return result.timer;
}

export async function deleteProjectTimer(
  client: DaemonClient,
  projectId: number,
  timerId: number
): Promise<void> {
  await client.control('timer.delete', { project_id: projectId, timer_id: timerId });
}

export function timerKindLabel(timer: TimerView): string {
  if (timer.kind === 'idle_any') return 'Idle · any';
  if (timer.kind === 'idle_all') return 'Idle · all';
  return timer.repeating ? 'Recurring' : 'One-shot';
}

export function timerStateLabel(timer: TimerView): string {
  if (timer.fired) return 'Fired';
  if (timer.paused) return 'Paused';
  if (timer.kind !== 'delay') return 'Watching';
  return 'Scheduled';
}
