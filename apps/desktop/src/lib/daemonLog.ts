export type DaemonLogTone = 'info' | 'warning' | 'error';

export interface DaemonLogEntry {
  id: number;
  tone: DaemonLogTone;
  title: string;
  detail: string | null;
  occurredAt: number;
  count: number;
}

export type NewDaemonLogEntry = Omit<DaemonLogEntry, 'count'>;

export class DaemonRequestTimeoutError extends Error {
  readonly method: string;
  readonly timeoutMs: number;

  constructor(method: string, timeoutMs: number) {
    super('The daemon did not answer in time');
    this.name = 'DaemonRequestTimeoutError';
    this.method = method;
    this.timeoutMs = timeoutMs;
  }
}

export function isDaemonRequestTimeoutError(cause: unknown): cause is DaemonRequestTimeoutError {
  return cause instanceof DaemonRequestTimeoutError;
}

export function appendDaemonLogEntry(
  entries: DaemonLogEntry[],
  entry: NewDaemonLogEntry,
  limit = 40,
  coalesceWindowMs = 30_000
): DaemonLogEntry[] {
  if (limit <= 0) return [];
  const latest = entries[0];
  if (
    latest
    && latest.tone === entry.tone
    && latest.title === entry.title
    && latest.detail === entry.detail
    && entry.occurredAt - latest.occurredAt <= coalesceWindowMs
  ) {
    return [
      { ...latest, occurredAt: entry.occurredAt, count: latest.count + 1 },
      ...entries.slice(1)
    ];
  }
  return [{ ...entry, count: 1 }, ...entries].slice(0, limit);
}
