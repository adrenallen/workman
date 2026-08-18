import type { ProcessKind, ProcessView } from './daemon';
import type { ProcessRuntimeStats } from './liveStats';

export type ProcessActivityState =
  | 'working'
  | 'needs_input'
  | 'waiting'
  | 'idle'
  | 'stopped'
  | 'crashed';

export type ProcessActivityTone = 'success' | 'needs-input' | 'waiting' | 'danger' | 'neutral';
export type ProjectKindActivityTone = ProcessActivityTone | 'idle';

export interface ProcessActivityPresentation {
  state: ProcessActivityState;
  shortLabel: string;
  label: string;
}

export type ProcessActivityRuntimeStats = Pick<ProcessRuntimeStats, 'foreground_active'>;

export interface ProjectActivityRollup {
  state: ProcessActivityState;
  active: number;
  waiting: number;
  needsInput: number;
  crashed: number;
  idle: number;
  stopped: number;
}

export interface ProjectKindActivityDetail {
  tone: ProjectKindActivityTone;
  active: number;
  running: number;
  starting: number;
  needsInput: number;
  crashed: number;
  waiting: number;
  idle: number;
  stopped: number;
  total: number;
  label: string;
  activeLabel: string;
  activeProcessIds: number[];
  processIds: number[];
  targetProcessId: number | null;
}

export type ProjectKindActivityRollup = Record<ProcessKind, ProjectKindActivityDetail>;

type KindActivityState =
  | 'running'
  | 'starting'
  | 'needs_input'
  | 'crashed'
  | 'waiting'
  | 'idle'
  | 'stopped';

interface KindTargetCandidate {
  id: number;
  priority: number;
  recency: number;
}

