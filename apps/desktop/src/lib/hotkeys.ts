import { writable } from 'svelte/store';

import {
  altModifierLabel,
  primaryModifier,
  primaryModifierLabel,
  secondaryModifier,
  shiftModifierLabel,
  terminalUnfocusKeys
} from './primaryModifier.ts';

export const projectHotkeyActions = [
  'project-1',
  'project-2',
  'project-3',
  'project-4',
  'project-5',
  'project-6',
  'project-7',
  'project-8',
  'project-9'
] as const;

export const creationHotkeyActions = [
  'new-agent',
  'new-terminal',
  'new-command',
  'new-scratchpad',
  'new-todo'
] as const;

export const workspaceHotkeyActions = [
  'previous-view',
  'quick-jump',
  'keyboard-reference',
  'open-settings',
  'toggle-project-rail',
  'toggle-project-tree',
  'quick-prompts'
] as const;

export const navigationHotkeyActions = [
  'navigate-left',
  'navigate-right',
  'previous-process',
  'next-process',
  'reorder-up',
  'reorder-down',
  'open-context-menu'
] as const;

export const terminalHotkeyActions = [
  'unfocus-terminal',
  'search-terminal'
] as const;

export const editingHotkeyActions = [
  'submit-focused-form',
  'toggle-scratchpad-list',
  'toggle-todo-inspector'
] as const;

export const feedbackLaunchHotkeyActions = [
  'start-feedback'
] as const;

export const recordingHotkeyActions = [
  'feedback-snap',
  'feedback-snap-region',
  'feedback-snap-display',
  'feedback-toggle-annotation',
  'feedback-undo',
  'feedback-clear',
  'feedback-toggle-pause',
  'feedback-toggle-mute',
  'feedback-finish'
] as const;

export const contextualHotkeyActions = [
  'new-quick-prompt'
] as const;

export type ProjectHotkeyAction = (typeof projectHotkeyActions)[number];
export type CreationHotkeyAction = (typeof creationHotkeyActions)[number];
export type WorkspaceHotkeyAction = (typeof workspaceHotkeyActions)[number];
export type NavigationHotkeyAction = (typeof navigationHotkeyActions)[number];
export type TerminalHotkeyAction = (typeof terminalHotkeyActions)[number];
export type EditingHotkeyAction = (typeof editingHotkeyActions)[number];
export type FeedbackLaunchHotkeyAction = (typeof feedbackLaunchHotkeyActions)[number];
export type RecordingHotkeyAction = (typeof recordingHotkeyActions)[number];
export type ContextualHotkeyAction = (typeof contextualHotkeyActions)[number];
export type HotkeyAction =
  | ProjectHotkeyAction
  | CreationHotkeyAction
  | WorkspaceHotkeyAction
  | NavigationHotkeyAction
  | TerminalHotkeyAction
  | EditingHotkeyAction
  | FeedbackLaunchHotkeyAction
  | RecordingHotkeyAction
  | ContextualHotkeyAction;

export interface HotkeyChord {
  code: string;
  primary: boolean;
  secondary: boolean;
  alt: boolean;
  shift: boolean;
}

export type HotkeyPreferences = Record<HotkeyAction, HotkeyChord | null>;

export interface HotkeyDefinition {
  id: HotkeyAction;
  label: string;
  description: string;
  group: 'workspace' | 'navigation' | 'terminal' | 'editing' | 'projects' | 'creation' | 'feedback';
}

