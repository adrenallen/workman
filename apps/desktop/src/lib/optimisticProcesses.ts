import type { ProcessKind, ProcessView, Project } from './daemon';
import type { SpawnAgentInput } from './agentTools';

export interface OptimisticProcess {
  process: ProcessView;
  error: string | null;
  retry: 'agent' | 'command' | null;
  agentSpawnInput: SpawnAgentInput | null;
  createdAt: number;
}

export interface OptimisticProcessInput {
  id: number;
  project: Project;
  kind: ProcessKind;
  name: string;
  command?: string | null;
  agentToolId?: number | null;
  retry?: OptimisticProcess['retry'];
  agentSpawnInput?: SpawnAgentInput | null;
}

export function createOptimisticProcess(input: OptimisticProcessInput): OptimisticProcess {
  return {
    process: {
      id: input.id,
      project_id: input.project.id,
      kind: input.kind,
      name: input.name,
      command: input.command ?? null,
      working_dir: input.project.path,
      env: {},
      auto_start: true,
      auto_restart: false,
      restart_when_changed: [],
      source: 'local',
      trust_hash: null,
      status: 'starting',
      pid: null,
      exit_code: null,
      exit_signal: null,
      exited_at: null,
      agent_tool_id: input.agentToolId ?? null,
      spawned_by_process_id: null,
      sort_order: Number.MAX_SAFE_INTEGER,
      agent_session_id: null,
      agent_launch_mode: null,
      agent_state: {
        state: 'working',
        working: true,
        needs_input: false,
        idle: false,
        exited: false,
        thinking: false,
        planning: false,
        tool_type: input.kind === 'agent' ? 'agent' : null,
        idle_seconds: 0,
        last_output_seconds: null,
        last_output_at: null,
        last_content_change_at: null,
        classification: 'starting'
      }
    },
    error: null,
    retry: input.retry ?? null,
    agentSpawnInput: input.agentSpawnInput
      ? { ...input.agentSpawnInput, extra_args: [...input.agentSpawnInput.extra_args] }
      : null,
    createdAt: Date.now()
  };
}

export function failOptimisticProcess(
  optimistic: OptimisticProcess,
  cause: unknown
): OptimisticProcess {
  const error = cause instanceof Error ? cause.message : String(cause);
  return {
    ...optimistic,
    error,
    process: {
      ...optimistic.process,
      status: 'crashed',
      agent_state: {
        ...optimistic.process.agent_state,
        state: 'exited',
        working: false,
        exited: true,
        classification: 'start_failed'
      }
    }
  };
}
