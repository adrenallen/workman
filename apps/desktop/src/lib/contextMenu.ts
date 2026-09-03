import { invoke } from '@tauri-apps/api/core';
import { get } from 'svelte/store';

import type { ScratchpadSummary, TodoSummary } from './coordination';
import type { ProcessView, Project } from './daemon';
import type { CreationDraft } from './creationDrafts';
import {
  customActionLabel,
  editorActionLabel,
  type OpenersState
} from './openers';
import type { ProjectTreeSelection } from './projectTree';
import type { RecordedFeedbackSummary } from './recordedFeedback';
import { hotkeyPreferences, matchesHotkeyAction } from './hotkeys';
import { projectFrequentActions } from './projectMenu';
import type { PullRequestState, WorktreeEntry, WorktreeRepository } from './worktrees';
import { projectRepositoryTitle } from './worktrees';
import type { ContextActionIcon, ContextActionId } from './contextMenuIcons';
import {
  terminalContextMenuItems,
  type TerminalContextActionId
} from './terminalContextMenu';

export {
  CONTEXT_ACTION_IDS,
  DESTRUCTIVE_CONTEXT_ACTION_IDS,
  contextActionIcon
} from './contextMenuIcons';
export type { ContextActionIcon, ContextActionId } from './contextMenuIcons';
export type { TerminalContextActionId } from './terminalContextMenu';

export interface ContextMenuItem {
  id: ContextActionId;
  label: string;
  detail?: string;
  shortcut?: string;
  disabled?: boolean;
  destructive?: boolean;
  tone?: ContextMenuTone;
  separatorBefore?: boolean;
  pullRequestState?: PullRequestState;
}

export type ContextMenuTone = 'default' | 'positive' | 'warning' | 'info' | 'danger';

export interface ContextMenuSubmenu {
  kind: 'submenu';
  label: string;
  detail?: string;
  icon: ContextActionIcon;
  items: ContextMenuItem[];
  separatorBefore?: boolean;
}

export type ContextMenuEntry = ContextMenuItem | ContextMenuSubmenu;

export type ContextMenuTarget =
  | {
      kind: 'project';
      project: Project;
      repository?: WorktreeRepository | null;
      worktree?: WorktreeEntry | null;
      importableWorktreeCount?: number;
      hasUnread?: boolean;
    }
  | { kind: 'process'; process: ProcessView; selection: ProjectTreeSelection }
  | {
      kind: 'terminal';
      process: Pick<ProcessView, 'id' | 'kind' | 'name'>;
      hasSelection: boolean;
      link: string | null;
      pasteEnabled: boolean;
    }
  | { kind: 'todo'; todo: ContextTodo; selection: ProjectTreeSelection }
  | { kind: 'draft'; draft: CreationDraft; selection: ProjectTreeSelection }
  | { kind: 'feedback'; feedback: ContextFeedback; selection: ProjectTreeSelection }
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
  items: ContextMenuEntry[];
}

export type ShellOpenTarget = 'editor' | 'finder' | 'reveal';

export type ContextTodo = Pick<TodoSummary, 'id' | 'title' | 'completed'>;
export type ContextFeedback = Pick<RecordedFeedbackSummary, 'id' | 'title' | 'archived'>;
export type ContextScratchpad = Pick<
  ScratchpadSummary,
  'id' | 'name' | 'revision' | 'archived'
>;

export const FOCUS_TERMINAL_EVENT = 'workman:focus-terminal';
export const TERMINAL_CONTEXT_ACTION_EVENT = 'workman:terminal-context-action';

export interface TerminalContextActionDetail {
  action: TerminalContextActionId;
  processId: number;
  link: string | null;
}

export function openWorkspacePath(path: string, target: ShellOpenTarget): Promise<void> {
  return invoke('shell_open_path', { path, target });
}

export function focusTerminalInput(processId: number): void {
  window.dispatchEvent(
    new CustomEvent(FOCUS_TERMINAL_EVENT, { detail: { processId } })
  );
}