export const hotkeyDefinitions: readonly HotkeyDefinition[] = [
  {
    id: 'previous-view',
    label: 'Previous view',
    description: 'Switch between the two most recent project views',
    group: 'workspace'
  },
  {
    id: 'quick-jump',
    label: 'Quick jump',
    description: 'Find or create something in any project',
    group: 'workspace'
  },
  {
    id: 'keyboard-reference',
    label: 'Keyboard reference',
    description: 'Show or close the keyboard reference',
    group: 'workspace'
  },
  {
    id: 'open-settings',
    label: 'Settings',
    description: 'Open application settings',
    group: 'workspace'
  },
  {
    id: 'toggle-project-rail',
    label: 'Toggle project rail',
    description: 'Collapse or expand the project rail',
    group: 'workspace'
  },
  {
    id: 'toggle-project-tree',
    label: 'Toggle project tree',
    description: 'Collapse or expand the current project tree',
    group: 'workspace'
  },
  {
    id: 'quick-prompts',
    label: 'Quick prompts',
    description: 'Open saved prompts for the selected agent',
    group: 'workspace'
  },
  {
    id: 'navigate-left',
    label: 'Navigate left',
    description: 'Move left across panels or adjacent details',
    group: 'navigation'
  },
  {
    id: 'navigate-right',
    label: 'Navigate right',
    description: 'Move right across panels or adjacent details',
    group: 'navigation'
  },
  {
    id: 'previous-process',
    label: 'Previous process',
    description: 'Select the previous process or process draft',
    group: 'navigation'
  },
  {
    id: 'next-process',
    label: 'Next process',
    description: 'Select the next process or process draft',
    group: 'navigation'
  },
  {
    id: 'reorder-up',
    label: 'Reorder up',
    description: 'Move the focused project or tree item up',
    group: 'navigation'
  },
  {
    id: 'reorder-down',
    label: 'Reorder down',
    description: 'Move the focused project or tree item down',
    group: 'navigation'
  },
  {
    id: 'open-context-menu',
    label: 'Open context menu',
    description: 'Open actions for the focused project or tree item',
    group: 'navigation'
  },
  {
    id: 'unfocus-terminal',
    label: 'Unfocus terminal',
    description: 'Return focus from terminal input to the project tree',
    group: 'terminal'
  },
  {
    id: 'search-terminal',
    label: 'Search terminal',
    description: 'Search the focused terminal buffer',
    group: 'terminal'
  },
  {
    id: 'submit-focused-form',
    label: 'Submit focused form',
    description: 'Create or save from the focused editor form',
    group: 'editing'
  },
  {
    id: 'toggle-scratchpad-list',
    label: 'Toggle scratchpad list',
    description: 'Show or hide the scratchpad list',
    group: 'editing'
  },
  {
    id: 'toggle-todo-inspector',
    label: 'Toggle todo inspector',
    description: 'Show or hide the todo inspector',
    group: 'editing'
  },
  {
    id: 'new-quick-prompt',
    label: 'New quick prompt',
    description: 'Create a prompt while the quick-prompt palette is open',
    group: 'editing'
  },
  {
    id: 'start-feedback',
    label: 'Start feedback',
    description: 'Open recorded feedback for the current project',
    group: 'feedback'
  },
  {
    id: 'feedback-snap',
    label: 'Snap last mode',
    description: 'Capture using the most recent region or display mode',
    group: 'feedback'
  },
  {
    id: 'feedback-snap-region',
    label: 'Snap region',
    description: 'Drag around part of a display to capture it',
    group: 'feedback'
  },
  {
    id: 'feedback-snap-display',
    label: 'Snap display',
    description: 'Capture the display containing the feedback toolbar',
    group: 'feedback'
  },
  {
    id: 'feedback-toggle-annotation',
    label: 'Toggle annotation',
    description: 'Switch between pointer and drawing mode',
    group: 'feedback'
  },
  {
    id: 'feedback-undo',
    label: 'Undo annotation',
    description: 'Remove the most recent screen annotation',
    group: 'feedback'
  },
  {
    id: 'feedback-clear',
    label: 'Clear annotations',
    description: 'Remove all current screen annotations',
    group: 'feedback'
  },
  {
    id: 'feedback-toggle-pause',
    label: 'Pause or resume feedback',
    description: 'Pause or resume the feedback timeline and microphone',
    group: 'feedback'
  },
  {
    id: 'feedback-toggle-mute',
    label: 'Mute or unmute microphone',
    description: 'Toggle microphone audio while the feedback timeline continues',
    group: 'feedback'
  },
  {
    id: 'feedback-finish',
    label: 'Stop feedback',
    description: 'Stop recording and begin local transcription',
    group: 'feedback'
  },
  ...projectHotkeyActions.map((id, index) => ({
    id,
    label: `Project ${index + 1}`,
    description: `Open project ${index + 1} in the project rail`,
    group: 'projects' as const
  })),
  {
    id: 'new-agent',
    label: 'New agent',
    description: 'Open a new agent draft in the current project',
    group: 'creation'
  },
  {
    id: 'new-terminal',
    label: 'New terminal',
    description: 'Start a terminal in the current project',
    group: 'creation'
  },
  {
    id: 'new-command',
    label: 'New command',
    description: 'Open a new command draft in the current project',
    group: 'creation'
  },
  {
    id: 'new-scratchpad',
    label: 'New scratchpad',
    description: 'Create a scratchpad in the current project',
    group: 'creation'
  },
  {
    id: 'new-todo',
    label: 'New todo',
    description: 'Open a new todo draft in the current project',
    group: 'creation'
  }
];

