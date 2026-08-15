import type { ConnectionStatus, ProcessStatus, ProcessView } from './daemon';

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

export interface KeepAwakeConnectionEvaluation {
  lastConnectedAt: number;
  shouldDisarm: boolean;
  recheckInMs: number | null;
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

export function shouldSubscribeProcessStatuses(
  documentVisible: boolean,
  keepAwakeArmed: boolean
): boolean {
  return documentVisible || keepAwakeArmed;
}

export function armKeepAwake(
  mode: KeepAwakeMode,
  specificAgentId: number | null
): KeepAwakeMachineState {
  const watchedAgentIds = mode === 'specific'
    ? specificAgentId === null ? [] : [specificAgentId]
    : [];

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
  const currentAgentIds = state.mode === 'all'
    ? runningAgents(processes).map((process) => process.id)
    : state.watchedAgentIds;
  const waitingAgentIds = currentAgentIds.filter((id) => {
    const process = byId.get(id);
    if (!process || !liveStatuses.has(process.status)) return false;
    if (process.agent_state.state !== 'idle') return true;
    return state.mode === 'all' && process.agent_state.last_output_at == null;
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

/** Evaluate using a fresh clock read whenever a status push or UI refresh re-runs the caller. */
export function evaluateKeepAwakeAtCurrentTime(
  state: KeepAwakeMachineState,
  processes: KeepAwakeAgent[],
  refresh: number,
  settleMs = KEEP_AWAKE_SETTLE_MS
): KeepAwakeEvaluation {
  void refresh;
  return evaluateKeepAwake(state, processes, Date.now(), settleMs);
}

/** Preserve the last healthy timestamp across connecting/disconnected oscillation. */
export function evaluateKeepAwakeConnection(
  lastConnectedAt: number,
  connectionStatus: ConnectionStatus['status'],
  now: number,
  disconnectMs = 60_000
): KeepAwakeConnectionEvaluation {
  if (connectionStatus === 'connected') {
    return { lastConnectedAt: now, shouldDisarm: false, recheckInMs: null };
  }
  const elapsed = Math.max(0, now - lastConnectedAt);
  return {
    lastConnectedAt,
    shouldDisarm: elapsed >= disconnectMs,
    recheckInMs: Math.max(0, disconnectMs - elapsed)
  };
}
