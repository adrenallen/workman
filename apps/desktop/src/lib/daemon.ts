import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

import type {
  CoordinationClient,
  CoordinationSnapshot,
  NewTodoInput,
  ScratchpadRead,
  TodoComment,
  TodoCompleteResult,
  TodoDetail
} from './coordination';
import type {
  AgentTool,
  AgentToolInput,
  AgentToolsClient,
  DeleteAgentToolResult,
  SpawnAgentInput,
  SpawnAgentResult
} from './agentTools';

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
  status: ProjectStatus;
}

export interface ConnectionStatus {
  status: 'connecting' | 'connected' | 'disconnected';
  message: string | null;
  port: number | null;
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
}

interface PendingRequest {
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

  coordinationSnapshot(projectId: number): Promise<CoordinationSnapshot> {
    return this.request('coordination.snapshot', { project_id: projectId });
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

  processes(projectId: number): Promise<ProcessView[]> {
    return this.request('process.list', { project_id: projectId });
  }

  subscribeProcessStatuses(): Promise<{ subscribed: boolean }> {
    return this.request('process.status_subscribe');
  }

  syncConfig(projectId: number): Promise<{ project_id: number; synced: boolean }> {
    return this.request('config.sync', { project_id: projectId });
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
    return this.request('agent_tools.list');
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
  }

  /** Typed escape hatch for small control-channel surfaces owned by feature modules. */
  control<T>(method: string, params: Record<string, unknown> = {}): Promise<T> {
    return this.request(method, params);
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
      pending.reject(new Error(rpc.error?.message ?? 'The daemon rejected the request'));
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
