import type { ContextActionId } from './contextMenuIcons';

export type TerminalContextActionId = Extract<
  ContextActionId,
  | 'terminal-copy'
  | 'terminal-paste'
  | 'terminal-open-link'
  | 'terminal-copy-link'
  | 'terminal-select-all'
>;

export interface TerminalContextMenuState {
  hasSelection: boolean;
  link: string | null;
  pasteEnabled: boolean;
}

export interface TerminalContextMenuItem {
  id: TerminalContextActionId;
  label: string;
  shortcut?: string;
  disabled?: boolean;
  separatorBefore?: boolean;
}

export function terminalContextMenuItems(
  state: TerminalContextMenuState
): TerminalContextMenuItem[] {
  const items: TerminalContextMenuItem[] = [
    {
      id: 'terminal-copy',
      label: 'Copy',
      shortcut: '⌘C',
      disabled: !state.hasSelection
    },
    {
      id: 'terminal-paste',
      label: 'Paste',
      shortcut: '⌘V',
      disabled: !state.pasteEnabled
    }
  ];

  if (state.link) {
    items.push(
      { id: 'terminal-open-link', label: 'Open link', separatorBefore: true },
      { id: 'terminal-copy-link', label: 'Copy link URL' }
    );
  }

  items.push({
    id: 'terminal-select-all',
    label: 'Select all',
    shortcut: '⌘A',
    separatorBefore: true
  });
  return items;
}
