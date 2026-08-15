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
import type {
  AgentTemplate,
  AgentTemplateInput,
  AgentTemplatesClient,
  DeleteAgentTemplateResult
} from './agentTemplates';
import type {
  DeleteQuickPromptResult,
  QuickPrompt,
  QuickPromptInput,
  QuickPromptsClient
} from './quickPrompts';
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
import type {
  CreateWorktreeInput,
  ForkWorktreeInput,
  OriginBranchList,
  RemoveWorktreeInput,
  WorktreeList,
  WorktreeMutation,
  WorktreeRemoval,
  WorktreeRefValidation
} from './worktrees';
import {
  replaceWorktreeOperations,
  resetWorktreeOperations,
  type WorktreeOperation,
  type WorktreeOperationAck
} from './worktreeProgress';
import type { ClaimedTodo } from './claimedTodos';
import { DaemonRequestTimeoutError } from './daemonLog';

export type ProjectStatus = 'running' | 'error' | 'idle';
export type ProcessStatus = 'stopped' | 'starting' | 'running' | 'exited' | 'crashed';
export type ProcessKind = 'command' | 'terminal' | 'agent';
export type ProcessSource = 'yml' | 'local';
export type AttentionState = 'working' | 'needs_input' | 'waiting' | 'idle' | 'exited';
export type NotificationType =
  | 'agent_done'
  | 'needs_input'
  | 'process_crashed'
  | 'timer_fired'
  | 'todo_assigned_to_you'
  | 'mentioned_in_comment';

export interface Notification {
  id: number;
  type: NotificationType;
  project_id: number | null;
  process_id: number | null;
  todo_id: number | null;
  comment_id: number | null;
  body: string;
  created_at: number;
  read_at: number | null;
}

export interface Project {
  id: number;
  path: string;
  name: string;
  display_name: string | null;
  icon: string | null;
  icon_color: string | null;
  icon_image: ProjectIconImage | null;
  selected: boolean;
  sort_order: number;
  status: ProjectStatus;
  repository_id: number | null;
  repository_root: string | null;
  parent_project_id: number | null;
  branch: string | null;
  worktree_managed: boolean;
  folder_id: number | null;
}

export interface ProjectIconImage {
  data_url: string;
  source: 'auto' | 'custom';
  path: string;
}

export interface Profile {
  id: number;
  name: string;
  active: boolean;
  project_count: number;
  agent_tool_count: number;
  created_at: number;
}

export interface ProfileRunningProcess {
  id: number;
  project_id: number;
  name: string;
  status: ProcessStatus;
}

