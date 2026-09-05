import type { Notification, ProcessView } from './daemon';
import { validParentId } from './agentLineage.ts';

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
  notification: Notification,
  processes: ProcessView[]
): boolean {
  if (notification.type !== 'agent_done' && notification.type !== 'needs_input') return true;
  const process = processes.find((candidate) => candidate.id === notification.process_id);
  // Missing/deleted processes must not hide a completion that still needs the user.
  if (!process) return true;
  const agents = processes.filter((candidate) => candidate.kind === 'agent' && candidate.project_id === process.project_id);
  return validParentId(process, new Map(agents.map((agent) => [agent.id, agent]))) === null;
}
