import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

export type ProjectStatus = 'running' | 'error' | 'idle';
export type ProcessStatus = 'stopped' | 'starting' | 'running' | 'crashed';
export type ProcessKind = 'command' | 'terminal' | 'agent';

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
  status: ProcessStatus;
  pid: number | null;
  exit_code: number | null;
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

interface PendingRequest {
  resolve: (result: unknown) => void;
  reject: (error: Error) => void;
  timeout: ReturnType<typeof setTimeout>;
}

export class DaemonClient {
  private sequence = 0;
  private pending = new Map<string, PendingRequest>();
  private unlisten: UnlistenFn[] = [];
  private terminalListeners = new Set<(frame: TerminalFrame) => void>();

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

  processes(projectId: number): Promise<ProcessView[]> {
    return this.request('process.list', { project_id: projectId });
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

  close(): void {
    for (const unlisten of this.unlisten.splice(0)) unlisten();
    for (const pending of this.pending.values()) {
      clearTimeout(pending.timeout);
      pending.reject(new Error('Desktop connection closed'));
    }
    this.pending.clear();
    this.terminalListeners.clear();
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
    let response: DaemonResponse;
    try {
      response = JSON.parse(text) as DaemonResponse;
    } catch {
      onProtocolError('The daemon sent an unreadable control message');
      return;
    }
    if (typeof response.id !== 'string') return;

    const pending = this.pending.get(response.id);
    if (!pending) return;
    clearTimeout(pending.timeout);
    this.pending.delete(response.id);
    if (response.ok && response.result !== undefined) {
      pending.resolve(response.result);
    } else {
      pending.reject(new Error(response.error?.message ?? 'The daemon rejected the request'));
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
