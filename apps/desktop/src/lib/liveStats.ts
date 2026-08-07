import { readable, writable, type Readable } from 'svelte/store';

export interface DescendantProcessStats {
  pid: number;
  parent_pid: number | null;
  name: string;
  command: string | null;
  cpu_percent: number;
  memory_bytes: number;
}

export interface ProcessRuntimeStats {
  process_id: number;
  pid: number | null;
  foreground_process_group: number | null;
  foreground_active: boolean;
  cpu_percent: number;
  memory_bytes: number;
  uptime_seconds: number;
  descendant_count: number;
  descendants: DescendantProcessStats[];
}

export interface ProjectRuntimeStats {
  project_id: number;
  memory_bytes: number;
}

export interface ProjectCounts {
  todo_open: number;
  scratchpad_total: number;
  agent_running: number;
  agent_total: number;
  terminal_running: number;
  terminal_total: number;
  command_running: number;
  command_total: number;
}

export interface LiveStatsSnapshot {
  sampled_at: number;
  processes: Record<number, ProcessRuntimeStats>;
  projects: Record<number, ProjectRuntimeStats>;
  counts: Record<number, ProjectCounts>;
}

const emptySnapshot = (): LiveStatsSnapshot => ({
  sampled_at: 0,
  processes: {},
  projects: {},
  counts: {}
});

const mutableLiveStats = writable<LiveStatsSnapshot>(emptySnapshot());

/** The one live telemetry source shared by the project tree and process status bar. */
export const liveStats: Readable<LiveStatsSnapshot> = readable(emptySnapshot(), (set) =>
  mutableLiveStats.subscribe(set)
);

/** Called only by the daemon event adapter. Feature components should read `liveStats`. */
export function updateLiveStats(snapshot: LiveStatsSnapshot): void {
  mutableLiveStats.set(snapshot);
}

export function resetLiveStats(): void {
  mutableLiveStats.set(emptySnapshot());
}