export const hotkeyStorageKey = 'workman.hotkeys.v1';

export const allHotkeyActions = [
  ...workspaceHotkeyActions,
  ...navigationHotkeyActions,
  ...terminalHotkeyActions,
  ...editingHotkeyActions,
  ...feedbackLaunchHotkeyActions,
  ...recordingHotkeyActions,
  ...projectHotkeyActions,
  ...creationHotkeyActions,
  // Context-only actions come last so globally active commands win shared chords.
  ...contextualHotkeyActions
] as const;
const legacyHotkeyActions = [...projectHotkeyActions, ...creationHotkeyActions] as const;
const supportedCodes = new Set([
  ...Array.from({ length: 26 }, (_, index) => `Key${String.fromCharCode(65 + index)}`),
  ...Array.from({ length: 10 }, (_, index) => `Digit${index}`),
  ...Array.from({ length: 12 }, (_, index) => `F${index + 1}`),
  'ArrowUp',
  'ArrowDown',
  'ArrowLeft',
  'ArrowRight',
  'Backquote',
  'Minus',
  'Equal',
  'BracketLeft',
  'BracketRight',
  'Backslash',
  'Semicolon',
  'Quote',
  'Comma',
  'Period',
  'Slash',
  'Enter',
  'Space',
  'Backspace',
  'Delete',
  'Home',
  'End',
  'PageUp',
  'PageDown'
]);

const codeLabels: Record<string, string> = {
  ArrowUp: '↑',
  ArrowDown: '↓',
  ArrowLeft: '←',
  ArrowRight: '→',
  Backquote: '`',
  Minus: '-',
  Equal: '=',
  BracketLeft: '[',
  BracketRight: ']',
  Backslash: '\\',
  Semicolon: ';',
  Quote: "'",
  Comma: ',',
  Period: '.',
  Slash: '/',
  Enter: '↵',
  Space: 'Space',
  Backspace: '⌫',
  Delete: 'Delete',
  Home: 'Home',
  End: 'End',
  PageUp: 'Page Up',
  PageDown: 'Page Down'
};

const secondaryModifierLabel = primaryModifierLabel === '⌘' ? '⌃' : 'Meta';
const defaultPreferences: HotkeyPreferences = Object.fromEntries([
  ['previous-view', primaryChord('Backquote')],
  ['quick-jump', primaryChord('KeyK')],
  ['keyboard-reference', primaryChord('Slash')],
  ['open-settings', primaryChord('Comma')],
  ['toggle-project-rail', primaryChord('KeyB')],
  ['toggle-project-tree', { ...primaryChord('KeyB'), shift: true }],
  ['quick-prompts', { ...primaryChord('KeyP'), shift: true }],
  ['navigate-left', primaryChord('ArrowLeft')],
  ['navigate-right', primaryChord('ArrowRight')],
  ['previous-process', primaryChord('ArrowUp')],
  ['next-process', primaryChord('ArrowDown')],
  ['reorder-up', altChord('ArrowUp')],
  ['reorder-down', altChord('ArrowDown')],
  ['open-context-menu', shiftChord('F10')],
  ['unfocus-terminal', terminalUnfocusChordValue()],
  ['search-terminal', primaryChord('KeyF')],
  ['submit-focused-form', primaryChord('Enter')],
  ['toggle-scratchpad-list', { ...primaryChord('KeyS'), shift: true }],
  ['toggle-todo-inspector', { ...primaryChord('KeyI'), shift: true }],
  ['start-feedback', { ...primaryChord('KeyF'), shift: true }],
  ['feedback-snap', { ...primaryChord('KeyC'), shift: true }],
  ['feedback-snap-region', { ...primaryChord('KeyR'), shift: true }],
  ['feedback-snap-display', { ...primaryChord('KeyD'), shift: true }],
  ['feedback-toggle-annotation', { ...primaryChord('KeyA'), shift: true }],
  ['feedback-undo', { ...primaryChord('KeyU'), shift: true }],
  ['feedback-clear', { ...primaryChord('Backspace'), shift: true }],
  ['feedback-toggle-pause', { ...primaryChord('Space'), shift: true }],
  ['feedback-toggle-mute', { ...primaryChord('KeyM'), shift: true }],
  ['feedback-finish', { ...primaryChord('Enter'), shift: true }],
  ...projectHotkeyActions.map((action, index) => [action, primaryChord(`Digit${index + 1}`)]),
  ['new-agent', primaryChord('KeyN')],
  ['new-terminal', primaryChord('KeyT')],
  ['new-command', null],
  ['new-scratchpad', null],
  ['new-todo', null],
  // This chord is intentionally shared with New agent. The prompt action exists only inside its
  // palette, where it handles the event before the global shortcut resolver.
  ['new-quick-prompt', primaryChord('KeyN')]
]) as HotkeyPreferences;