/** Resolve lifecycle and live telemetry into the color-bearing activity state. */
export function processActivity(
  process: ProcessView,
  stats?: ProcessActivityRuntimeStats
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

/**
 * Summarize project activity without losing which kind owns it.
 *
 * `running` means an agent is actively working, a terminal has an active
 * foreground process, or a command is live. Agent input requests outrank active
 * work. Agent targets then prefer the newest output/content activity; runtime-
 * only kinds use the most recently started process because live stats do not
 * expose a last-activity timestamp. Process id is the final deterministic tie-
 * break.
 * Inactive errors remain available to summaries, but a live indicator keeps its
 * running tone instead of being recolored by a process that has already exited.
 */
export function projectKindActivity(
  processes: ProcessView[],
  statsByProcess: Record<number, ProcessRuntimeStats | undefined>
): ProjectKindActivityRollup {
  const rollup: ProjectKindActivityRollup = {
    agent: emptyKindActivity('agent'),
    terminal: emptyKindActivity('terminal'),
    command: emptyKindActivity('command')
  };
  const targets: Record<ProcessKind, KindTargetCandidate | null> = {
    agent: null,
    terminal: null,
    command: null
  };
  const candidates: Record<ProcessKind, KindTargetCandidate[]> = {
    agent: [],
    terminal: [],
    command: []
  };
  const rosters: Record<ProcessKind, KindTargetCandidate[]> = {
    agent: [],
    terminal: [],
    command: []
  };

  for (const process of processes) {
    const detail = rollup[process.kind];
    const stats = statsByProcess[process.id];
    const state = processKindActivityState(process, stats);
    detail.total += 1;
    rosters[process.kind].push(rosterCandidate(process, state, stats));

    if (state === 'running') detail.running += 1;
    if (state === 'starting') detail.starting += 1;
    if (state === 'needs_input') detail.needsInput += 1;
    if (state === 'crashed') detail.crashed += 1;
    if (state === 'waiting') detail.waiting += 1;
    if (state === 'idle') detail.idle += 1;
    if (state === 'stopped') detail.stopped += 1;

    const candidate = targetCandidate(process, state, stats);
    if (candidate) {
      candidates[process.kind].push(candidate);
      if (isBetterTarget(candidate, targets[process.kind])) {
        targets[process.kind] = candidate;
      }
    }
  }

  for (const kind of ['agent', 'terminal', 'command'] as const) {
    const detail = rollup[kind];
    detail.active = detail.running
      + (kind === 'terminal' ? 0 : detail.starting)
      + (kind === 'agent' ? detail.needsInput : 0);
    detail.tone = kindActivityTone(detail);
    detail.label = kindActivityLabel(kind, detail);
    detail.activeProcessIds = candidates[kind]
      .sort(compareTargets)
      .map((candidate) => candidate.id);
    detail.processIds = rosters[kind]
      .sort(compareTargets)
      .map((candidate) => candidate.id);
    detail.activeLabel = kindActiveLabel(kind, detail, processes);
    detail.targetProcessId = targets[kind]?.id ?? null;
  }

  return rollup;
}

function emptyKindActivity(kind: ProcessKind): ProjectKindActivityDetail {
  return {
    tone: 'neutral',
    active: 0,
    running: 0,
    starting: 0,
    needsInput: 0,
    crashed: 0,
    waiting: 0,
    idle: 0,
    stopped: 0,
    total: 0,
    label: `no ${kindPlural(kind)}`,
    activeLabel: `no ${kindPlural(kind)} running`,
    activeProcessIds: [],
    processIds: [],
    targetProcessId: null
  };
}

function processKindActivityState(
  process: ProcessView,
  stats: ProcessRuntimeStats | undefined
): KindActivityState {
  if (processHasError(process)) return 'crashed';
  if (
    process.status === 'stopped'
    || process.status === 'exited'
    || (process.kind === 'agent' && process.agent_state.exited)
  ) {
    return 'stopped';
  }

  if (process.kind === 'agent') {
    const attention = String(process.agent_state.state);
    if (process.agent_state.needs_input || attention === 'needs_input') return 'needs_input';
    if (attention === 'waiting') return 'waiting';
    if (process.agent_state.working || attention === 'working') return 'running';
    if (process.status === 'starting') return 'starting';
    return 'idle';
  }

  if (process.kind === 'terminal') {
    if (process.status === 'starting') return 'starting';
    return process.status === 'running' && stats?.foreground_active === true
      ? 'running'
      : 'idle';
  }

  if (process.status === 'starting') return 'starting';
  if (process.status === 'running') return 'running';
  return 'idle';
}

function processHasError(process: ProcessView): boolean {
  if (process.status === 'crashed') return true;
  const exited = process.status === 'exited'
    || (process.kind === 'agent' && process.agent_state.exited);
  return exited
    && (process.exit_signal != null || (process.exit_code != null && process.exit_code !== 0));
}

function kindActivityTone(detail: ProjectKindActivityDetail): ProjectKindActivityTone {
  if (detail.needsInput > 0) return 'needs-input';
  if (detail.running > 0 || detail.starting > 0) return 'success';
  if (detail.crashed > 0) return 'danger';
  return 'idle';
}

function kindActiveLabel(
  kind: ProcessKind,
  detail: ProjectKindActivityDetail,
  processes: ProcessView[]
): string {
  if (detail.active === 0) return `no ${kindPlural(kind)} running`;
  if (kind === 'terminal') {
    return `${detail.active} ${detail.active === 1 ? 'terminal' : 'terminals'} live`;
  }
  if (kind === 'command') {
    if (detail.active === 1) {
      const process = processes.find((candidate) => candidate.id === detail.activeProcessIds[0]);
      const state = process?.status === 'starting' ? 'starting' : 'running';
      return `${process?.name ?? 'command'} ${state}`;
    }
    return `${detail.active} commands running`;
  }

  const fragments: string[] = [];
  addLabelFragment(fragments, detail.running, kind, 'working', 'working');
  addLabelFragment(fragments, detail.starting, kind, 'starting', 'starting');
  addLabelFragment(fragments, detail.needsInput, kind, 'needs input', 'need input');
  return fragments.join(' · ');
}

function kindActivityLabel(kind: ProcessKind, detail: ProjectKindActivityDetail): string {
  if (detail.total === 0) return `no ${kindPlural(kind)}`;

  const fragments: string[] = [];
  if (kind === 'agent') {
    addLabelFragment(fragments, detail.running, kind, 'working', 'working');
    addLabelFragment(fragments, detail.starting, kind, 'starting', 'starting');
    addLabelFragment(fragments, detail.needsInput, kind, 'needs input', 'need input');
    addLabelFragment(fragments, detail.waiting, kind, 'waiting', 'waiting');
  } else {
    addLabelFragment(fragments, detail.running, kind, 'running', 'running');
    addLabelFragment(fragments, detail.starting, kind, 'starting', 'starting');
  }
  addLabelFragment(fragments, detail.crashed, kind, 'crashed', 'crashed');
  addLabelFragment(fragments, detail.idle, kind, 'idle', 'idle');
  addLabelFragment(fragments, detail.stopped, kind, 'stopped', 'stopped');
  return fragments.join(' · ');
}

function addLabelFragment(
  fragments: string[],
  count: number,
  kind: ProcessKind,
  singularState: string,
  pluralState: string
): void {
  if (count === 0) return;
  const state = count === 1 ? singularState : pluralState;
  const subject = fragments.length === 0 ? ` ${count === 1 ? kind : kindPlural(kind)}` : '';
  fragments.push(`${count}${subject} ${state}`);
}

function kindPlural(kind: ProcessKind): string {
  return kind === 'agent' ? 'agents' : kind === 'terminal' ? 'terminals' : 'commands';
}

function targetCandidate(
  process: ProcessView,
  state: KindActivityState,
  stats: ProcessRuntimeStats | undefined
): KindTargetCandidate | null {
  if (process.kind === 'agent') {
    if (state !== 'needs_input' && state !== 'running' && state !== 'starting') return null;
    return {
      id: process.id,
      priority: state === 'needs_input' ? 2 : 1,
      recency: agentRecency(process, stats)
    };
  }
  if (state !== 'running' && (process.kind === 'terminal' || state !== 'starting')) return null;
  return {
    id: process.id,
    priority: process.kind === 'terminal' && stats?.foreground_active === true ? 2 : 1,
    recency: runtimeRecency(stats)
  };
}

function rosterCandidate(
  process: ProcessView,
  state: KindActivityState,
  stats: ProcessRuntimeStats | undefined
): KindTargetCandidate {
  return {
    id: process.id,
    priority: state === 'needs_input'
      ? 3
      : process.status === 'running' || process.status === 'starting'
        ? 2
        : 1,
    recency: process.kind === 'agent'
      ? agentRecency(process, stats)
      : process.exited_at ?? runtimeRecency(stats)
  };
}

function agentRecency(
  process: ProcessView,
  stats: ProcessRuntimeStats | undefined
): number {
  return process.agent_state.last_content_change_at
    ?? process.agent_state.last_output_at
    ?? process.exited_at
    ?? runtimeRecency(stats);
}

function runtimeRecency(stats: ProcessRuntimeStats | undefined): number {
  return stats && Number.isFinite(stats.uptime_seconds)
    ? Date.now() - stats.uptime_seconds * 1_000
    : Number.NEGATIVE_INFINITY;
}

function isBetterTarget(
  candidate: KindTargetCandidate,
  current: KindTargetCandidate | null
): boolean {
  if (!current) return true;
  if (candidate.priority !== current.priority) return candidate.priority > current.priority;
  if (candidate.recency !== current.recency) return candidate.recency > current.recency;
  return candidate.id > current.id;
}

function compareTargets(left: KindTargetCandidate, right: KindTargetCandidate): number {
  if (left.priority !== right.priority) return right.priority - left.priority;
  if (left.recency !== right.recency) return right.recency - left.recency;
  return right.id - left.id;
}

function presentation(
  state: ProcessActivityState,
  shortLabel: string,
  label: string
): ProcessActivityPresentation {
  return { state, shortLabel, label };
}
