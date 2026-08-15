import type { ProcessStatus, ProcessView } from './daemon';

export const KEEP_AWAKE_SETTLE_MS = 60_000;

export type KeepAwakeMode = 'all' | 'specific';

export interface KeepAwakeMachineState {
  armed: boolean;
  mode: KeepAwakeMode;
  watchedAgentIds: number[];
  idleSince: number | null;
}

export interface KeepAwakeEvaluation {
  state: KeepAwakeMachineState;
  shouldRelease: boolean;
  waitingAgentIds: number[];
  releaseInSeconds: number | null;
}

export type KeepAwakeAgent = Pick<ProcessView, 'id' | 'kind' | 'status' | 'agent_state'>;

const liveStatuses = new Set<ProcessStatus>(['starting', 'running']);

export function initialKeepAwakeState(): KeepAwakeMachineState {
  return {
    armed: false,
    mode: 'all',
    watchedAgentIds: [],
    idleSince: null
  };
}

export function runningAgents<T extends KeepAwakeAgent>(processes: T[]): T[] {
  return processes.filter(
    (process) => process.kind === 'agent' && liveStatuses.has(process.status)
  );
}

export function armKeepAwake(
  mode: KeepAwakeMode,
  specificAgentId: number | null,
  processes: KeepAwakeAgent[]
): KeepAwakeMachineState {
  const watchedAgentIds = mode === 'specific'
    ? specificAgentId === null ? [] : [specificAgentId]
    : runningAgents(processes).map((process) => process.id);

  return {
    armed: true,
    mode,
    watchedAgentIds,
    idleSince: null
  };
}

export function disarmKeepAwake(state: KeepAwakeMachineState): KeepAwakeMachineState {
  if (!state.armed && state.idleSince === null && state.watchedAgentIds.length === 0) return state;
  return {
    ...state,
    armed: false,
    watchedAgentIds: [],
    idleSince: null
  };
}

export function evaluateKeepAwake(
  state: KeepAwakeMachineState,
  processes: KeepAwakeAgent[],
  now: number,
  settleMs = KEEP_AWAKE_SETTLE_MS
): KeepAwakeEvaluation {
  if (!state.armed) {
    return {
      state,
      shouldRelease: false,
      waitingAgentIds: [],
      releaseInSeconds: null
    };
  }

  const byId = new Map(processes.map((process) => [process.id, process]));
  const waitingAgentIds = state.watchedAgentIds.filter((id) => {
    const process = byId.get(id);
    if (!process || !liveStatuses.has(process.status)) return false;
    return process.agent_state.state !== 'idle';
  });

  if (waitingAgentIds.length > 0) {
    const next = state.idleSince === null ? state : { ...state, idleSince: null };
    return {
      state: next,
      shouldRelease: false,
      waitingAgentIds,
      releaseInSeconds: null
    };
  }

  const idleSince = state.idleSince ?? now;
  const elapsed = Math.max(0, now - idleSince);
  if (elapsed >= settleMs) {
    return {
      state: disarmKeepAwake(state),
      shouldRelease: true,
      waitingAgentIds: [],
      releaseInSeconds: 0
    };
  }

  return {
    state: state.idleSince === idleSince ? state : { ...state, idleSince },
    shouldRelease: false,
    waitingAgentIds: [],
    releaseInSeconds: Math.max(1, Math.ceil((settleMs - elapsed) / 1_000))
  };
}
