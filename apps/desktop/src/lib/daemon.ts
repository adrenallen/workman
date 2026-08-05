import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

import type {
  CoordinationClient,
  CoordinationSnapshot,
  NewScratchpadInput,
  NewTodoInput,
  Scratchpad,
  ScratchpadRead,
  TodoComment,
  TodoCompleteResult,
  TodoDetail
} from './coordination';
import type {
  AgentTool,
  AgentToolConfigPreview,
  AgentToolConfigWrite,
  AgentToolDeepCheck,
  AgentToolsHealth,
  AgentToolInput,
  AgentToolsClient,
  DeleteAgentToolResult,
  SpawnAgentInput,
  SpawnAgentResult
} from './agentTools';
import {
  resetLiveStats,
  updateLiveStats,
  type LiveStatsSnapshot
} from './liveStats';
import {
  resetTimerLifecycle,
  updateTimerLifecycle,
  type TimerLifecycleEvent,
  type TimerView
} from './timerLifecycle';

export type ProjectStatus = 'running' | 'error' | 'idle';
export type ProcessStatus = 'stopped' | 'starting' | 'running' | 'exited' | 'crashed';
export type ProcessKind = 'command' | 'terminal' | 'agent';
export type ProcessSource = 'yml' | 'local';
export type AttentionState = 'working' | 'needs_input' | 'idle' | 'exited';

export interface Project {
  id: number;
  path: string;
  name: string;
  display_name: string | null;
  icon: string | null;
  selected: boolean;
  sort_order: number;
  status: ProjectStatus;
}

export interface ConnectionStatus {
  status: 'connecting' | 'connected' | 'disconnected';
  message: string | null;
  port: number | null;
  app_version: string;
  app_build_id: string;
  app_control_protocol_version: number;
  daemon_version: string | null;
  daemon_build_id: string | null;
  daemon_control_protocol_version: number | null;
  version_compatible: boolean;
}

export class DaemonRequestError extends Error {
  constructor(
    readonly method: string,
    readonly code: string,
    message: string
  ) {
    super(message);
    this.name = 'DaemonRequestError';
  }
}

export function isUnsupportedControlMethod(cause: unknown): boolean {
  return cause instanceof DaemonRequestError && cause.code === 'method_not_found';
}

export interface ProcessView {
  id: number;
  project_id: number;
  kind: ProcessKind;
  name: string;
  command: string | null;
  working_dir: string;
  env: Record<string, string>;
  auto_start: boolean;
  auto_restart: boolean;
  restart_when_changed: string[];
  source: ProcessSource;
  trust_hash: string | null;
  status: ProcessStatus;
  pid: number | null;
  exit_code: number | null;
  exit_signal: number | null;
  exited_at: number | null;
  agent_tool_id: number | null;
  spawned_by_process_id: number | null;
  sort_order: number;
  agent_state: AgentState;
}

export interface AgentState {
  state: AttentionState;
  working: boolean;
  needs_input: boolean;
  idle: boolean;
  exited: boolean;
  thinking: boolean;
  planning: boolean;
  tool_type: string | null;
  idle_seconds: number;
  last_output_seconds: number | null;
  last_output_at: number | null;
  last_content_change_at: number | null;
  classification: string | null;
}

export interface TrustFields {
  command: string | null;
  working_dir: string;
  env: Record<string, string>;
  auto_start: boolean;
  auto_restart: boolean;
  restart_when_changed: string[];
}

export interface TrustFieldChange {
  field: keyof TrustFields;
  previous: unknown | null;
  current: unknown;
}

export interface TrustReview {
  process_id: number;
  process_name: string;
  trusted: boolean;
  expected_hash: string;
  fields: TrustFields;
  changes: TrustFieldChange[];
}

export interface TerminalFrame {
  process_id: number;
  start_offset: number;
  gap: boolean;
  data: number[];
}

type DaemonFrame =
  | { kind: 'text'; data: string }
  | { kind: 'binary'; data: number[] }
  | { kind: 'terminal'; data: TerminalFrame };

