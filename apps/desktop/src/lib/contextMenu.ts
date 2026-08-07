import { invoke } from '@tauri-apps/api/core';

import type { ScratchpadSummary, TodoSummary } from './coordination';
import type { ProcessView, Project } from './daemon';
import {
  customActionLabel,
  editorActionLabel,
  type OpenersState
} from './openers';
import type { ProjectTreeSelection } from './projectTree';
import type { WorktreeEntry, WorktreeRepository } from './worktrees';
import { projectRepositoryTitle } from './worktrees';
import type { ContextActionId } from './contextMenuIcons';

export {
  CONTEXT_ACTION_IDS,
  DESTRUCTIVE_CONTEXT_ACTION_IDS,
  contextActionIcon
} from './contextMenuIcons';
export type { ContextActionIcon, ContextActionId } from './contextMenuIcons';

export interface ContextMenuItem {
  id: ContextActionId;
  label: string;
  detail?: string;
  shortcut?: string;
  disabled?: boolean;
  destructive?: boolean;
  separatorBefore?: boolean;
}

export type ContextMenuTarget =
  | {
      kind: 'project';
      project: Project;
      repository?: WorktreeRepository | null;
      worktree?: WorktreeEntry | null;
      importableWorktreeCount?: number;
    }
  | { kind: 'process'; process: ProcessView; selection: ProjectTreeSelection }
  | { kind: 'todo'; todo: ContextTodo; selection: ProjectTreeSelection }
  | { kind: 'scratchpad'; scratchpad: ContextScratchpad; selection: ProjectTreeSelection };

export interface ContextMenuRequest {
  target: ContextMenuTarget;
  x: number;
  y: number;
  restoreFocus: HTMLElement | null;
}

export interface ContextMenuDescriptor {
  title: string;
  subtitle: string;
  items: ContextMenuItem[];
}

export type ShellOpenTarget = 'editor' | 'finder' | 'reveal';

export type ContextTodo = Pick<TodoSummary, 'id' | 'title' | 'completed'>;
export type ContextScratchpad = Pick<
  ScratchpadSummary,
  'id' | 'name' | 'revision' | 'archived'
>;

export const FOCUS_TERMINAL_EVENT = 'workman:focus-terminal';

export function openWorkspacePath(path: string, target: ShellOpenTarget): Promise<void> {
  return invoke('shell_open_path', { path, target });
}

export function focusTerminalInput(processId: number): void {
  window.dispatchEvent(
    new CustomEvent(FOCUS_TERMINAL_EVENT, { detail: { processId } })
  );
}

export function contextMenuRequest(
  event: MouseEvent,
  target: ContextMenuTarget
): ContextMenuRequest {
  event.preventDefault();
  event.stopPropagation();
  return {
    target,
    x: event.clientX,
    y: event.clientY,
    restoreFocus: event.currentTarget instanceof HTMLElement ? event.currentTarget : null
  };
}

export function keyboardContextMenuRequest(
  event: KeyboardEvent,
  target: ContextMenuTarget
): ContextMenuRequest | null {
  if (!event.shiftKey || event.key !== 'F10') return null;
  const anchor = event.currentTarget instanceof HTMLElement ? event.currentTarget : null;
  if (!anchor) return null;
  const bounds = anchor.getBoundingClientRect();
  event.preventDefault();
  event.stopPropagation();
  return {
    target,
    x: Math.min(bounds.left + 18, bounds.right - 8),
    y: Math.min(bounds.bottom - 3, window.innerHeight - 8),
    restoreFocus: anchor
  };
}

export function describeContextMenu(
  target: ContextMenuTarget,
  openers: OpenersState | null = null
): ContextMenuDescriptor {
  switch (target.kind) {
    case 'project':
      return {
        title: projectRepositoryTitle(target.project, target.repository),
        subtitle: target.worktree?.kind === 'main'
          ? `REPOSITORY · ${target.project.id}`
          : target.worktree
            ? `WORKTREE · ${target.worktree.kind.toUpperCase()}`
            : `PROJECT · ${target.project.id}`,
        items: projectItems(target, openers)
      };
    case 'process':
      return {
        title: target.process.name,
        subtitle: `${target.process.kind.toUpperCase()} · ${target.process.id}`,
        items: processItems(target.process)
      };
    case 'todo':
      return {
        title: target.todo.title,
        subtitle: `TODO · ${target.todo.id}`,
        items: [
          {
            id: target.todo.completed ? 'reopen-todo' : 'complete-todo',
            label: target.todo.completed ? 'Reopen todo' : 'Complete todo'
          },
          { id: 'copy-title', label: 'Copy title', separatorBefore: true }
        ]
      };
    case 'scratchpad':
      return {
        title: target.scratchpad.name,
        subtitle: `SCRATCHPAD · ${target.scratchpad.id}`,
        items: [
          { id: 'rename', label: 'Rename' },
          {
            id: 'archive-scratchpad',
            label: target.scratchpad.archived ? 'Archived' : 'Archive',
            disabled: target.scratchpad.archived
          },
          {
            id: 'delete-scratchpad',
            label: 'Delete scratchpad…',
            destructive: true,
            separatorBefore: true
          }
        ]
      };
  }
}

