import type { ProcessView } from './daemon';

export type AgentVisualState = 'idle' | 'working' | 'needs_input' | 'waiting' | 'exited';

export interface AgentStatusPresentation {
  state: AgentVisualState;
  label: string;
  shortLabel: string;
}

/**
 * Resolve one truthful presentation from process lifecycle plus the attention classifier.
 *
 * Lifecycle termination wins over stale attention flags. `waiting` is intentionally accepted
 * before it enters the generated daemon type so todo 366 can add the timer state without
 * creating another visual-state branch.
 */
export function agentStatusPresentation(process: ProcessView): AgentStatusPresentation {
  const attention = String(process.agent_state.state);
  const exited =
    process.status === 'crashed' ||
    process.status === 'exited' ||
    process.status === 'stopped' ||
    process.agent_state.exited ||
    attention === 'exited';

  if (exited) {
    return {
      state: 'exited',
      shortLabel: 'Exited',
      label: exitLabel(process)
    };
  }
  if (process.agent_state.needs_input || attention === 'needs_input') {
    return {
      state: 'needs_input',
      shortLabel: 'Needs input',
      label: `${process.name} · needs input`
    };
  }
  if (attention === 'waiting') {
    return {
      state: 'waiting',
      shortLabel: 'Waiting',
      label: `${process.name} · waiting for timer`
    };
  }
  if (process.agent_state.working || attention === 'working') {
    return {
      state: 'working',
      shortLabel: 'Working',
      label: `${process.name} · working`
    };
  }
  return {
    state: 'idle',
    shortLabel: 'Idle',
    label: `${process.name} · idle`
  };
}

function exitLabel(process: ProcessView): string {
  if (process.status === 'stopped') return `${process.name} · exited cleanly · stopped by user`;
  if (process.status === 'crashed' || process.exit_signal !== null || (process.exit_code ?? 0) !== 0) {
    if (process.exit_signal !== null) return `${process.name} · crashed · signal ${process.exit_signal}`;
    if (process.exit_code !== null) return `${process.name} · crashed · exit code ${process.exit_code}`;
    return `${process.name} · crashed`;
  }
  if (process.exit_code !== null) return `${process.name} · exited cleanly · code ${process.exit_code}`;
  return `${process.name} · exited cleanly`;
}