export interface ProfileSwitchImpact {
  profile: Profile;
  impact: {
    profile_id: number;
    running_processes: ProfileRunningProcess[];
  };
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

export interface WorktreeHealthCheck {
  id: string;
  label: string;
  required: boolean;
  status: 'ready' | 'attention' | 'missing';
  detail: string;
  version: string | null;
  fix_hint: string | null;
}

export interface WorktreeHealth {
  summary: string;
  all_required_ready: boolean;
  checks: WorktreeHealthCheck[];
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
  agent_session_id: string | null;
  agent_launch_mode: 'fresh' | 'continued_latest' | 'resumed_session' | null;
  agent_state: AgentState;
  claimed_todos?: ClaimedTodo[];
}

export interface RenderedProcessOutput {
  text: string;
  raw_end_offset: number;
  status: ProcessStatus;
}

export interface AgentState {
  state: AttentionState;
  working: boolean;
  needs_input: boolean;
  waiting?: boolean;
  watched?: boolean;
  unread?: boolean;
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
  waiting_on?: AgentWaitingReason[];
}

export interface AgentWaitingProcess {
  process_id: number;
  process_name: string;
}

export interface AgentWaitingReason {
  timer_id: number;
  kind: 'delay' | 'idle_any' | 'idle_all';
  due_at: number;
  max_wait_ms: number;
  remaining_ms: number;
  paused: boolean;
  watch_processes: AgentWaitingProcess[];
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
  kitty_keyboard_flags: number;
  modify_other_keys: number;
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
  worktree_operations?: WorktreeOperation[];
}

interface PendingRequest {
  method: string;
  resolve: (result: unknown) => void;
  reject: (error: Error) => void;
  timeout: ReturnType<typeof setTimeout>;
}

interface QueuedTerminalInput {
  processId: number;
  data: number[];
}

export class DaemonClient
  implements CoordinationClient, AgentToolsClient, AgentTemplatesClient, QuickPromptsClient
{
  private sequence = 0;
  private pending = new Map<string, PendingRequest>();
  private connected = false;
  private inputQueue: QueuedTerminalInput[] = [];
  private inputPumpRunning = false;
  private inputRetry: ReturnType<typeof setTimeout> | null = null;
  private unlisten: UnlistenFn[] = [];
  private terminalListeners = new Set<(frame: TerminalFrame) => void>();
  private processListeners = new Set<(processes: ProcessView[]) => void>();

  async start(
    onStatus: (status: ConnectionStatus) => void,
    onProtocolError: (message: string) => void
  ): Promise<ConnectionStatus> {
    this.unlisten.push(
      await listen<ConnectionStatus>('daemon://status', (event) => {
        this.setConnectionStatus(event.payload);
        onStatus(event.payload);
      }),
      await listen<DaemonFrame>('daemon://message', (event) => {
        if (event.payload.kind === 'text') {
          this.handleText(event.payload.data, onProtocolError);
        } else if (event.payload.kind === 'terminal') {
          for (const listener of this.terminalListeners) listener(event.payload.data);
        }
      })
    );
    const status = await invoke<ConnectionStatus>('daemon_status');
    this.setConnectionStatus(status);
    return status;
  }

  projects(): Promise<Project[]> {
    return this.request('projects.list');
  }

  profiles(): Promise<Profile[]> {
    return this.request<{ profiles: Profile[] }>('profile.list').then((result) => result.profiles);
  }

  createProfile(name: string, copyCurrent: boolean): Promise<Profile> {
    return this.request<{ profile: Profile }>('profile.create', {
      name,
      copy_current: copyCurrent
    }).then((result) => result.profile);
  }

  renameProfile(profileId: number, name: string): Promise<Profile> {
    return this.request<{ profile: Profile }>('profile.rename', {
      profile_id: profileId,
      name
    }).then((result) => result.profile);
  }

  profileSwitchImpact(profileId: number): Promise<ProfileSwitchImpact> {
    return this.request('profile.switch_impact', { profile_id: profileId });
  }

  switchProfile(profileId: number, confirmStopRunning: boolean): Promise<Profile> {
    return this.request<{ profile: Profile }>('profile.switch', {
      profile_id: profileId,
      confirm_stop_running: confirmStopRunning
    }).then((result) => result.profile);
  }

  deleteProfile(profileId: number): Promise<void> {
    return this.request('profile.delete', {
      profile_id: profileId,
      confirm_delete: true
    });
  }

  exportProfile(profileId: number, path: string): Promise<void> {
    return this.request('profile.export', { profile_id: profileId, path });
  }

  importProfile(path: string, name?: string): Promise<Profile> {
    return this.request<{ profile: Profile }>('profile.import', { path, name })
      .then((result) => result.profile);
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

  updateProjectSettings(
    projectId: number,
    displayName: string,
    icon: string | null,
    iconColor: string | null
  ): Promise<Project[]> {
    return this.request('projects.update_settings', {
      project_id: projectId,
      display_name: displayName,
      icon,
      icon_color: iconColor
    });
  }

  setProjectCustomIcon(projectId: number, sourcePath: string): Promise<Project[]> {
    return this.request('projects.set_custom_icon', {
      project_id: projectId,
      source_path: sourcePath
    });
  }

  refreshProjectIcon(projectId: number): Promise<ProjectIconImage | null> {
    return this.request('projects.refresh_icon', { project_id: projectId });
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
        scratchpad_total_count: 0,
        archived_scratchpads: [],
        archived_scratchpad_total_count: 0
      }
    );
  }

  coordinationTodo(projectId: number, todoId: number): Promise<TodoDetail> {
    return this.request('coordination.todo', { project_id: projectId, todo_id: todoId });
  }

