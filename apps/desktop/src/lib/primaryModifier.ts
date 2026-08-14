// The app-level shortcut modifier: Command on macOS, Control everywhere else.
// Terminal key encoding is deliberately untouched; PTY sequences keep reporting
// the physical modifiers a program expects.

const macLike = /Mac|iP(hone|ad|od)/.test(globalThis.navigator?.platform ?? '');

type ModifierEvent = Pick<KeyboardEvent, 'metaKey' | 'ctrlKey'>;

/** True when the platform's primary shortcut modifier is held. */
export function primaryModifier(event: ModifierEvent): boolean {
  return macLike ? event.metaKey : event.ctrlKey;
}

/**
 * True when the other platform's primary modifier is held. Shortcut sites
 * exclude it so Control keeps its terminal meaning on macOS and the Windows
 * key stays with the operating system elsewhere.
 */
export function secondaryModifier(event: ModifierEvent): boolean {
  return macLike ? event.ctrlKey : event.metaKey;
}

/** The labels shown for modifiers in shortcut help. */
export const primaryModifierLabel = macLike ? '⌘' : 'Ctrl';
export const altModifierLabel = macLike ? '⌥' : 'Alt';
export const shiftModifierLabel = macLike ? '⇧' : 'Shift';

/**
 * The terminal unfocus chord: ⌘U on macOS, Ctrl+Shift+U elsewhere — plain
 * Ctrl+U is shell line editing, and terminal input stays sovereign.
 */
export function terminalUnfocusChord(
  event: ModifierEvent & Pick<KeyboardEvent, 'altKey' | 'shiftKey' | 'key'>
): boolean {
  return (
    primaryModifier(event) &&
    !secondaryModifier(event) &&
    !event.altKey &&
    event.shiftKey !== macLike &&
    event.key.toLowerCase() === 'u'
  );
}

export const terminalUnfocusKeys = macLike ? ['⌘', 'U'] : ['Ctrl', 'Shift', 'U'];
