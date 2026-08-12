import type { Project, ProjectStatus } from './daemon';

export type WorktreeKind = 'main' | 'managed' | 'adopted' | 'external';
export type WorktreeStatus = 'clean' | 'dirty' | 'missing' | 'bare';
export type PullRequestState = 'draft' | 'open' | 'merged' | 'closed';
export type PullRequestChecks = 'none' | 'pending' | 'passing' | 'failing';
export type PullRequestMergeability = 'mergeable' | 'conflicting' | 'unknown';
export type EnvironmentPolicy = 'copy' | 'skip';

export interface HerdStatus {
  available: boolean;
  parked: boolean;
  tld: string | null;
  error: string | null;
}

export interface WorktreeRepository {
  id: number;
  name: string;
  root_path: string;
  managed_root: string;
  preferences: Record<string, string>;
  herd: HerdStatus;
}

export interface PullRequestStatus {
  number: number;
  state: PullRequestState;
  url: string;
  checks: PullRequestChecks;
  mergeable: PullRequestMergeability;
}

export interface PullRequestCache {
  available: boolean;
  checked_at: number | null;
  expires_at: number | null;
  error: string | null;
}

export interface WorktreeEntry {
  project_id: number | null;
  parent_project_id: number | null;
  path: string;
  branch: string;
  head: string;
  kind: WorktreeKind;
  status: WorktreeStatus;
  managed: boolean;
  registered: boolean;
  can_adopt: boolean;
  can_remove: boolean;
  delete_safety: WorktreeDeleteSafety | null;
  locked: boolean;
  prunable: boolean;
  site_url: string | null;
  pull_request: PullRequestStatus | null;
}

export interface WorktreeDeleteSafety {
  dirty_files: number;
  untracked_files: number;
  dirty_paths: string[];
  ignored_files: number;
  ignored_paths: string[];
  unpushed_commits: number;
  unmerged_commits: number;
  upstream: string | null;
  push_target: string | null;
  merge_target: string;
  dependent_worktrees: string[];
  requires_force: boolean;
}

export interface WorktreeList {
  repository: WorktreeRepository;
  worktrees: WorktreeEntry[];
  pull_requests: PullRequestCache;
}

export interface OriginBranchList {
  repository_id: number;
  branches: string[];
  options?: WorktreeBranchOption[];
}

export interface WorktreeBranchOption {
  name: string;
  source: 'local' | 'origin';
}

export interface WorktreeMutation {
  repository: WorktreeRepository;
  project: Project;
  worktree: WorktreeEntry;
  environment?: {
    policy: EnvironmentPolicy;
    copied: boolean;
    remembered: boolean;
    source?: string | null;
    destination?: string | null;
  } | null;
}

export interface WorktreeRemoval {
  project_id: number;
  path: string;
  branch: string;
  removed: boolean;
  project_unregistered: boolean;
  deleted_from_disk: boolean;
  metadata_pruned: boolean;
  branch_kept: boolean;
}

export interface CreateWorktreeInput {
  project_id: number;
  branch: string;
  from_ref?: string;
  env_policy: EnvironmentPolicy;
  remember_env_policy: boolean;
}

export interface ForkWorktreeInput {
  project_id: number;
  branch: string;
  env_policy: EnvironmentPolicy;
  remember_env_policy: boolean;
}

export interface RemoveWorktreeInput {
  project_id: number;
  confirm_remove: true;
  confirm_stop_running: boolean;
  delete_from_disk: boolean;
  force_dirty: boolean;
  confirm_branch?: string;
}

export type WorktreeDialogSubmission =
  | {
      mode: 'create';
      branch: string;
      fromRef?: string;
      envPolicy: EnvironmentPolicy;
      rememberEnvPolicy: boolean;
    }
  | {
      mode: 'fork';
      branch: string;
      envPolicy: EnvironmentPolicy;
      rememberEnvPolicy: boolean;
    }
  | { mode: 'adopt'; path: string };

/**
 * Seed the flat rail from the former nested presentation without deriving its order again later.
 * Root order and each root's child order stay stable; orphaned worktrees remain visible in place.
 */
