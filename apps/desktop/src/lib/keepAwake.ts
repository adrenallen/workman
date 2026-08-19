import type { ConnectionStatus, ProcessStatus, ProcessView } from './daemon';

export const KEEP_AWAKE_SETTLE_MS = 60_000;
export const KEEP_AWAKE_UNREACHABLE_MS = 10 * 60_000;
export const KEEP_AWAKE_MAX_OBSERVATION_GAP_MS = 5_000;
export const KEEP_AWAKE_AUTO_STORAGE_KEY = 'workman.keep-awake.auto.v1';
export const KEEP_AWAKE_STATE_STORAGE_KEY = 'workman.keep-awake.state.v1';

export type KeepAwakeMode = 'all' | 'specific';
export type KeepAwakeArmSource = 'manual' | 'auto' | null;

export interface KeepAwakeMachineState {
  armed: boolean;
  mode: KeepAwakeMode;
  armSource: KeepAwakeArmSource;
  watchedAgentIds: number[];
  idleObservedMs: number;
  lastIdleObservationAt: number | null;
}

export interface AutoKeepAwakeState {
  activeAgentIds: number[];
  suppressedUntilActivityEdge: boolean;
  lastObservationAt: number | null;
}

export interface PersistedKeepAwakeState {
  autoState: Pick<AutoKeepAwakeState, 'activeAgentIds' | 'suppressedUntilActivityEdge'>;
  preferredMode: KeepAwakeMode;
  preferredSpecificAgentId: number | null;
  activeHold: Pick<KeepAwakeMachineState, 'mode' | 'armSource' | 'watchedAgentIds'> | null;
}

export interface AutoKeepAwakeEvaluation {
  state: AutoKeepAwakeState;
  activeAgentIds: number[];
  activityEdge: boolean;
  shouldArm: boolean;
}

interface KeepAwakePreferenceStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

export interface KeepAwakeEvaluation {
  state: KeepAwakeMachineState;
  shouldRelease: boolean;
  waitingAgentIds: number[];
  releaseInSeconds: number | null;
}

export interface KeepAwakeReconciliation {
  state: KeepAwakeMachineState;
  holdLost: boolean;
}

export interface KeepAwakeObservation {
  connected: boolean;
}

export interface KeepAwakeConnectionState {
  disconnectedObservedMs: number;
  lastObservationAt: number | null;
}

export interface KeepAwakeConnectionEvaluation {
  state: KeepAwakeConnectionState;
  daemonUnreachable: boolean;
  recheckInMs: number | null;
}

export type KeepAwakeAgent = Pick<ProcessView, 'id' | 'kind' | 'status' | 'agent_state'>;

const liveStatuses = new Set<ProcessStatus>(['starting', 'running']);
const activeObservation: KeepAwakeObservation = { connected: true };

export function initialKeepAwakeState(): KeepAwakeMachineState {
  return {
    armed: false,
    mode: 'all',
    armSource: null,
    watchedAgentIds: [],
    idleObservedMs: 0,
    lastIdleObservationAt: null
  };
}

export function initialAutoKeepAwakeState(): AutoKeepAwakeState {
  return {
    activeAgentIds: [],
    suppressedUntilActivityEdge: false,
    lastObservationAt: null
  };
}

export function initialPersistedKeepAwakeState(): PersistedKeepAwakeState {
  return {
    autoState: {
      activeAgentIds: [],
      suppressedUntilActivityEdge: false
    },
    preferredMode: 'all',
    preferredSpecificAgentId: null,
    activeHold: null
  };
}

export function loadAutoKeepAwakePreference(
  storage: KeepAwakePreferenceStorage | null = browserStorage()
): boolean {
  if (!storage) return false;
  try {
    return JSON.parse(storage.getItem(KEEP_AWAKE_AUTO_STORAGE_KEY) ?? 'false') === true;
  } catch {
    return false;
  }
}

export function saveAutoKeepAwakePreference(
  enabled: boolean,
  storage: KeepAwakePreferenceStorage | null = browserStorage()
): void {
  if (!storage) return;
  try {
    storage.setItem(KEEP_AWAKE_AUTO_STORAGE_KEY, JSON.stringify(enabled));
  } catch {
    // Keep the current session usable if per-machine storage is unavailable or full.
  }
}