export const hotkeyPreferences = writable<HotkeyPreferences>(loadHotkeyPreferences());

export function defaultHotkeyPreferences(): HotkeyPreferences {
  return clonePreferences(defaultPreferences);
}

export function hotkeyActionLabel(action: HotkeyAction): string {
  return hotkeyDefinitions.find((definition) => definition.id === action)?.label ?? action;
}

export function projectHotkeyIndex(action: HotkeyAction): number | null {
  const index = projectHotkeyActions.indexOf(action as ProjectHotkeyAction);
  return index < 0 ? null : index;
}

export function hotkeyFromKeyboardEvent(event: KeyboardEvent): HotkeyChord | null {
  if (!supportedCodes.has(event.code)) return null;
  const chord: HotkeyChord = {
    code: event.code,
    primary: primaryModifier(event),
    secondary: secondaryModifier(event),
    alt: event.altKey,
    shift: event.shiftKey
  };
  // Only function keys can safely use Shift alone; ordinary Shift-only chords would steal text.
  return chord.primary || chord.secondary || chord.alt || (chord.shift && /^F\d+$/.test(chord.code))
    ? chord
    : null;
}

export function matchesHotkey(event: KeyboardEvent, chord: HotkeyChord | null): boolean {
  return chord !== null
    && event.code === chord.code
    && primaryModifier(event) === chord.primary
    && secondaryModifier(event) === chord.secondary
    && event.altKey === chord.alt
    && event.shiftKey === chord.shift;
}

export function findHotkeyAction(
  event: KeyboardEvent,
  preferences: HotkeyPreferences
): HotkeyAction | null {
  return allHotkeyActions.find((action) => matchesHotkey(event, preferences[action])) ?? null;
}

export function matchesHotkeyAction(
  event: KeyboardEvent,
  action: HotkeyAction,
  preferences: HotkeyPreferences
): boolean {
  return matchesHotkey(event, preferences[action]);
}

export function equalHotkey(left: HotkeyChord | null, right: HotkeyChord | null): boolean {
  return left === right || (
    left !== null
    && right !== null
    && left.code === right.code
    && left.primary === right.primary
    && left.secondary === right.secondary
    && left.alt === right.alt
    && left.shift === right.shift
  );
}

export function hotkeyDisplayParts(chord: HotkeyChord | null): string[] {
  if (!chord) return [];
  return [
    ...(chord.primary ? [primaryModifierLabel] : []),
    ...(chord.secondary ? [secondaryModifierLabel] : []),
    ...(chord.alt ? [altModifierLabel] : []),
    ...(chord.shift ? [shiftModifierLabel] : []),
    [codeLabels[chord.code] ?? chord.code.replace(/^Key/, '').replace(/^Digit/, '')]
  ].flat();
}

export function hotkeyDisplayLabel(chord: HotkeyChord | null): string {
  const parts = hotkeyDisplayParts(chord);
  return parts.join(primaryModifierLabel === '⌘' ? '' : '+');
}

