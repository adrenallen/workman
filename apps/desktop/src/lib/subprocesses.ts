import type { DaemonClient } from './daemon';
import type { DescendantProcessStats } from './liveStats';

export interface SubprocessList {
  process_id: number;
  root_pid: number;
  subprocesses: DescendantProcessStats[];
}

export interface KillSubprocessResult {
  process_id: number;
  pid: number;
  signal: 'term' | 'kill';
  delivered: boolean;
}

export function listSubprocesses(
  client: DaemonClient,
  processId: number
): Promise<SubprocessList> {
  return client.control<SubprocessList>('process.subprocesses', { process_id: processId });
}

export function killSubprocess(
  client: DaemonClient,
  processId: number,
  pid: number,
  force = false
): Promise<KillSubprocessResult> {
  return client.control<KillSubprocessResult>('process.kill_subprocess', {
    process_id: processId,
    pid,
    force
  });
}
