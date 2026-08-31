import { writable } from 'svelte/store';

import type { Project } from './daemon';
import type { WorktreeRemoval } from './worktrees';

export type WorktreeOperationMode = 'create' | 'fork' | 'adopt' | 'remove';
export type WorktreeOperationStatus = 'pending' | 'running' | 'completed' | 'failed';
export type WorktreeStepId =
  | 'branch'
  | 'processes'
  | 'worktree'
  | 'files'
  | 'prune'
  | 'environment'
  | 'herd'
  | 'registered';
export type WorktreeStepStatus = 'pending' | 'running' | 'completed' | 'failed' | 'skipped';

export interface WorktreeOperationStep {
  id: WorktreeStepId;
  label: string;
  status: WorktreeStepStatus;
  detail: string | null;
}

export interface WorktreeOperation {
  id: string;
  mode: WorktreeOperationMode;
  source_project_id: number | null;
  repository_id: number | null;
  branch: string | null;
  path: string | null;
  label: string;
  status: WorktreeOperationStatus;
  steps: WorktreeOperationStep[];
  error: string | null;
  error_code: string | null;
  project: Project | null;
  removal: WorktreeRemoval | null;
  created_at: number;
  updated_at: number;
  local: boolean;
}

export interface BeginWorktreeOperation {
  id: string;
  mode: WorktreeOperationMode;
  sourceProjectId: number | null;
  repositoryId: number | null;
  branch?: string | null;
  path?: string | null;
  label?: string | null;
}

export interface WorktreeOperationAck {
  operation_id: string;
  accepted: boolean;
}

export interface WorktreeOperationDismissal {
  operation_id: string;
  dismissed: boolean;
}

const labels = {
  branch: 'Branch created',
  worktree: 'Worktree added',
  environment: '.env ported',
  herd: 'Herd parked',
  registered: 'Project registered'
} as const;

const store = writable<WorktreeOperation[]>([]);
const dismissedOperationIds = new Set<string>();
export const worktreeOperations = { subscribe: store.subscribe };

export function beginWorktreeOperation(input: BeginWorktreeOperation): WorktreeOperation {
  dismissedOperationIds.delete(input.id);
  const now = Date.now();
  const operation: WorktreeOperation = {
    id: input.id,
    mode: input.mode,
    source_project_id: input.sourceProjectId,
    repository_id: input.repositoryId,
    branch: input.branch ?? null,
    path: input.path ?? null,
    label: input.label ?? (
      input.mode === 'adopt' || input.mode === 'remove'
        ? basename(input.path ?? '') || (input.mode === 'remove' ? 'Removing project' : 'Adopting worktree')
        : input.branch || (input.mode === 'fork' ? 'Forking worktree' : 'Creating worktree')
    ),
    status: 'running',
    steps: initialSteps(input.mode),
    error: null,
    error_code: null,
    project: null,
    removal: null,
    created_at: now,
    updated_at: now,
    local: true
  };
  store.update((operations) => [operation, ...operations.filter((candidate) => candidate.id !== input.id)]);
  return operation;
}

export function replaceWorktreeOperations(serverOperations: WorktreeOperation[]): void {
  store.update((current) => {
    const localById = new Map(current.map((operation) => [operation.id, operation]));
    const visibleServerOperations = serverOperations.filter(
      (operation) => !dismissedOperationIds.has(operation.id)
    );
    const serverIds = new Set(visibleServerOperations.map((operation) => operation.id));
    const recentLocal = current.filter((operation) =>
      operation.local && !serverIds.has(operation.id) && Date.now() - operation.created_at < 120_000
    );
    return [
      ...visibleServerOperations.map((operation) => {
        const local = localById.get(operation.id);
        return {
          ...operation,
          source_project_id: operation.source_project_id ?? local?.source_project_id ?? null,
          repository_id: operation.repository_id ?? local?.repository_id ?? null,
          branch: operation.branch ?? local?.branch ?? null,
          path: operation.path ?? local?.path ?? null,
          label: local?.label ?? operation.label,
          local: false
        };
      }),
      ...recentLocal
    ].sort((left, right) => right.updated_at - left.updated_at);
  });
}

export function failWorktreeOperation(id: string, message: string): void {
  store.update((operations) => operations.map((operation) => {
    if (operation.id !== id) return operation;
    const steps = operation.steps.map((step) =>
      step.status === 'running' ? { ...step, status: 'failed' as const, detail: message } : step
    );
    return { ...operation, status: 'failed', error: message, steps, updated_at: Date.now() };
  }));
}

export function dismissWorktreeOperation(
  id: string,
  dismissRemote?: (operationId: string) => Promise<unknown>
): void {
  dismissedOperationIds.add(id);
  store.update((operations) => operations.filter((operation) => operation.id !== id));
  void dismissRemote?.(id).catch(() => undefined);
}

export function resetWorktreeOperations(): void {
  dismissedOperationIds.clear();
  store.set([]);
}

export function worktreeOperationMatchesProject(
  operation: WorktreeOperation,
  project: Project
): boolean {
  if (operation.mode === 'remove') {
    return operation.source_project_id === project.id
      || operation.removal?.project_id === project.id;
  }
  if (operation.project?.id === project.id) return true;
  return operation.path !== null
    && normalizePath(operation.path) === normalizePath(project.path);
}

export function worktreeOperationForProject(
  operations: readonly WorktreeOperation[],
  project: Project
): WorktreeOperation | null {
  return operations.find((operation) => worktreeOperationMatchesProject(operation, project)) ?? null;
}

export function standaloneWorktreeOperations(
  operations: readonly WorktreeOperation[],
  projects: readonly Project[]
): WorktreeOperation[] {
  return operations.filter((operation) =>
    !projects.some((project) => worktreeOperationMatchesProject(operation, project))
  );
}

export function worktreeOperationStateLabel(operation: WorktreeOperation): string {
  if (operation.status === 'failed') {
    if (operation.mode === 'remove') return 'Removal failed';
    if (operation.mode === 'fork') return 'Fork failed';
    if (operation.mode === 'adopt') return 'Adoption failed';
    return 'Creation failed';
  }
  if (operation.status === 'completed') {
    return operation.mode === 'remove' ? 'Removed' : 'Ready';
  }
  if (operation.mode === 'remove') return 'Removing…';
  if (operation.mode === 'fork') return 'Forking…';
  if (operation.mode === 'adopt') return 'Adopting…';
  return 'Creating…';
}

function initialSteps(mode: WorktreeOperationMode): WorktreeOperationStep[] {
  if (mode === 'remove') {
    return [
      ['processes', 'Processes stopped'],
      ['worktree', 'Git worktree removed'],
      ['files', 'Local files deleted'],
      ['prune', 'Metadata pruned'],
      ['registered', 'Project unregistered']
    ].map(([id, label], index) => ({
      id: id as WorktreeStepId,
      label,
      status: index === 0 ? 'running' : 'pending',
      detail: null
    }));
  }
  return (Object.keys(labels) as Array<keyof typeof labels>).map((id, index) => ({
    id,
    label: mode === 'adopt' && id === 'branch' ? 'Worktree inspected' : labels[id],
    status: index === 0 ? 'running' : 'pending',
    detail: null
  }));
}

function basename(path: string): string {
  return path.replace(/[\\/]+$/, '').split(/[\\/]/).pop() ?? '';
}

function normalizePath(path: string): string {
  const normalized = path.replace(/\\/g, '/').replace(/\/+$/, '');
  return normalized || '/';
}