export function nativeHotkeyAccelerator(chord: HotkeyChord | null): string | null {
  if (!chord) return null;
  const modifiers = [
    ...(chord.primary ? ['CmdOrCtrl'] : []),
    ...(chord.secondary ? [primaryModifierLabel === '⌘' ? 'Ctrl' : 'Super'] : []),
    ...(chord.alt ? ['Alt'] : []),
    ...(chord.shift ? ['Shift'] : [])
  ];
  return [...modifiers, chord.code].join('+');
}

export function hotkeyAriaLabel(chord: HotkeyChord | null): string | null {
  if (!chord) return null;
  const modifiers = [
    ...(chord.primary ? [primaryModifierLabel === '⌘' ? 'Meta' : 'Control'] : []),
    ...(chord.secondary ? [primaryModifierLabel === '⌘' ? 'Control' : 'Meta'] : []),
    ...(chord.alt ? ['Alt'] : []),
    ...(chord.shift ? ['Shift'] : [])
  ];
  const key = chord.code.replace(/^Key/, '').replace(/^Digit/, '');
  return [...modifiers, key].join('+');
}

export function reservedHotkeyLabel(chord: HotkeyChord): string | null {
  const reserved: Array<[HotkeyChord, string]> = [
    [primaryChord('KeyQ'), 'Quit'],
    [primaryChord('KeyW'), 'Close window'],
    [primaryChord('KeyM'), 'Minimize window'],
    [primaryChord('KeyH'), 'Hide application'],
    [primaryChord('KeyX'), 'Cut'],
    [primaryChord('KeyC'), 'Copy'],
    [primaryChord('KeyV'), 'Paste'],
    [primaryChord('KeyA'), 'Select all'],
    [primaryChord('KeyZ'), 'Undo'],
    [{ ...primaryChord('KeyZ'), shift: true }, 'Redo'],
    [primaryChord('KeyY'), 'Redo']
  ];
  return reserved.find(([candidate]) => equalHotkey(candidate, chord))?.[1] ?? null;
}

/** Assigns one chord to one action, clearing it from any action that previously used it. */
export function setHotkeyBinding(
  action: HotkeyAction,
  chord: HotkeyChord | null
): HotkeyAction | null {
  let displaced: HotkeyAction | null = null;
  hotkeyPreferences.update((current) => {
    const next = clonePreferences(current);
    if (chord) {
      for (const candidate of allHotkeyActions) {
        if (
          candidate !== action
          && !canShareHotkey(action, candidate)
          && equalHotkey(next[candidate], chord)
        ) {
          next[candidate] = null;
          displaced ??= candidate;
        }
      }
    }
    next[action] = chord ? { ...chord } : null;
    saveHotkeyPreferences(next);
    return next;
  });
  return displaced;
}

export function resetHotkeyBindings(): void {
  const next = defaultHotkeyPreferences();
  saveHotkeyPreferences(next);
  hotkeyPreferences.set(next);
}

export function loadHotkeyPreferences(
  storage: Pick<Storage, 'getItem'> | null = browserStorage()
): HotkeyPreferences {
  if (!storage) return defaultHotkeyPreferences();
  try {
    const parsed = JSON.parse(storage.getItem(hotkeyStorageKey) ?? 'null') as unknown;
    if (
      !isRecord(parsed)
      || (parsed.version !== 1 && parsed.version !== 2 && parsed.version !== 3)
      || !isRecord(parsed.bindings)
    ) {
      return defaultHotkeyPreferences();
    }
    const legacyPreferences = parsed.version === 1 || parsed.version === 2;
    const next = legacyPreferences
      ? emptyHotkeyPreferences()
      : defaultHotkeyPreferences();
    if (legacyPreferences) {
      for (const action of legacyHotkeyActions) {
        next[action] = defaultPreferences[action] ? { ...defaultPreferences[action]! } : null;
      }
      if (parsed.version === 1) next['new-terminal'] = null;
    }
    const storedActions = legacyPreferences ? legacyHotkeyActions : allHotkeyActions;
    for (const action of storedActions) {
      const value = parsed.bindings[action];
      if (value === null) next[action] = null;
      else if (isHotkeyChord(value) && reservedHotkeyLabel(value) === null) next[action] = value;
    }
    const terminalDefault = defaultPreferences['new-terminal'];
    if (
      parsed.version === 1
      && next['new-terminal'] === null
      && terminalDefault !== null
      && !allHotkeyActions.some((action) => equalHotkey(next[action], terminalDefault))
    ) {
      next['new-terminal'] = { ...terminalDefault };
    }
    if (legacyPreferences) {
      for (const action of allHotkeyActions) {
        if ((legacyHotkeyActions as readonly HotkeyAction[]).includes(action)) continue;
        const chord = defaultPreferences[action];
        if (
          chord
          && !allHotkeyActions.some((candidate) => (
            !canShareHotkey(action, candidate) && equalHotkey(next[candidate], chord)
          ))
        ) next[action] = { ...chord };
      }
    }
    return deduplicatePreferences(next);
  } catch {
    return defaultHotkeyPreferences();
  }
}