interface DaemonResponse {
  id: string;
  ok: boolean;
  result?: unknown;
  error?: { code: string; message: string };
}

interface ProcessStatusesEvent {
  event: 'process.statuses';
  processes: ProcessView[];
  stats?: LiveStatsSnapshot;
  timers?: TimerView[];
  timer_events?: TimerLifecycleEvent[];
}

interface PendingRequest {
  method: string;
  resolve: (result: unknown) => void;
  reject: (error: Error) => void;
  timeout: ReturnType<typeof setTimeout>;
}

export class DaemonClient implements CoordinationClient, AgentToolsClient {
  private sequence = 0;
  private pending = new Map<string, PendingRequest>();
  private unlisten: UnlistenFn[] = [];
  private terminalListeners = new Set<(frame: TerminalFrame) => void>();
  private processListeners = new Set<(processes: ProcessView[]) => void>();

  async start(
    onStatus: (status: ConnectionStatus) => void,
    onProtocolError: (message: string) => void
  ): Promise<ConnectionStatus> {
    this.unlisten.push(
      await listen<ConnectionStatus>('daemon://status', (event) => onStatus(event.payload)),
      await listen<DaemonFrame>('daemon://message', (event) => {
        if (event.payload.kind === 'text') {
          this.handleText(event.payload.data, onProtocolError);
        } else if (event.payload.kind === 'terminal') {
          for (const listener of this.terminalListeners) listener(event.payload.data);
        }
      })
    );
    return invoke<ConnectionStatus>('daemon_status');
  }

  projects(): Promise<Project[]> {
    return this.request('projects.list');
  }

  register(path: string): Promise<Project[]> {
    return this.request('projects.register', { path });
  }

  select(projectId: number): Promise<Project[]> {
    return this.request('projects.select', { project_id: projectId });
  }

  rename(projectId: number, name: string): Promise<Project[]> {
    return this.request('projects.rename', { project_id: projectId, name });
  }

  reorderProjects(orderedIds: number[]): Promise<Project[]> {
    return this.request('project.reorder', { ordered_ids: orderedIds });
  }

  coordinationSnapshot(projectId: number): Promise<CoordinationSnapshot> {
    return this.requestOptional(
      'coordination.snapshot',
      { project_id: projectId },
      {
        project_id: projectId,
        todos: [],
        todo_total_count: 0,
        scratchpads: [],
        scratchpad_total_count: 0
      }
    );
  }

  coordinationTodo(projectId: number, todoId: number): Promise<TodoDetail> {
    return this.request('coordination.todo', { project_id: projectId, todo_id: todoId });
  }

  coordinationTodoCreate(projectId: number, input: NewTodoInput): Promise<TodoDetail['todo']> {
    return this.request('coordination.todo_create', { project_id: projectId, ...input });
  }

  coordinationTodoComplete(
    projectId: number,
    todoId: number,
    completed: boolean
  ): Promise<TodoCompleteResult> {
    return this.request('coordination.todo_complete', {
      project_id: projectId,
      todo_id: todoId,
      completed
    });
  }

  coordinationTodoComment(
    projectId: number,
    todoId: number,
    body: string
  ): Promise<TodoComment> {
    return this.request('coordination.todo_comment', {
      project_id: projectId,
      todo_id: todoId,
      body
    });
  }

  coordinationScratchpad(projectId: number, scratchpadId: number): Promise<ScratchpadRead> {
    return this.request('coordination.scratchpad', {
      project_id: projectId,
      scratchpad_id: scratchpadId
    });
  }

  coordinationScratchpadCreate(projectId: number, input: NewScratchpadInput): Promise<Scratchpad> {
    return this.request('coordination.scratchpad_create', {
      project_id: projectId,
      ...input
    });
  }

  processes(projectId: number): Promise<ProcessView[]> {
    return this.request('process.list', { project_id: projectId });
  }