export function loadPersistedKeepAwakeState(
  storage: KeepAwakePreferenceStorage | null = browserStorage()
): PersistedKeepAwakeState {
  const fallback = initialPersistedKeepAwakeState();
  if (!storage) return fallback;
  try {
    const parsed = JSON.parse(storage.getItem(KEEP_AWAKE_STATE_STORAGE_KEY) ?? 'null');
    if (!parsed || typeof parsed !== 'object') return fallback;
    const activeAgentIds = validAgentIds(parsed.autoState?.activeAgentIds);
    const preferredMode = validMode(parsed.preferredMode) ? parsed.preferredMode : 'all';
    const preferredSpecificAgentId = validAgentId(parsed.preferredSpecificAgentId)
      ? parsed.preferredSpecificAgentId
      : null;
    const hold = parsed.activeHold;
    const activeHold = hold
      && validMode(hold.mode)
      && (hold.armSource === 'manual' || hold.armSource === 'auto')
      ? {
          mode: hold.mode,
          armSource: hold.armSource,
          watchedAgentIds: validAgentIds(hold.watchedAgentIds)
        }
      : null;
    return {
      autoState: {
        activeAgentIds,
        suppressedUntilActivityEdge: parsed.autoState?.suppressedUntilActivityEdge === true
      },
      preferredMode,
      preferredSpecificAgentId,
      activeHold
    };
  } catch {
    return fallback;
  }
}

export function savePersistedKeepAwakeState(
  state: PersistedKeepAwakeState,
  storage: KeepAwakePreferenceStorage | null = browserStorage()
): void {
  if (!storage) return;
  try {
    storage.setItem(KEEP_AWAKE_STATE_STORAGE_KEY, JSON.stringify(state));
  } catch {
    // Keep the current session usable if per-machine storage is unavailable or full.
  }
}

export function initialKeepAwakeConnectionState(): KeepAwakeConnectionState {
  return {
    disconnectedObservedMs: 0,
    lastObservationAt: null
  };
}

export function runningAgents<T extends KeepAwakeAgent>(processes: T[]): T[] {
  return processes.filter(
    (process) => process.kind === 'agent' && liveStatuses.has(process.status)
  );
}

export function activeKeepAwakeAgents<T extends KeepAwakeAgent>(processes: T[]): T[] {
  return runningAgents(processes).filter(agentRequiresKeepAwake);
}

export function shouldHoldAutoKeepAwake(
  processes: KeepAwakeAgent[],
  enabled: boolean,
  suppressedUntilActivityEdge = false
): boolean {
  return enabled
    && !suppressedUntilActivityEdge
    && activeKeepAwakeAgents(processes).length > 0;
}

export function shouldSubscribeProcessStatuses(
  documentVisible: boolean,
  keepAwakeArmed: boolean,
  autoKeepAwakeEnabled = false
): boolean {
  return documentVisible || keepAwakeArmed || autoKeepAwakeEnabled;
}

export function armKeepAwake(
  mode: KeepAwakeMode,
  specificAgentId: number | null,
  armSource: Exclude<KeepAwakeArmSource, null> = 'manual'
): KeepAwakeMachineState {
  const watchedAgentIds = mode === 'specific'
    ? specificAgentId === null ? [] : [specificAgentId]
    : [];

  return {
    armed: true,
    mode,
    armSource,
    watchedAgentIds,
    idleObservedMs: 0,
    lastIdleObservationAt: null
  };
}

export function disarmKeepAwake(state: KeepAwakeMachineState): KeepAwakeMachineState {
  if (
    !state.armed
    && state.watchedAgentIds.length === 0
    && state.idleObservedMs === 0
    && state.lastIdleObservationAt === null
  ) return state;
  return {
    ...state,
    armed: false,
    armSource: null,
    watchedAgentIds: [],
    idleObservedMs: 0,
    lastIdleObservationAt: null
  };
}

export function reconcileKeepAwakeIntent(
  state: KeepAwakeMachineState,
  nativeArmed: boolean
): KeepAwakeReconciliation {
  const holdLost = state.armed && !nativeArmed;
  return {
    state: holdLost ? disarmKeepAwake(state) : state,
    holdLost
  };
}

