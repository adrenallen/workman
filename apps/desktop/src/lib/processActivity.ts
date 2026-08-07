import type { ProcessView } from './daemon';
import type { ProcessRuntimeStats } from './liveStats';

export type ProcessActivityState =
  | 'working'
  | 'needs_input'
  | 'waiting'
  | 'idle'
  | 'stopped'
  | 'crashed';

export type ProcessActivityTone = 'success' | 'needs-input' | 'waiting' | 'danger' | 'neutral';

export interface ProcessActivityPresentation {
  state: ProcessActivityState;
  shortLabel: string;
  label: string;
}

export interface ProjectActivityRollup {
  state: ProcessActivityState;
  active: number;
  waiting: number;
  needsInput: number;
  crashed: number;
  idle: number;
  stopped: number;
}

/** Resolve lifecycle and live telemetry into the color-bearing activity state. */
export function processActivity(
  process: ProcessView,
  stats?: ProcessRuntimeStats
): ProcessActivityPresentation {
  if (process.status === 'crashed') {
    return presentation('crashed', 'Crashed', `${process.name} · crashed`);
  }
  if (process.status === 'stopped' || process.status === 'exited') {
    return presentation('stopped', 'Stopped', `${process.name} · ${process.status}`);
  }

  if (process.kind === 'agent') {
    const attention = String(process.agent_state.state);
    if (process.agent_state.needs_input || attention === 'needs_input') {
      return presentation('needs_input', 'Needs input', `${process.name} · needs input`);
    }
    if (attention === 'waiting') {
      return presentation('waiting', 'Waiting', `${process.name} · waiting`);
    }
    if (process.agent_state.working || attention === 'working') {
      return presentation('working', 'Working', `${process.name} · working`);
    }
    return presentation('idle', 'Idle', `${process.name} · idle`);
  }

  if (process.kind === 'terminal') {
    if (stats?.foreground_active === true) {
      return presentation(
        'working',
        'Working',
        `${process.name} · foreground command running`
      );
    }
    const suffix = process.status === 'starting' ? 'starting shell' : 'idle shell';
    return presentation('idle', process.status === 'starting' ? 'Starting' : 'Idle', `${process.name} · ${suffix}`);
  }

  if (process.status === 'running' || process.status === 'starting') {
    return presentation('working', process.status === 'starting' ? 'Starting' : 'Working', `${process.name} · ${process.status}`);
  }
  return presentation('idle', 'Idle', `${process.name} · idle`);
}

export function processActivityTone(state: ProcessActivityState): ProcessActivityTone {
  if (state === 'working') return 'success';
  if (state === 'needs_input') return 'needs-input';
  if (state === 'waiting') return 'waiting';
  if (state === 'crashed') return 'danger';
  return 'neutral';
}

/** Roll up the same activity states used by rows and status bars for a project rail dot. */
export function projectActivityRollup(
  processes: ProcessView[],
  statsByProcess: Record<number, ProcessRuntimeStats | undefined>
): ProjectActivityRollup {
  const rollup: ProjectActivityRollup = {
    state: 'idle',
    active: 0,
    waiting: 0,
    needsInput: 0,
    crashed: 0,
    idle: 0,
    stopped: 0
  };

  for (const process of processes) {
    const state = processActivity(process, statsByProcess[process.id]).state;
    rollup.active += Number(state === 'working');
    rollup.waiting += Number(state === 'waiting');
    rollup.needsInput += Number(state === 'needs_input');
    rollup.crashed += Number(state === 'crashed');
    rollup.idle += Number(state === 'idle');
    rollup.stopped += Number(state === 'stopped');
  }

  // Match the established attention rollup: human input, faults, active work,
  // timer waits, then quiet/dead states.
  rollup.state = rollup.needsInput > 0
    ? 'needs_input'
    : rollup.crashed > 0
      ? 'crashed'
      : rollup.active > 0
        ? 'working'
        : rollup.waiting > 0
          ? 'waiting'
          : 'idle';
  return rollup;
}

function presentation(
  state: ProcessActivityState,
  shortLabel: string,
  label: string
): ProcessActivityPresentation {
  return { state, shortLabel, label };
}