  reorderProcesses(
    projectId: number,
    kind: ProcessKind,
    orderedIds: number[]
  ): Promise<ProcessView[]> {
    return this.request('process.reorder', {
      project_id: projectId,
      kind,
      ordered_ids: orderedIds
    });
  }

  subscribeProcessStatuses(): Promise<{ subscribed: boolean }> {
    return this.requestOptional('process.status_subscribe', {}, { subscribed: false });
  }

  syncConfig(projectId: number): Promise<{ project_id: number; synced: boolean }> {
    return this.requestOptional(
      'config.sync',
      { project_id: projectId },
      { project_id: projectId, synced: false }
    );
  }

  startProcess(processId: number): Promise<ProcessView> {
    return this.request('process.start', { process_id: processId });
  }

  stopProcess(processId: number): Promise<ProcessView> {
    return this.request('process.stop', { process_id: processId });
  }

  restartProcess(processId: number): Promise<ProcessView> {
    return this.request('process.restart', { process_id: processId });
  }

  spawnTerminal(projectId: number): Promise<ProcessView> {
    return this.request('process.spawn_terminal', { project_id: projectId });
  }

  listAgentTools(): Promise<AgentTool[]> {
    return this.requestOptional('agent_tools.list', {}, []);
  }

  agentToolsHealth(): Promise<AgentToolsHealth> {
    return this.request('agent_tools.health');
  }

  previewAgentToolConfig(agentToolId: number): Promise<AgentToolConfigPreview> {
    return this.request('agent_tools.configure_preview', { agent_tool_id: agentToolId });
  }

  configureAgentTool(
    agentToolId: number,
    expectedPreviewSha256: string
  ): Promise<AgentToolConfigWrite> {
    return this.request('agent_tools.configure', {
      agent_tool_id: agentToolId,
      confirm_write: true,
      expected_preview_sha256: expectedPreviewSha256
    });
  }

  deepCheckAgentTool(projectId: number, agentToolId: number): Promise<AgentToolDeepCheck> {
    return this.request('agent_tools.deep_check', {
      project_id: projectId,
      agent_tool_id: agentToolId
    });
  }

  saveAgentTool(tool: AgentToolInput): Promise<AgentTool> {
    return this.request('agent_tools.save', { tool });
  }

  deleteAgentTool(agentToolId: number): Promise<DeleteAgentToolResult> {
    return this.request('agent_tools.delete', { agent_tool_id: agentToolId });
  }

  spawnAgent(input: SpawnAgentInput): Promise<SpawnAgentResult> {
    return this.request('agents.spawn', { ...input });
  }

  closeProcess(processId: number): Promise<ProcessView> {
    return this.request('process.close', { process_id: processId });
  }

  trustReview(processId: number): Promise<TrustReview> {
    return this.request('process.trust_review', { process_id: processId });
  }

  trustYmlProcess(processId: number, expectedHash: string): Promise<ProcessView> {
    return this.request('process.trust_yml', {
      process_id: processId,
      expected_hash: expectedHash
    });
  }

  attachTerminal(processId: number, offset = 0): Promise<{ process_id: number; offset: number }> {
    return this.request('terminal.attach', { process_id: processId, offset });
  }

  detachTerminal(): Promise<{ process_id: null }> {
    return this.request('terminal.detach');
  }

  sendInput(processId: number, data: Uint8Array): Promise<ProcessView> {
    return this.request('process.send_input', {
      process_id: processId,
      data: bytesToBase64(data)
    });
  }

  submitInput(processId: number, input: string): Promise<ProcessView> {
    return this.request('process.send_input', {
      process_id: processId,
      data: bytesToBase64(new TextEncoder().encode(input)),
      submit: true
    });
  }

  resizeTerminal(
    processId: number,
    rows: number,
    cols: number,
    pixelWidth: number,
    pixelHeight: number
  ): Promise<ProcessView> {
    return this.request('process.resize', {
      process_id: processId,
      rows,
      cols,
      pixel_width: pixelWidth,
      pixel_height: pixelHeight
    });
  }

