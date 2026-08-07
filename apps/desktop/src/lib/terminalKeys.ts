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

export function encodeTerminalKey(
  event: TerminalKeyEvent,
  mode: TerminalKeyboardMode
): string | null {
  const kittyKeyboardActive = (mode.kittyFlags & 1) !== 0;
  const optionOnly = event.altKey && !event.ctrlKey && !event.metaKey && !event.shiftKey;

  if (optionOnly) {
    switch (event.key) {
      case 'ArrowLeft':
        return kittyKeyboardActive || mode.modifyOtherKeys !== 0 ? '\x1b[1;3D' : '\x1bb';
      case 'ArrowRight':
        return kittyKeyboardActive || mode.modifyOtherKeys !== 0 ? '\x1b[1;3C' : '\x1bf';
      case 'Backspace':
        if (kittyKeyboardActive) return '\x1b[127;3u';
        if (mode.modifyOtherKeys !== 0) return '\x1b[27;3;127~';
        return '\x1b\x7f';
    }
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
