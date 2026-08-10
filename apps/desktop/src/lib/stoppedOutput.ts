import type { ProcessStatus, ProcessView } from './daemon';

const STOPPED_OUTPUT_STATUSES: ReadonlySet<ProcessStatus> = new Set([
  'stopped',
  'exited',
  'crashed'
]);

/** Identify one immutable retained-output snapshot across repeated status broadcasts. */
export function stoppedOutputSnapshotKey(
  process: Pick<ProcessView, 'id' | 'status' | 'exited_at'>,
  connected: boolean
): string | null {
  if (!connected || !STOPPED_OUTPUT_STATUSES.has(process.status)) return null;
  return `${process.id}:${process.exited_at ?? 'never-started'}`;
}