  onTerminal(listener: (frame: TerminalFrame) => void): () => void {
    this.terminalListeners.add(listener);
    return () => this.terminalListeners.delete(listener);
  }

  onProcessStatuses(listener: (processes: ProcessView[]) => void): () => void {
    this.processListeners.add(listener);
    return () => this.processListeners.delete(listener);
  }

  close(): void {
    for (const unlisten of this.unlisten.splice(0)) unlisten();
    for (const pending of this.pending.values()) {
      clearTimeout(pending.timeout);
      pending.reject(new Error('Desktop connection closed'));
    }
    this.pending.clear();
    this.terminalListeners.clear();
    this.processListeners.clear();
    resetLiveStats();
    resetTimerLifecycle();
  }

  /** Typed escape hatch for small control-channel surfaces owned by feature modules. */
  control<T>(method: string, params: Record<string, unknown> = {}): Promise<T> {
    return this.request(method, params);
  }

  restartDaemon(): Promise<{ restarting: boolean }> {
    return invoke('daemon_restart', { confirmProcessesStopped: true });
  }

  private async requestOptional<T>(
    method: string,
    params: Record<string, unknown>,
    fallback: T
  ): Promise<T> {
    try {
      return await this.request<T>(method, params);
    } catch (cause) {
      if (isUnsupportedControlMethod(cause)) return fallback;
      throw cause;
    }
  }

  private async request<T>(type: string, fields: Record<string, unknown> = {}): Promise<T> {
    const id = `desktop-${Date.now()}-${++this.sequence}`;
    const message = JSON.stringify({ id, method: type, params: fields });
    const response = new Promise<T>((resolve, reject) => {
      const timeout = setTimeout(() => {
        this.pending.delete(id);
        reject(new Error('The daemon did not answer in time'));
      }, 5000);
      this.pending.set(id, {
        method: type,
        resolve: (result) => resolve(result as T),
        reject,
        timeout
      });
    });

    try {
      await invoke('daemon_send', { message });
    } catch (error) {
      const pending = this.pending.get(id);
      if (pending) {
        clearTimeout(pending.timeout);
        this.pending.delete(id);
        pending.reject(new Error(String(error)));
      }
    }
    return response;
  }

  private handleText(text: string, onProtocolError: (message: string) => void): void {
    let response: DaemonResponse | ProcessStatusesEvent;
    try {
      response = JSON.parse(text) as DaemonResponse | ProcessStatusesEvent;
    } catch {
      onProtocolError('The daemon sent an unreadable control message');
      return;
    }
    const event = response as ProcessStatusesEvent;
    if (event.event === 'process.statuses' && Array.isArray(event.processes)) {
      if (event.stats && typeof event.stats.sampled_at === 'number') {
        updateLiveStats(event.stats);
      }
      if (Array.isArray(event.timers) || Array.isArray(event.timer_events)) {
        updateTimerLifecycle(
          Array.isArray(event.timers) ? event.timers : undefined,
          Array.isArray(event.timer_events) ? event.timer_events : []
        );
      }
      for (const listener of this.processListeners) listener(event.processes);
      return;
    }
    const rpc = response as DaemonResponse;
    if (typeof rpc.id !== 'string') return;

    const pending = this.pending.get(rpc.id);
    if (!pending) return;
    clearTimeout(pending.timeout);
    this.pending.delete(rpc.id);
    if (rpc.ok && rpc.result !== undefined) {
      pending.resolve(rpc.result);
    } else {
      pending.reject(
        new DaemonRequestError(
          pending.method,
          rpc.error?.code ?? 'unknown_error',
          rpc.error?.message ?? 'The daemon rejected the request'
        )
      );
    }
  }
}

function bytesToBase64(bytes: Uint8Array): string {
  let binary = '';
  const chunkSize = 0x8000;
  for (let offset = 0; offset < bytes.length; offset += chunkSize) {
    binary += String.fromCharCode(...bytes.subarray(offset, offset + chunkSize));
  }
  return btoa(binary);
}