/**
 * Coordinate the persistent auto preference with the existing arm/release machine.
 * A manual disarm records the currently active ids and suppresses re-arming until
 * any agent has a fresh inactive-to-active edge. Edges are accepted only across
 * a short, continuous observation interval; reload, daemon reconnect, sleep, and
 * project-switch gaps rebase the snapshot without cancelling manual suppression.
 */
export function evaluateAutoKeepAwake(
  state: AutoKeepAwakeState,
  processes: KeepAwakeAgent[],
  enabled: boolean,
  now = 0,
  observation: KeepAwakeObservation = activeObservation,
  maxObservationGapMs = KEEP_AWAKE_MAX_OBSERVATION_GAP_MS
): AutoKeepAwakeEvaluation {
  if (!observation.connected) {
    const paused = state.lastObservationAt === null
      ? state
      : { ...state, lastObservationAt: null };
    return {
      state: paused,
      activeAgentIds: state.activeAgentIds,
      activityEdge: false,
      shouldArm: false
    };
  }
  const activeAgentIds = activeKeepAwakeAgents(processes)
    .map((process) => process.id)
    .sort((left, right) => left - right);
  const previous = new Set(state.activeAgentIds);
  const observationContinuous = state.lastObservationAt !== null
    && now >= state.lastObservationAt
    && now - state.lastObservationAt <= maxObservationGapMs;
  const activityEdge = observationContinuous
    && activeAgentIds.some((id) => !previous.has(id));
  const suppressedUntilActivityEdge = enabled
    ? state.suppressedUntilActivityEdge && !activityEdge
    : false;
  const nextState = sameIds(state.activeAgentIds, activeAgentIds)
    && state.suppressedUntilActivityEdge === suppressedUntilActivityEdge
    && state.lastObservationAt === now
    ? state
    : { activeAgentIds, suppressedUntilActivityEdge, lastObservationAt: now };

  return {
    state: nextState,
    activeAgentIds,
    activityEdge,
    shouldArm: shouldHoldAutoKeepAwake(processes, enabled, suppressedUntilActivityEdge)
  };
}

export function suppressAutoKeepAwake(
  state: AutoKeepAwakeState,
  processes: KeepAwakeAgent[],
  now = state.lastObservationAt
): AutoKeepAwakeState {
  const activeAgentIds = activeKeepAwakeAgents(processes)
    .map((process) => process.id)
    .sort((left, right) => left - right);
  if (
    sameIds(state.activeAgentIds, activeAgentIds)
    && state.suppressedUntilActivityEdge
    && state.lastObservationAt === now
  ) {
    return state;
  }
  return { activeAgentIds, suppressedUntilActivityEdge: true, lastObservationAt: now };
}

export function evaluateKeepAwake(
  state: KeepAwakeMachineState,
  processes: KeepAwakeAgent[],
  now: number,
  observation: KeepAwakeObservation = activeObservation,
  settleMs = KEEP_AWAKE_SETTLE_MS,
  maxObservationGapMs = KEEP_AWAKE_MAX_OBSERVATION_GAP_MS
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
    if (agentRequiresKeepAwake(process)) return true;
    return state.mode === 'all'
      && process.agent_state.state === 'idle'
      && process.agent_state.last_output_at == null;
  });

  if (!observation.connected) {
    return {
      state: pauseIdleObservation(state),
      shouldRelease: false,
      waitingAgentIds: [],
      releaseInSeconds: null
    };
  }

  if (waitingAgentIds.length > 0) {
    return {
      state: resetIdleObservation(state),
      shouldRelease: false,
      waitingAgentIds,
      releaseInSeconds: null
    };
  }

  const delta = observationDelta(state.lastIdleObservationAt, now, maxObservationGapMs);
  const idleObservedMs = state.idleObservedMs + delta;
  if (idleObservedMs >= settleMs) {
    return {
      state: disarmKeepAwake(state),
      shouldRelease: true,
      waitingAgentIds: [],
      releaseInSeconds: 0
    };
  }

  const nextState = state.idleObservedMs === idleObservedMs && state.lastIdleObservationAt === now
    ? state
    : { ...state, idleObservedMs, lastIdleObservationAt: now };
  return {
    state: nextState,
    shouldRelease: false,
    waitingAgentIds: [],
    releaseInSeconds: Math.max(1, Math.ceil((settleMs - idleObservedMs) / 1_000))
  };
}

