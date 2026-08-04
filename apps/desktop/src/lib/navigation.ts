import { writable, type Readable } from 'svelte/store';

import type { CoordinationSnapshot } from './coordination';
import type { ProcessView } from './daemon';
import type { ProjectTreeSelection } from './projectTree';

export type AppNavigationTarget =
  | { type: 'project'; projectId: number }
  | { type: 'item'; selection: ProjectTreeSelection }
  | { type: 'settings'; projectId?: number }
  | { type: 'new-terminal'; projectId: number }
  | { type: 'spawn-agent'; projectId: number; agentToolId: number; agentToolName: string }
  | { type: 'add-command'; projectId: number }
  | { type: 'new-todo'; projectId: number }
  | { type: 'new-scratchpad'; projectId: number };

export type NavigationSource =
  | 'palette'
  | 'tree'
  | 'project-rail'
  | 'status-bar'
  | 'context-menu'
  | 'keyboard'
  | 'api';

export interface AppNavigationRequest {
  id: number;
  target: AppNavigationTarget;
  source: NavigationSource;
}

export interface AppNavigationState {
  request: AppNavigationRequest | null;
}

export interface NavigationProjectSnapshot {
  processes: ProcessView[];
  coordination: CoordinationSnapshot | null;
}

const recentStorageKey = 'gbuild.navigation.recents.v1';
const recentLimit = 20;

function createAppNavigation(): Readable<AppNavigationState> & {
  navigate: (target: AppNavigationTarget, source?: NavigationSource) => number;
  acknowledge: (requestId: number) => void;
} {
  const { subscribe, update } = writable<AppNavigationState>({ request: null });
  let sequence = 0;

  return {
    subscribe,
    navigate(target, source = 'api') {
      const id = ++sequence;
      update(() => ({ request: { id, target, source } }));
      return id;
    },
    acknowledge(requestId) {
      update((state) => (state.request?.id === requestId ? { request: null } : state));
    }
  };
}

export const appNavigation = createAppNavigation();

export function navigationTargetKey(target: AppNavigationTarget): string {
  switch (target.type) {
    case 'project':
      return `project:${target.projectId}`;
    case 'item':
      return `item:${target.selection.projectId}:${target.selection.kind}:${target.selection.id}`;
    case 'settings':
      return `settings:${target.projectId ?? 'current'}`;
    case 'new-terminal':
      return `new-terminal:${target.projectId}`;
    case 'spawn-agent':
      return `spawn-agent:${target.projectId}:${target.agentToolId}`;
    case 'add-command':
      return `add-command:${target.projectId}`;
    case 'new-todo':
      return `new-todo:${target.projectId}`;
    case 'new-scratchpad':
      return `new-scratchpad:${target.projectId}`;
  }
}

export function readRecentNavigationKeys(): string[] {
  try {
    const value = JSON.parse(localStorage.getItem(recentStorageKey) ?? '[]');
    return Array.isArray(value) ? value.filter((key): key is string => typeof key === 'string') : [];
  } catch {
    return [];
  }
}

export function recordRecentNavigation(target: AppNavigationTarget): void {
  if (target.type !== 'project' && target.type !== 'item') return;
  const key = navigationTargetKey(target);
  try {
    const next = [key, ...readRecentNavigationKeys().filter((candidate) => candidate !== key)].slice(
      0,
      recentLimit
    );
    localStorage.setItem(recentStorageKey, JSON.stringify(next));
  } catch {
    // Navigation stays functional when webview storage is unavailable.
  }
}

/**
 * Scores a case-insensitive subsequence match. Contiguous characters and word
 * boundaries are rewarded, while wide spans and long candidates are penalized.
 */
export function fuzzySubsequenceScore(query: string, candidate: string): number | null {
  const needle = query.trim().toLocaleLowerCase();
  const haystack = candidate.toLocaleLowerCase();
  if (!needle) return 0;

  let score = 0;
  let previous = -1;
  let first = -1;
  let cursor = 0;

  for (const character of needle) {
    const index = haystack.indexOf(character, cursor);
    if (index < 0) return null;
    if (first < 0) first = index;

    score += 10;
    if (index === previous + 1) score += 8;
    if (index === 0 || /[\s/_.:-]/.test(haystack[index - 1] ?? '')) score += 6;
    if (index === cursor) score += 2;

    previous = index;
    cursor = index + 1;
  }

  const span = previous - first + 1;
  score -= Math.max(0, span - needle.length) * 1.5;
  score -= Math.max(0, haystack.length - needle.length) * 0.04;
  if (first === 0) score += 5;
  return score;
}
