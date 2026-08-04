import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

export type ProjectStatus = 'running' | 'error' | 'idle';

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

interface DaemonFrame {
  kind: 'text' | 'binary';
  data: string | number[];
}

interface ProjectResponse {
  id: string;
  ok: boolean;
  result?: Project[];
  error?: { code: string; message: string };
}

interface PendingRequest {
  resolve: (projects: Project[]) => void;
  reject: (error: Error) => void;
  timeout: ReturnType<typeof setTimeout>;
}

export class DaemonClient {
  private sequence = 0;
  private pending = new Map<string, PendingRequest>();
  private unlisten: UnlistenFn[] = [];

  async start(
    onStatus: (status: ConnectionStatus) => void,
    onProtocolError: (message: string) => void
  ): Promise<ConnectionStatus> {
    this.unlisten.push(
      await listen<ConnectionStatus>('daemon://status', (event) => onStatus(event.payload)),
      await listen<DaemonFrame>('daemon://message', (event) => {
        if (event.payload.kind !== 'text' || typeof event.payload.data !== 'string') return;
        this.handleText(event.payload.data, onProtocolError);
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

  close(): void {
    for (const unlisten of this.unlisten.splice(0)) unlisten();
    for (const pending of this.pending.values()) {
      clearTimeout(pending.timeout);
      pending.reject(new Error('Desktop connection closed'));
    }
    this.pending.clear();
  }

  private async request(type: string, fields: Record<string, unknown> = {}): Promise<Project[]> {
    const id = `desktop-${Date.now()}-${++this.sequence}`;
    const message = JSON.stringify({ id, method: type, params: fields });
    const response = new Promise<Project[]>((resolve, reject) => {
      const timeout = setTimeout(() => {
        this.pending.delete(id);
        reject(new Error('The daemon did not answer in time'));
      }, 5000);
      this.pending.set(id, { resolve, reject, timeout });
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
    let response: ProjectResponse;
    try {
      response = JSON.parse(text) as ProjectResponse;
    } catch {
      onProtocolError('The daemon sent an unreadable control message');
      return;
    }
    if (typeof response.id !== 'string') return;

    const pending = this.pending.get(response.id);
    if (!pending) return;
    clearTimeout(pending.timeout);
    this.pending.delete(response.id);
    if (response.ok && response.result) {
      pending.resolve(response.result);
    } else {
      pending.reject(new Error(response.error?.message ?? 'The daemon rejected the request'));
    }
  }
}