export function saveHotkeyPreferences(
  preferences: HotkeyPreferences,
  storage: Pick<Storage, 'setItem'> | null = browserStorage()
): void {
  if (!storage) return;
  try {
    storage.setItem(hotkeyStorageKey, JSON.stringify({ version: 3, bindings: preferences }));
  } catch {
    // Hotkeys remain active for this session when webview storage is unavailable.
  }
}

function primaryChord(code: string): HotkeyChord {
  return { code, primary: true, secondary: false, alt: false, shift: false };
}

function altChord(code: string): HotkeyChord {
  return { code, primary: false, secondary: false, alt: true, shift: false };
}

function shiftChord(code: string): HotkeyChord {
  return { code, primary: false, secondary: false, alt: false, shift: true };
}

function terminalUnfocusChordValue(): HotkeyChord {
  return {
    ...primaryChord('KeyU'),
    shift: terminalUnfocusKeys.includes('Shift')
  };
}

function isHotkeyChord(value: unknown): value is HotkeyChord {
  return isRecord(value)
    && typeof value.code === 'string'
    && supportedCodes.has(value.code)
    && typeof value.primary === 'boolean'
    && typeof value.secondary === 'boolean'
    && typeof value.alt === 'boolean'
    && typeof value.shift === 'boolean'
    && (
      value.primary
      || value.secondary
      || value.alt
      || (value.shift && /^F\d+$/.test(value.code))
    );
}

function deduplicatePreferences(preferences: HotkeyPreferences): HotkeyPreferences {
  const next = clonePreferences(preferences);
  const seen: Array<[HotkeyAction, HotkeyChord]> = [];
  for (const action of allHotkeyActions) {
    const chord = next[action];
    if (!chord) continue;
    if (seen.some(([candidate, candidateChord]) => (
      !canShareHotkey(action, candidate) && equalHotkey(candidateChord, chord)
    ))) next[action] = null;
    else seen.push([action, chord]);
  }
  return next;
}

function canShareHotkey(left: HotkeyAction, right: HotkeyAction): boolean {
  return (left === 'new-quick-prompt' && right === 'new-agent')
    || (left === 'new-agent' && right === 'new-quick-prompt')
    // Off macOS the terminal unfocuses with primary+Shift+U, because primary+U
    // is line-kill in a shell. That is the recorder's undo chord too, but the
    // recorder registers its shortcuts globally only while a session runs, so
    // the two never listen at once and undo must not be deduplicated away.
    || (left === 'unfocus-terminal' && right === 'feedback-undo')
    || (left === 'feedback-undo' && right === 'unfocus-terminal');
}

function clonePreferences(preferences: HotkeyPreferences): HotkeyPreferences {
  return Object.fromEntries(
    allHotkeyActions.map((action) => [action, preferences[action] ? { ...preferences[action] } : null])
  ) as HotkeyPreferences;
}

function emptyHotkeyPreferences(): HotkeyPreferences {
  return Object.fromEntries(allHotkeyActions.map((action) => [action, null])) as HotkeyPreferences;
}

function isRecord(value: unknown): value is Record<string, any> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function browserStorage(): Storage | null {
  try {
    return typeof localStorage === 'undefined' ? null : localStorage;
  } catch {
    return null;
  }
}