export function dispatchTerminalContextAction(
  action: ContextActionId,
  target: Extract<ContextMenuTarget, { kind: 'terminal' }>
): void {
  if (!action.startsWith('terminal-')) return;
  window.dispatchEvent(new CustomEvent<TerminalContextActionDetail>(
    TERMINAL_CONTEXT_ACTION_EVENT,
    {
      detail: {
        action: action as TerminalContextActionId,
        processId: target.process.id,
        link: target.link
      }
    }
  ));
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
  if (!matchesHotkeyAction(event, 'open-context-menu', get(hotkeyPreferences))) return null;
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
    case 'terminal':
      return {
        title: target.process.name,
        subtitle: `TERMINAL · ${target.process.id}`,
        items: terminalItems(target)
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
    case 'draft':
      return {
        title: target.selection.label,
        subtitle: `${target.draft.kind.toUpperCase()} DRAFT`,
        items: [
          {
            id: 'discard-draft',
            label: 'Discard draft…',
            destructive: true
          }
        ]
      };
    case 'feedback':
      return {
        title: target.feedback.title,
        subtitle: `FEEDBACK · ${target.feedback.id}`,
        items: [
          {
            id: 'archive-feedback',
            label: target.feedback.archived ? 'Restore feedback' : 'Archive feedback'
          },
          { id: 'copy-title', label: 'Copy title' },
          {
            id: 'delete-feedback',
            label: 'Delete feedback…',
            destructive: true,
            separatorBefore: true
          }
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

function terminalItems(
  target: Extract<ContextMenuTarget, { kind: 'terminal' }>
): ContextMenuItem[] {
  return terminalContextMenuItems(target);
}

function projectItems(
  target: Extract<ContextMenuTarget, { kind: 'project' }>,
  openers: OpenersState | null
): ContextMenuEntry[] {
  const { project, repository, worktree } = target;
  const frequentItems: ContextMenuItem[] = projectFrequentActions({
    editorLabel: openers
      ? editorActionLabel(openers.config, openers.editors)
      : 'Open in editor',
    pullRequest: worktree?.pull_request,
    siteUrl: worktree?.site_url
  });
  const customOpenerItem: ContextMenuItem[] = openers?.config.sidebar.customEnabled
    ? [{ id: 'open-custom', label: customActionLabel(openers.config) }]
    : [];

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
    worktreeItems.push({ id: 'refresh-pull-request', label: 'Refresh pull request status' });
  }

  const removal: ContextMenuItem = {
    id: 'remove-project',
    label: 'Remove project…',
    detail: 'Keeps files unless local deletion is selected',
    destructive: true,
    tone: 'danger',
    separatorBefore: true
  };

  const createItems: ContextMenuItem[] = [
    { id: 'new-terminal', label: 'New terminal' },
    { id: 'add-command', label: 'Add command…' },
    { id: 'new-todo', label: 'New todo…' },
    { id: 'new-scratchpad', label: 'New scratchpad' }
  ];

  const commandItems: ContextMenuItem[] = [
    {
      id: 'start-all-commands',
      label: 'Start all commands',
      detail: 'Runs every command in this project',
      tone: 'positive'
    },
    {
      id: 'stop-all-commands',
      label: 'Stop all commands',
      detail: 'Requests a graceful stop',
      tone: 'warning'
    }
  ];
  const projectMoreItems: ContextMenuItem[] = [
    { id: 'project-settings', label: 'Project settings…' },
    { id: 'rename', label: 'Rename project' },
    ...(target.hasUnread
      ? [{
          id: 'mark-read',
          label: 'Mark as read',
          detail: 'Clears notifications for this project'
        } satisfies ContextMenuItem]
      : []),
    { id: 'copy-path', label: 'Copy path' }
  ];

  return [
    ...frequentItems,
    ...customOpenerItem,
    ...(!project.selected
      ? [{ id: 'select', label: 'Select project', separatorBefore: true } satisfies ContextMenuItem]
      : []),
    {
      id: 'new-agent',
      label: 'New agent…',
      detail: 'Start a coding agent in this project',
      tone: 'positive',
      separatorBefore: project.selected
    },
    {
      kind: 'submenu',
      label: 'Create',
      icon: 'plus',
      items: createItems
    },
    ...(worktreeItems.length > 0
      ? [{
          kind: 'submenu',
          label: 'Worktrees',
          icon: 'git-branch',
          items: worktreeItems
        } satisfies ContextMenuSubmenu]
      : []),
    {
      kind: 'submenu',
      label: 'Commands',
      icon: 'square-terminal',
      items: commandItems
    },
    {
      kind: 'submenu',
      label: 'More',
      icon: 'ellipsis',
      items: projectMoreItems,
      separatorBefore: true
    },
    removal
  ];
}

function processItems(process: ProcessView): ContextMenuEntry[] {
  const running = process.status === 'running' || process.status === 'starting';
  const items: ContextMenuEntry[] = [];

  if (running) {
    items.push({
      id: 'stop',
      label: 'Stop',
      detail: 'Graceful · stays in the sidebar',
      tone: 'warning'
    });
    items.push({ id: 'restart', label: 'Restart', tone: 'info' });
    items.push({
      id: 'kill',
      label: 'Force stop…',
      detail: 'Use if Stop is not responding',
      destructive: true,
      tone: 'danger'
    });
  } else {
    items.push({
      id: 'start',
      label: process.kind === 'command' ? 'Run' : 'Start',
      detail: 'Starts this process again',
      tone: 'positive'
    });
  }

  if (process.kind === 'agent') {
    items.push({
      id: 'send-prompt',
      label: 'Send prompt',
      disabled: !running,
      separatorBefore: true
    });
    if (process.agent_state.unread) {
      items.push({
        id: 'mark-read',
        label: 'Mark read',
        detail: 'Clears the finished-agent notification'
      });
    }
  }

  if (process.kind === 'command') {
    items.push({ id: 'edit-command', label: 'Edit command…', separatorBefore: true });
  }

  const moreItems: ContextMenuItem[] = [];
  if (process.kind === 'agent' && process.spawned_by_process_id !== null) {
    moreItems.push({ id: 'view-parent', label: 'View parent' });
  }
  if (process.kind !== 'command') {
    moreItems.push({ id: 'rename', label: 'Rename' });
  }
  if (process.kind === 'command' && process.source === 'yml') {
    moreItems.push({ id: 'reveal-config', label: 'Reveal in workman.yml' });
  }
  moreItems.push({ id: 'copy-name', label: 'Copy name' });
  moreItems.push({ id: 'copy-id', label: 'Copy process ID' });
  items.push({
    kind: 'submenu',
    label: 'More',
    icon: 'ellipsis',
    items: moreItems,
    separatorBefore: process.kind !== 'command'
  });

  if (process.kind === 'command') {
    items.push({
      id: 'remove-command',
      label: 'Remove command…',
      destructive: true,
      tone: 'danger',
      separatorBefore: true
    });
  }

  if (process.kind === 'agent' || process.kind === 'terminal') {
    items.push({
      id: 'close',
      label: `Remove ${process.kind}…`,
      detail: running
        ? 'Stops it first, then removes it from the sidebar'
        : 'Removes its saved sidebar entry',
      destructive: true,
      tone: 'danger',
      separatorBefore: true
    });
  }

  return items;
}
