import { invoke } from '@tauri-apps/api/core';

import type { ScratchpadSummary, TodoSummary } from './coordination';
import type { ProcessView, Project } from './daemon';
import type { ProjectTreeSelection } from './projectTree';

export type ContextActionId =
  | 'select'
  | 'start'
  | 'stop'
  | 'restart'
  | 'kill'
  | 'close'
  | 'rename'
  | 'copy-name'
  | 'copy-id'
  | 'send-prompt'
  | 'view-parent'
  | 'reveal-config'
  | 'complete-todo'
  | 'reopen-todo'
  | 'copy-title'
  | 'archive-scratchpad'
  | 'delete-scratchpad'
  | 'start-all-commands'
  | 'stop-all-commands'
  | 'remove-project'
  | 'open-in-editor'
  | 'open-in-finder'
  | 'copy-path';

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
  | { kind: 'project'; project: Project }
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

export const FOCUS_TERMINAL_EVENT = 'gbuild:focus-terminal';

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

export function describeContextMenu(target: ContextMenuTarget): ContextMenuDescriptor {
  switch (target.kind) {
    case 'project':
      return {
        title: target.project.display_name?.trim() || target.project.name,
        subtitle: `PROJECT · ${target.project.id}`,
        items: projectItems(target.project)
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

function projectItems(project: Project): ContextMenuItem[] {
  return [
    { id: 'select', label: project.selected ? 'Selected project' : 'Select project', disabled: project.selected },
    { id: 'rename', label: 'Rename' },
    { id: 'start-all-commands', label: 'Start all commands', separatorBefore: true },
    { id: 'stop-all-commands', label: 'Stop all commands' },
    { id: 'open-in-editor', label: 'Open in editor', separatorBefore: true },
    { id: 'open-in-finder', label: 'Show in Finder' },
    { id: 'copy-path', label: 'Copy path' },
    {
      id: 'remove-project',
      label: 'Remove from gbuild…',
      detail: 'Keeps files on disk',
      destructive: true,
      separatorBefore: true
    }
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
  }

  if (process.kind === 'command' && process.source === 'yml') {
    items.push({ id: 'reveal-config', label: 'Reveal in gbuild.yml', separatorBefore: true });
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
