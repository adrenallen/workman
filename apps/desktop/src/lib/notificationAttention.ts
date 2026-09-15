import type { Notification, ProcessView } from './daemon';
import { validParentId } from './agentLineage.ts';

type NotificationTarget = Pick<Notification, 'type' | 'process_id' | 'project_id'>;

/** Shared alert scope; the computer-notification switch only controls OS delivery. */
export function notificationMatchesPreferences(
  notification: NotificationTarget,
  processes: ProcessView[],
  preferences: { mode: 'all' | 'top_level' | 'project_ready'; needsInput: boolean }
): boolean {
  if (notification.type === 'project_ready') {
    return preferences.mode === 'project_ready' && isProjectReady(notification.project_id, processes);
  }
  if (preferences.mode === 'project_ready' && (notification.type === 'agent_done' || notification.type === 'needs_input')) return false;
  if (notification.type === 'needs_input' && !preferences.needsInput) return false;
  return preferences.mode !== 'top_level' || isTopLevelAgentNotification(notification, processes);
}

export function isProjectReady(projectId: number | null, processes: ProcessView[]): boolean {
  const agents = processes.filter((process) => process.kind === 'agent' && process.project_id === projectId);
  return agents.length > 0 && agents.every((process) =>
    process.status !== 'starting' && (
      process.status !== 'running'
      || (!process.agent_state.working && process.agent_state.state !== 'working')
    )
  );
}

/** A selected tab is only read when its terminal is actually in the foreground. */
export function isAgentNotificationViewed(
  processId: number,
  focused: boolean,
  visible: boolean,
  displayedAgentId: number | null
): boolean {
  return focused && visible && displayedAgentId === processId;
}

export function isTopLevelAgentNotification(
  notification: NotificationTarget,
  processes: ProcessView[]
): boolean {
  if (notification.type !== 'agent_done' && notification.type !== 'needs_input') return true;
  const process = processes.find((candidate) => candidate.id === notification.process_id);
  // Missing/deleted processes must not hide a completion that still needs the user.
  if (!process) return true;
  const agents = processes.filter((candidate) => candidate.kind === 'agent' && candidate.project_id === process.project_id);
  return validParentId(process, new Map(agents.map((agent) => [agent.id, agent]))) === null;
}