function projectItems(
  target: Extract<ContextMenuTarget, { kind: 'project' }>,
  openers: OpenersState | null
): ContextMenuItem[] {
  const { project, repository, worktree } = target;
  const openerItems: ContextMenuItem[] = openers ? [
    ...(openers.config.sidebar.editorEnabled
      ? [{ id: 'open-in-editor' as const, label: editorActionLabel(openers.config, openers.editors) }]
      : []),
    ...(openers.config.sidebar.finderEnabled
      ? [{ id: 'open-in-finder' as const, label: 'Show in Finder' }]
      : []),
    ...(openers.config.sidebar.customEnabled
      ? [{ id: 'open-custom' as const, label: customActionLabel(openers.config) }]
      : [])
  ] : [
    { id: 'open-in-editor', label: 'Open in editor' },
    { id: 'open-in-finder', label: 'Show in Finder' }
  ];
  if (openerItems[0]) openerItems[0].separatorBefore = true;

  const worktreeItems: ContextMenuItem[] = [];
  if (repository && worktree?.kind === 'main') {
    worktreeItems.push(
      { id: 'new-worktree', label: 'New worktree…', detail: `Managed under ${repository.managed_root}` },
      { id: 'adopt-worktree', label: 'Adopt existing…', detail: 'Register one folder in place' }
    );
    if ((target.importableWorktreeCount ?? 0) > 0) {
      const count = target.importableWorktreeCount!;
      worktreeItems.push({
        id: 'import-worktrees',
        label: 'Import worktrees…',
        detail: `${count} unregistered worktree${count === 1 ? '' : 's'} found`
      });
    }
    worktreeItems.push({ id: 'refresh-worktrees', label: 'Refresh worktrees and PRs' });
  } else if (repository && worktree) {
    worktreeItems.push({
      id: 'fork-worktree',
      label: 'Fork again…',
      detail: `Start at exact HEAD ${worktree.head.slice(0, 10)}`
    });
    if (worktree.site_url) {
      worktreeItems.push({ id: 'open-herd-site', label: 'Open Herd site', detail: worktree.site_url });
    }
    if (worktree.pull_request) {
      worktreeItems.push({
        id: 'open-pull-request',
        label: `Open pull request #${worktree.pull_request.number}`,
        detail: `${worktree.pull_request.state} · checks ${worktree.pull_request.checks}`
      });
    }
    worktreeItems.push({ id: 'refresh-pull-request', label: 'Refresh pull request status' });
  }

  const removal: ContextMenuItem = worktree?.can_remove
    ? {
        id: 'remove-worktree',
        label: 'Remove worktree…',
        detail: 'Deletes managed folder · keeps branch',
        destructive: true,
        separatorBefore: true
      }
    : {
        id: 'remove-project',
        label: 'Remove from Workman…',
        detail: 'Keeps files on disk',
        destructive: true,
        separatorBefore: true
      };

  return [
    { id: 'select', label: project.selected ? 'Selected project' : 'Select project', disabled: project.selected },
    { id: 'project-settings', label: 'Project settings…' },
    { id: 'rename', label: 'Rename' },
    { id: 'new-agent', label: 'New agent…', separatorBefore: true },
    { id: 'new-terminal', label: 'New terminal' },
    { id: 'add-command', label: 'Add command…' },
    { id: 'new-todo', label: 'New todo…' },
    { id: 'new-scratchpad', label: 'New scratchpad' },
    ...worktreeItems.map((item, index) => ({ ...item, separatorBefore: index === 0 })),
    { id: 'start-all-commands', label: 'Start all commands', separatorBefore: true },
    { id: 'stop-all-commands', label: 'Stop all commands' },
    ...openerItems,
    { id: 'copy-path', label: 'Copy path', separatorBefore: openerItems.length === 0 },
    removal
  ];
}

function processItems(process: ProcessView): ContextMenuItem[] {
  const running = process.status === 'running' || process.status === 'starting';
  const items: ContextMenuItem[] = [];

  if (running) {
    items.push({ id: 'stop', label: 'Stop' });
    items.push({ id: 'restart', label: 'Restart' });
    items.push({ id: 'kill', label: 'Kill immediately…', destructive: true });
  } else {
    items.push({ id: 'start', label: process.kind === 'command' ? 'Run' : 'Start' });
  }

  if (process.kind === 'agent') {
    items.push({
      id: 'send-prompt',
      label: 'Send prompt',
      disabled: !running,
      separatorBefore: true
    });
    if (process.spawned_by_process_id !== null) {
      items.push({ id: 'view-parent', label: 'View parent' });
    }
    if (process.agent_state.unread) {
      items.push({
        id: 'mark-read',
        label: 'Mark read',
        detail: 'Clears the finished-agent notification'
      });
    }
  }

  if (process.kind === 'command' && process.source === 'yml') {
    items.push({ id: 'reveal-config', label: 'Reveal in workman.yml', separatorBefore: true });
  }

  items.push({ id: 'rename', label: 'Rename', separatorBefore: process.kind !== 'command' });
  items.push({ id: 'copy-name', label: 'Copy name' });
  items.push({ id: 'copy-id', label: 'Copy process ID' });

  if (process.kind === 'agent' || process.kind === 'terminal') {
    items.push({
      id: 'close',
      label: `Close ${process.kind}…`,
      destructive: true,
      separatorBefore: true
    });
  }

  return items;
}
