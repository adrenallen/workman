import { writable } from 'svelte/store';

import type { Project } from './daemon';

export type WorktreeOperationMode = 'create' | 'fork' | 'adopt';
export type WorktreeOperationStatus = 'pending' | 'running' | 'completed' | 'failed';
export type WorktreeStepId = 'branch' | 'worktree' | 'environment' | 'herd' | 'registered';
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
  project: Project | null;
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
}

export interface WorktreeOperationAck {
  operation_id: string;
  accepted: boolean;
}

export interface WorktreeOperationDismissal {
  operation_id: string;
  dismissed: boolean;
}

const labels: Record<WorktreeStepId, string> = {
  branch: 'Branch created',
  worktree: 'Worktree added',
  environment: '.env ported',
  herd: 'Herd parked',
  registered: 'Project registered'
};

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
    label: input.mode === 'adopt'
      ? basename(input.path ?? '') || 'Adopting worktree'
      : input.branch || (input.mode === 'fork' ? 'Forking worktree' : 'Creating worktree'),
    status: 'running',
    steps: initialSteps(input.mode),
    error: null,
    project: null,
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

function initialSteps(mode: WorktreeOperationMode): WorktreeOperationStep[] {
  return (Object.keys(labels) as WorktreeStepId[]).map((id, index) => ({
    id,
    label: mode === 'adopt' && id === 'branch' ? 'Worktree inspected' : labels[id],
    status: index === 0 ? 'running' : 'pending',
    detail: null
  }));
}

function basename(path: string): string {
  return path.replace(/[\\/]+$/, '').split(/[\\/]/).pop() ?? '';
}