export function initialFlatProjectOrder(projects: Project[]): number[] {
  const projectIds = new Set(projects.map((project) => project.id));
  const childrenByParent = new Map<number, Project[]>();
  for (const project of projects) {
    if (project.parent_project_id === null || !projectIds.has(project.parent_project_id)) continue;
    const children = childrenByParent.get(project.parent_project_id) ?? [];
    children.push(project);
    childrenByParent.set(project.parent_project_id, children);
  }

  const orderedIds: number[] = [];
  const added = new Set<number>();
  for (const project of projects) {
    if (project.parent_project_id !== null && projectIds.has(project.parent_project_id)) continue;
    orderedIds.push(project.id);
    added.add(project.id);
    for (const child of childrenByParent.get(project.id) ?? []) {
      orderedIds.push(child.id);
      added.add(child.id);
    }
  }
  for (const project of projects) {
    if (!added.has(project.id)) orderedIds.push(project.id);
  }
  return orderedIds;
}

/** Resolve the quiet parent label used by a flat worktree row, including orphan fallbacks. */
export function worktreeParentLabel(
  project: Project,
  projects: Project[],
  repositoryName?: string | null
): string | null {
  if (project.parent_project_id === null) return null;
  const parent = projects.find((candidate) => candidate.id === project.parent_project_id);
  if (parent) return projectDisplayName(parent);
  return repositoryName?.trim() || repositoryNameFromProject(project);
}

export function projectBranchLabel(project: Project): string {
  return projectDisplayName(project, project.branch?.trim() || project.name);
}

export function projectRepositoryTitle(project: Project, repository?: WorktreeRepository | null): string {
  const displayName = project.display_name?.trim();
  if (displayName) return displayName;
  if (!project.branch) return project.name;
  return `${repository?.name ?? repositoryNameFromProject(project)}: ${project.branch}`;
}

export function projectDisplayName(project: Project, fallback = project.name): string {
  return project.display_name?.trim() || fallback;
}

export function repositoryNameFromProject(project: Project): string {
  const separator = project.name.indexOf(': ');
  return separator > 0 ? project.name.slice(0, separator) : project.name;
}

export function rollupProjectStatus(projects: Project[]): ProjectStatus {
  if (projects.some((project) => project.status === 'error')) return 'error';
  if (projects.some((project) => project.status === 'running')) return 'running';
  return 'idle';
}

export function projectStatusRollupLabel(repository: string, projects: Project[]): string {
  const running = projects.filter((project) => project.status === 'running').length;
  const errors = projects.filter((project) => project.status === 'error').length;
  return `${repository} · ${running} running · ${errors} with errors · ${projects.length} workspaces`;
}

export function pullRequestLabel(pullRequest: PullRequestStatus): string {
  const state = pullRequest.state === 'draft' ? 'draft' : pullRequest.state;
  return `Pull request #${pullRequest.number} ${state} · ${pullRequestDetail(pullRequest)}`;
}

export function pullRequestDetail(pullRequest: PullRequestStatus): string {
  const checks = pullRequest.checks === 'none' ? 'No checks reported' : `Checks ${pullRequest.checks}`;
  if (pullRequest.state === 'merged' || pullRequest.state === 'closed') return checks;
  const mergeability = pullRequest.mergeable === 'unknown'
    ? 'Mergeability unknown'
    : pullRequest.mergeable === 'conflicting'
      ? 'Merge conflicts detected'
      : 'Mergeable';
  return `${checks} · ${mergeability}`;
}

export function pullRequestTone(pullRequest: PullRequestStatus): 'success' | 'warning' | 'danger' | 'neutral' {
  if (pullRequest.state === 'merged' || pullRequest.state === 'closed') return 'neutral';
  if (pullRequest.checks === 'failing' || pullRequest.mergeable === 'conflicting') return 'danger';
  if (pullRequest.checks === 'pending' || pullRequest.state === 'draft') return 'warning';
  return pullRequest.checks === 'passing' ? 'success' : 'neutral';
}