/** Evaluate with the monotonic timestamp captured by the caller's latest active UI tick. */
export function evaluateKeepAwakeAtCurrentTime(
  state: KeepAwakeMachineState,
  processes: KeepAwakeAgent[],
  monotonicNow: number,
  observation: KeepAwakeObservation,
  settleMs = KEEP_AWAKE_SETTLE_MS
): KeepAwakeEvaluation {
  return evaluateKeepAwake(state, processes, monotonicNow, observation, settleMs);
}

/**
 * Count only short, actively observed disconnect intervals. A suspended or throttled
 * webview creates a gap and restarts confirmation instead of converting sleep time
 * into a daemon-outage verdict.
 */
export function evaluateKeepAwakeConnection(
  state: KeepAwakeConnectionState,
  connectionStatus: ConnectionStatus['status'],
  now: number,
  unreachableMs = KEEP_AWAKE_UNREACHABLE_MS,
  maxObservationGapMs = KEEP_AWAKE_MAX_OBSERVATION_GAP_MS
): KeepAwakeConnectionEvaluation {
  if (connectionStatus === 'connected') {
    const reset = resetConnectionObservation(state);
    return { state: reset, daemonUnreachable: false, recheckInMs: null };
  }

  const delta = observationDelta(state.lastObservationAt, now, maxObservationGapMs);
  const disconnectedObservedMs = state.disconnectedObservedMs + delta;
  const nextState = state.disconnectedObservedMs === disconnectedObservedMs
    && state.lastObservationAt === now
    ? state
    : { disconnectedObservedMs, lastObservationAt: now };
  const daemonUnreachable = disconnectedObservedMs >= unreachableMs;
  return {
    state: nextState,
    daemonUnreachable,
    recheckInMs: daemonUnreachable ? null : Math.max(0, unreachableMs - disconnectedObservedMs)
  };
}

function observationDelta(
  previous: number | null,
  now: number,
  maxObservationGapMs: number
): number {
  if (previous === null) return 0;
  return Math.min(Math.max(0, now - previous), maxObservationGapMs);
}

function resetIdleObservation(state: KeepAwakeMachineState): KeepAwakeMachineState {
  if (state.idleObservedMs === 0 && state.lastIdleObservationAt === null) return state;
  return { ...state, idleObservedMs: 0, lastIdleObservationAt: null };
}

function pauseIdleObservation(state: KeepAwakeMachineState): KeepAwakeMachineState {
  if (state.lastIdleObservationAt === null) return state;
  return { ...state, lastIdleObservationAt: null };
}

function resetConnectionObservation(
  state: KeepAwakeConnectionState
): KeepAwakeConnectionState {
  if (state.disconnectedObservedMs === 0 && state.lastObservationAt === null) return state;
  return initialKeepAwakeConnectionState();
}

function sameIds(left: number[], right: number[]): boolean {
  return left.length === right.length && left.every((id, index) => id === right[index]);
}

/**
 * Waiting is an idle refinement. It counts as active only while at least one
 * finite timer/watch is live and unpaused; paused or metadata-only parks must
 * let auto keep-awake settle and release.
 */
function agentRequiresKeepAwake(process: KeepAwakeAgent): boolean {
  const attention = String(process.agent_state.state);
  if (
    process.agent_state.needs_input
    || process.agent_state.working
    || process.agent_state.thinking
    || process.agent_state.planning
    || attention === 'needs_input'
    || attention === 'working'
  ) return true;
  if (attention === 'waiting') {
    return process.agent_state.waiting_on?.some(
      (reason) => !reason.paused && reason.max_wait_ms > 0 && reason.remaining_ms > 0
    ) === true;
  }
  // Preserve PR #15's protection for a just-launched agent before its first status sample.
  return process.status === 'starting' && process.agent_state.last_output_at == null;
}

function validMode(value: unknown): value is KeepAwakeMode {
  return value === 'all' || value === 'specific';
}

function validAgentId(value: unknown): value is number {
  return Number.isSafeInteger(value) && Number(value) > 0;
}

function validAgentIds(value: unknown): number[] {
  if (!Array.isArray(value)) return [];
  return [...new Set(value.filter(validAgentId))].sort((left, right) => left - right);
}

function browserStorage(): KeepAwakePreferenceStorage | null {
  return typeof localStorage === 'undefined' ? null : localStorage;
}
