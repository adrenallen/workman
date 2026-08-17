import { primaryModifier, secondaryModifier } from './primaryModifier.ts';

export interface TerminalKeyboardMode {
  kittyFlags: number;
  modifyOtherKeys: number;
}

export interface TerminalKeyEvent {
  key: string;
  altKey: boolean;
  ctrlKey: boolean;
  metaKey: boolean;
  shiftKey: boolean;
}

export function processCycleDirection(event: TerminalKeyEvent): -1 | 1 | null {
  if (!primaryModifier(event) || event.altKey || secondaryModifier(event) || event.shiftKey) {
    return null;
  }
  if (event.key === 'ArrowUp') return -1;
  if (event.key === 'ArrowDown') return 1;
  return null;
}

interface MacOsEditingKeyBinding {
  key: string;
  altKey?: boolean;
  metaKey?: boolean;
  legacy: string;
  kitty: string;
  modifyOtherKeys: string;
}

// Ghostty's macOS defaults and iTerm2's Natural Text Editing preset express
// navigation as readline editing chords. Keep the physical-key match and every
// negotiated representation together so terminal modes cannot drift apart.
const MACOS_EDITING_KEY_BINDINGS: readonly MacOsEditingKeyBinding[] = [
  {
    key: 'ArrowLeft',
    altKey: true,
    legacy: '\x1bb',
    kitty: '\x1b[98;3u',
    modifyOtherKeys: '\x1b[27;3;98~'
  },
  {
    key: 'ArrowRight',
    altKey: true,
    legacy: '\x1bf',
    kitty: '\x1b[102;3u',
    modifyOtherKeys: '\x1b[27;3;102~'
  },
  {
    key: 'Backspace',
    altKey: true,
    legacy: '\x1b\x7f',
    kitty: '\x1b[127;3u',
    modifyOtherKeys: '\x1b[27;3;127~'
  },
  {
    key: 'Delete',
    altKey: true,
    legacy: '\x1bd',
    kitty: '\x1b[100;3u',
    modifyOtherKeys: '\x1b[27;3;100~'
  },
  {
    key: 'ArrowLeft',
    metaKey: true,
    legacy: '\x01',
    kitty: '\x1b[97;5u',
    modifyOtherKeys: '\x1b[27;5;97~'
  },
  {
    key: 'ArrowRight',
    metaKey: true,
    legacy: '\x05',
    kitty: '\x1b[101;5u',
    modifyOtherKeys: '\x1b[27;5;101~'
  },
  {
    key: 'Backspace',
    metaKey: true,
    legacy: '\x15',
    kitty: '\x1b[117;5u',
    modifyOtherKeys: '\x1b[27;5;117~'
  }
];

export function encodeTerminalKey(
  event: TerminalKeyEvent,
  mode: TerminalKeyboardMode
): string | null {
  const kittyKeyboardActive = (mode.kittyFlags & 1) !== 0;
  const editingBinding = MACOS_EDITING_KEY_BINDINGS.find((binding) =>
    event.key === binding.key
    && event.altKey === Boolean(binding.altKey)
    && event.metaKey === Boolean(binding.metaKey)
    && !event.ctrlKey
    && !event.shiftKey
  );
  if (editingBinding) {
    if (kittyKeyboardActive) return editingBinding.kitty;
    if (mode.modifyOtherKeys !== 0) return editingBinding.modifyOtherKeys;
    return editingBinding.legacy;
  }

  const codepoint = event.key === 'Enter' ? 13 : event.key === 'Tab' ? 9 : null;
  if (codepoint === null) return null;
  if (!event.shiftKey && !event.altKey && !event.ctrlKey && !event.metaKey) return null;

  const modifier = 1
    + Number(event.shiftKey)
    + 2 * Number(event.altKey)
    + 4 * Number(event.ctrlKey)
    + 8 * Number(event.metaKey);
  if (kittyKeyboardActive) return `\x1b[${codepoint};${modifier}u`;

  const modifyOtherKeysApplies = mode.modifyOtherKeys === 2
    || (mode.modifyOtherKeys === 1 && (event.altKey || event.metaKey));
  return modifyOtherKeysApplies ? `\x1b[27;${modifier};${codepoint}~` : null;
}
