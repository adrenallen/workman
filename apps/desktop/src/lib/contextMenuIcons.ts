export const CONTEXT_ACTION_IDS = [
  'select',
  'project-settings',
  'start',
  'stop',
  'restart',
  'kill',
  'close',
  'edit-command',
  'remove-command',
  'rename',
  'copy-name',
  'copy-id',
  'send-prompt',
  'view-parent',
  'mark-read',
  'reveal-config',
  'complete-todo',
  'reopen-todo',
  'copy-title',
  'archive-scratchpad',
  'delete-scratchpad',
  'new-agent',
  'new-terminal',
  'add-command',
  'new-todo',
  'new-scratchpad',
  'start-all-commands',
  'stop-all-commands',
  'remove-project',
  'open-in-editor',
  'open-in-finder',
  'open-custom',
  'copy-path',
  'new-worktree',
  'adopt-worktree',
  'import-worktrees',
  'fork-worktree',
  'remove-worktree',
  'refresh-worktrees',
  'refresh-pull-request',
  'open-pull-request',
  'open-herd-site',
  'terminal-copy',
  'terminal-paste',
  'terminal-open-link',
  'terminal-copy-link',
  'terminal-select-all'
] as const;

export type ContextActionId = (typeof CONTEXT_ACTION_IDS)[number];

export type ContextActionIcon =
  | 'archive'
  | 'bot'
  | 'check'
  | 'circle-check'
  | 'clipboard-paste'
  | 'clipboard'
  | 'copy'
  | 'external-link'
  | 'file-code'
  | 'folder'
  | 'git-branch'
  | 'git-branch-plus'
  | 'git-fork'
  | 'import'
  | 'link'
  | 'message-square'
  | 'notebook-text'
  | 'pencil'
  | 'play'
  | 'refresh-cw'
  | 'settings'
  | 'square'
  | 'square-terminal'
  | 'trash-2';

export const DESTRUCTIVE_CONTEXT_ACTION_IDS = [
  'kill',
  'close',
  'remove-command',
  'delete-scratchpad',
  'remove-project',
  'remove-worktree'
] as const satisfies readonly ContextActionId[];

const CONTEXT_ACTION_ICONS: Record<ContextActionId, ContextActionIcon> = {
  select: 'check',
  'project-settings': 'settings',
  start: 'play',
  stop: 'square',
  restart: 'refresh-cw',
  kill: 'trash-2',
  close: 'trash-2',
  'edit-command': 'pencil',
  'remove-command': 'trash-2',
  rename: 'pencil',
  'copy-name': 'clipboard',
  'copy-id': 'clipboard',
  'send-prompt': 'message-square',
  'view-parent': 'git-branch',
  'mark-read': 'check',
  'reveal-config': 'file-code',
  'complete-todo': 'circle-check',
  'reopen-todo': 'circle-check',
  'copy-title': 'clipboard',
  'archive-scratchpad': 'archive',
  'delete-scratchpad': 'trash-2',
  'new-agent': 'bot',
  'new-terminal': 'square-terminal',
  'add-command': 'play',
  'new-todo': 'circle-check',
  'new-scratchpad': 'notebook-text',
  'start-all-commands': 'play',
  'stop-all-commands': 'square',
  'remove-project': 'trash-2',
  'open-in-editor': 'external-link',
  'open-in-finder': 'folder',
  'open-custom': 'external-link',
  'copy-path': 'clipboard',
  'new-worktree': 'git-branch-plus',
  'adopt-worktree': 'import',
  'import-worktrees': 'import',
  'fork-worktree': 'git-fork',
  'remove-worktree': 'trash-2',
  'refresh-worktrees': 'refresh-cw',
  'refresh-pull-request': 'refresh-cw',
  'open-pull-request': 'external-link',
  'open-herd-site': 'external-link',
  'terminal-copy': 'copy',
  'terminal-paste': 'clipboard-paste',
  'terminal-open-link': 'external-link',
  'terminal-copy-link': 'link',
  'terminal-select-all': 'check'
};

export function contextActionIcon(id: ContextActionId): ContextActionIcon {
  return CONTEXT_ACTION_ICONS[id];
}
