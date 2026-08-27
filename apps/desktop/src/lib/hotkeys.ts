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

export type ProjectHotkeyAction = (typeof projectHotkeyActions)[number];
export type CreationHotkeyAction = (typeof creationHotkeyActions)[number];
export type HotkeyAction = ProjectHotkeyAction | CreationHotkeyAction;

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
  group: 'projects' | 'creation';
}

export const hotkeyDefinitions: readonly HotkeyDefinition[] = [
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

const allHotkeyActions = [...projectHotkeyActions, ...creationHotkeyActions] as const;
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
  ...projectHotkeyActions.map((action, index) => [action, primaryChord(`Digit${index + 1}`)]),
  ['new-agent', primaryChord('KeyN')],
  ['new-terminal', null],
  ['new-command', null],
  ['new-scratchpad', null],
  ['new-todo', null]
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
  // Global unmodified or Shift-only keys would steal ordinary typing and editing.
  return chord.primary || chord.secondary || chord.alt ? chord : null;
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

export function reservedHotkeyLabel(chord: HotkeyChord): string | null {
  const reserved: Array<[HotkeyChord, string]> = [
    [primaryChord('KeyK'), 'Quick jump'],
    [primaryChord('Slash'), 'Keyboard reference'],
    [primaryChord('Comma'), 'Settings'],
    [primaryChord('KeyB'), 'Toggle project rail'],
    [{ ...primaryChord('KeyB'), shift: true }, 'Toggle project tree'],
    [{ ...primaryChord('KeyP'), shift: true }, 'Quick prompts'],
    [primaryChord('KeyF'), 'Search terminal buffer'],
    [primaryChord('ArrowLeft'), 'Panel navigation'],
    [primaryChord('ArrowRight'), 'Panel navigation'],
    [primaryChord('ArrowUp'), 'Process navigation'],
    [primaryChord('ArrowDown'), 'Process navigation'],
    [altChord('ArrowUp'), 'Reorder focused item'],
    [altChord('ArrowDown'), 'Reorder focused item'],
    [terminalUnfocusChordValue(), 'Unfocus terminal']
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
        if (candidate !== action && equalHotkey(next[candidate], chord)) {
          next[candidate] = null;
          displaced = candidate;
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
    if (!isRecord(parsed) || parsed.version !== 1 || !isRecord(parsed.bindings)) {
      return defaultHotkeyPreferences();
    }
    const next = defaultHotkeyPreferences();
    for (const action of allHotkeyActions) {
      const value = parsed.bindings[action];
      if (value === null) next[action] = null;
      else if (isHotkeyChord(value) && reservedHotkeyLabel(value) === null) next[action] = value;
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
    storage.setItem(hotkeyStorageKey, JSON.stringify({ version: 1, bindings: preferences }));
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
    && (value.primary || value.secondary || value.alt);
}

function deduplicatePreferences(preferences: HotkeyPreferences): HotkeyPreferences {
  const next = clonePreferences(preferences);
  const seen: HotkeyChord[] = [];
  for (const action of allHotkeyActions) {
    const chord = next[action];
    if (!chord) continue;
    if (seen.some((candidate) => equalHotkey(candidate, chord))) next[action] = null;
    else seen.push(chord);
  }
  return next;
}

function clonePreferences(preferences: HotkeyPreferences): HotkeyPreferences {
  return Object.fromEntries(
    allHotkeyActions.map((action) => [action, preferences[action] ? { ...preferences[action] } : null])
  ) as HotkeyPreferences;
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