  coordinationTodoCreate(projectId: number, input: NewTodoInput): Promise<TodoDetail['todo']> {
    return this.request('coordination.todo_create', { project_id: projectId, ...input });
  }

  coordinationTodoReorder(
    projectId: number,
    orderedIds: number[]
  ): Promise<CoordinationSnapshot> {
    return this.request('coordination.todo_reorder', {
      project_id: projectId,
      ordered_ids: orderedIds
    });
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

  coordinationScratchpadReorder(
    projectId: number,
    orderedIds: number[]
  ): Promise<CoordinationSnapshot> {
    return this.request('coordination.scratchpad_reorder', {
      project_id: projectId,
      ordered_ids: orderedIds
    });
  }

  coordinationScratchpadUpdate(
    projectId: number,
    scratchpadId: number,
    expectedRevision: number,
    content: string
  ): Promise<ScratchpadRead> {
    return this.request('coordination.scratchpad_update', {
      project_id: projectId,
      scratchpad_id: scratchpadId,
      expected_revision: expectedRevision,
      content
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

  unsubscribeProcessStatuses(): Promise<{ subscribed: boolean }> {
    return this.requestOptional('process.status_unsubscribe', {}, { subscribed: false });
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

  markProcessRead(processId: number): Promise<ProcessView> {
    return this.request('process.mark_read', { process_id: processId });
  }

  notifications(read?: boolean): Promise<Notification[]> {
    return this.request('notifications.list', { read, limit: 100 });
  }

  markNotificationRead(notificationId: number): Promise<{ updated: number }> {
    return this.request('notifications.mark_read', { notification_id: notificationId });
  }

  markAllNotificationsRead(): Promise<{ updated: number }> {
    return this.request('notifications.mark_read', { all: true });
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

  worktreeHealth(): Promise<WorktreeHealth> {
    return this.request('worktree.health');
  }

  worktrees(projectId: number, refreshPullRequests = false): Promise<WorktreeList> {
    return this.request('worktree.list', {
      project_id: projectId,
      refresh_pull_requests: refreshPullRequests
    });
  }

  originWorktreeBranches(projectId: number): Promise<OriginBranchList> {
    return this.request('worktree.branches', { project_id: projectId });
  }

  validateWorktreeRef(projectId: number, ref: string): Promise<WorktreeRefValidation> {
    return this.request('worktree.ref_validate', { project_id: projectId, ref });
  }

  createWorktree(input: CreateWorktreeInput): Promise<WorktreeMutation> {
    return this.request('worktree.create', { ...input });
  }

  createWorktreeAsync(
    operationId: string,
    input: CreateWorktreeInput
  ): Promise<WorktreeOperationAck> {
    return this.request('worktree.create_async', { operation_id: operationId, ...input });
  }

  forkWorktree(input: ForkWorktreeInput): Promise<WorktreeMutation> {
    return this.request('worktree.fork', { ...input });
  }

  forkWorktreeAsync(
    operationId: string,
    input: ForkWorktreeInput
  ): Promise<WorktreeOperationAck> {
    return this.request('worktree.fork_async', { operation_id: operationId, ...input });
  }

  adoptWorktree(path: string): Promise<WorktreeMutation> {
    return this.request('worktree.adopt', { path });
  }

  adoptWorktreeAsync(operationId: string, path: string): Promise<WorktreeOperationAck> {
    return this.request('worktree.adopt_async', { operation_id: operationId, path });
  }

  removeWorktree(input: RemoveWorktreeInput): Promise<WorktreeRemoval> {
    return this.request('worktree.remove', { ...input });
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

  setAgentToolIcon(agentToolId: number, sourcePath: string): Promise<AgentTool> {
    return this.request('agent_tools.set_icon', {
      agent_tool_id: agentToolId,
      source_path: sourcePath
    });
  }

  removeAgentToolIcon(agentToolId: number): Promise<AgentTool> {
    return this.request('agent_tools.remove_icon', { agent_tool_id: agentToolId });
  }

  deleteAgentTool(agentToolId: number): Promise<DeleteAgentToolResult> {
    return this.request('agent_tools.delete', { agent_tool_id: agentToolId });
  }

  reorderAgentTools(agentToolIds: number[]): Promise<AgentTool[]> {
    return this.request('agent_tools.reorder', { agent_tool_ids: agentToolIds });
  }

  listQuickPrompts(): Promise<QuickPrompt[]> {
    return this.requestOptional('quick_prompts.list', {}, []);
  }

  saveQuickPrompt(prompt: QuickPromptInput): Promise<QuickPrompt> {
    return this.request('quick_prompts.save', { prompt });
  }

  deleteQuickPrompt(quickPromptId: number): Promise<DeleteQuickPromptResult> {
    return this.request('quick_prompts.delete', { quick_prompt_id: quickPromptId });
  }

  reorderQuickPrompts(quickPromptIds: number[]): Promise<QuickPrompt[]> {
    return this.request('quick_prompts.reorder', { quick_prompt_ids: quickPromptIds });
  }

  listAgentTemplates(): Promise<AgentTemplate[]> {
    return this.request('agent_templates.list', {});
  }

  saveAgentTemplate(template: AgentTemplateInput): Promise<AgentTemplate> {
    return this.request('agent_templates.save', { template });
  }

  deleteAgentTemplate(agentTemplateId: number): Promise<DeleteAgentTemplateResult> {
    return this.request('agent_templates.delete', { agent_template_id: agentTemplateId });
  }

  reorderAgentTemplates(agentTemplateIds: number[]): Promise<AgentTemplate[]> {
    return this.request('agent_templates.reorder', {
      agent_template_ids: agentTemplateIds
    });
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

  renderedProcessOutput(processId: number): Promise<RenderedProcessOutput> {
    return this.request('process.rendered_output', { process_id: processId });
  }

  attachTerminal(
    processId: number,
    offset = 0
  ): Promise<{
    process_id: number;
    offset: number;
    replay_start_offset: number;
    replay_end_offset: number;
    focus_reporting: boolean;
    keyboard_protocol: {
      kitty_flags: number;
      modify_other_keys: number;
    };
  }> {
    return this.request('terminal.attach', { process_id: processId, offset });
  }

  detachTerminal(): Promise<{ process_id: null }> {
    return this.request('terminal.detach');
  }

  sendInput(processId: number, data: Uint8Array): Promise<void> {
    this.inputQueue.push({ processId, data: Array.from(data) });
    this.flushInputQueue();
    return Promise.resolve();
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
    this.connected = false;
    if (this.inputRetry) clearTimeout(this.inputRetry);
    this.inputRetry = null;
    this.inputQueue = [];
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
    resetWorktreeOperations();
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
      const deepCheckInFlight = type === 'agent_tools.deep_check' || Array.from(this.pending.values())
        .some((request) => request.method === 'agent_tools.deep_check');
      const requestTimeout = type.startsWith('daemon.update_')
        ? 180_000
        : type.startsWith('worktree.')
          ? 60_000
          : deepCheckInFlight
            ? 65_000
          : 5_000;
      const timeout = setTimeout(() => {
        this.pending.delete(id);
        reject(new DaemonRequestTimeoutError(type, requestTimeout));
      }, requestTimeout);
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

  private setConnectionStatus(status: ConnectionStatus): void {
    this.connected = status.status === 'connected';
    if (this.connected) this.flushInputQueue();
  }

  private flushInputQueue(): void {
    if (!this.connected || this.inputPumpRunning || this.inputQueue.length === 0) return;
    this.inputPumpRunning = true;
    void (async () => {
      while (this.connected && this.inputQueue.length > 0) {
        try {
          const input = this.inputQueue[0];
          await invoke('daemon_send_input', {
            processId: input.processId,
            data: input.data
          });
          this.inputQueue.shift();
        } catch {
          if (!this.inputRetry) {
            this.inputRetry = setTimeout(() => {
              this.inputRetry = null;
              this.flushInputQueue();
            }, 16);
          }
          break;
        }
      }
    })().finally(() => {
      this.inputPumpRunning = false;
      if (this.connected && this.inputQueue.length > 0 && !this.inputRetry) {
        queueMicrotask(() => this.flushInputQueue());
      }
    });
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
      if (Array.isArray(event.worktree_operations)) {
        replaceWorktreeOperations(event.worktree_operations);
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
