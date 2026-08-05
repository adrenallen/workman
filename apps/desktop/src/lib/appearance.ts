import { get, writable } from 'svelte/store';

export type ThemePreference = 'light' | 'dark' | 'system';
export type UiFontId =
  | 'system'
  | 'inter'
  | 'source-sans'
  | 'archivo'
  | 'avenir-next'
  | 'helvetica-neue';
export type TerminalFontId = 'jetbrains-mono' | 'sf-mono' | 'menlo';

export interface AppearanceSettings {
  theme: ThemePreference;
  uiFont: UiFontId;
  uiFontScale: number;
  uiScale: number;
  terminalFont: TerminalFontId;
  terminalFontSize: number;
}

export interface FontChoice<T extends string> {
  id: T;
  label: string;
  css: string;
  bundled?: boolean;
  localName?: string;
}

export const APPEARANCE_STORAGE_KEY = 'awm.appearance.v2';

export const DEFAULT_APPEARANCE: Readonly<AppearanceSettings> = {
  theme: 'system',
  uiFont: 'system',
  uiFontScale: 1,
  uiScale: 1,
  terminalFont: 'jetbrains-mono',
  terminalFontSize: 13
};

export const UI_FONT_CHOICES: readonly FontChoice<UiFontId>[] = [
  {
    id: 'system',
    label: 'System default',
    css: '-apple-system, BlinkMacSystemFont, "SF Pro Text", "Segoe UI", sans-serif'
  },
  { id: 'inter', label: 'Inter', css: '"Inter Variable", sans-serif', bundled: true },
  {
    id: 'source-sans',
    label: 'Source Sans 3',
    css: '"Source Sans 3 Variable", sans-serif',
    bundled: true
  },
  {
    id: 'archivo',
    label: 'Archivo',
    css: '"Archivo Variable", sans-serif',
    bundled: true
  },
  {
    id: 'avenir-next',
    label: 'Avenir Next',
    css: '"Avenir Next", sans-serif',
    localName: 'Avenir Next'
  },
  {
    id: 'helvetica-neue',
    label: 'Helvetica Neue',
    css: '"Helvetica Neue", sans-serif',
    localName: 'Helvetica Neue'
  }
];

export const TERMINAL_FONT_CHOICES: readonly FontChoice<TerminalFontId>[] = [
  {
    id: 'jetbrains-mono',
    label: 'JetBrains Mono',
    css: '"JetBrains Mono Variable", monospace',
    bundled: true
  },
  {
    id: 'sf-mono',
    label: 'SF Mono',
    css: '"SFMono-Regular", "SF Mono", monospace',
    localName: 'SF Mono'
  },
  { id: 'menlo', label: 'Menlo', css: 'Menlo, monospace', localName: 'Menlo' }
];

export const UI_FONT_SCALE_STEPS = [0.85, 0.925, 1, 1.075, 1.15] as const;
export const UI_SCALE_STEPS = [0.9, 1, 1.1, 1.2] as const;

export const appearance = writable<AppearanceSettings>({ ...DEFAULT_APPEARANCE });

let initialized = false;
let systemTheme: MediaQueryList | null = null;

export function initializeAppearance(): AppearanceSettings {
  if (initialized) return get(appearance);
  initialized = true;
  const settings = readAppearance();
  appearance.set(settings);
  applyAppearance(settings);

  systemTheme = window.matchMedia('(prefers-color-scheme: light)');
  systemTheme.addEventListener('change', applySystemTheme);
  return settings;
}

export function updateAppearance(patch: Partial<AppearanceSettings>): AppearanceSettings {
  const settings = sanitizeAppearance({ ...get(appearance), ...patch });
  appearance.set(settings);
  persistAppearance(settings);
  applyAppearance(settings);
  return settings;
}

export function resetAppearance(): AppearanceSettings {
  return updateAppearance({ ...DEFAULT_APPEARANCE });
}

export function currentAppearance(): AppearanceSettings {
  return get(appearance);
}

export function uiFontCss(id: UiFontId): string {
  return UI_FONT_CHOICES.find((choice) => choice.id === id)?.css ?? UI_FONT_CHOICES[0].css;
}

export function terminalFontCss(id: TerminalFontId): string {
  return TERMINAL_FONT_CHOICES.find((choice) => choice.id === id)?.css
    ?? TERMINAL_FONT_CHOICES[0].css;
}

export function installedUiFonts(): FontChoice<UiFontId>[] {
  return UI_FONT_CHOICES.filter((choice) => isFontAvailable(choice));
}

export function installedTerminalFonts(): FontChoice<TerminalFontId>[] {
  return TERMINAL_FONT_CHOICES.filter((choice) => isFontAvailable(choice));
}

function isFontAvailable<T extends string>(choice: FontChoice<T>): boolean {
  if (choice.id === 'system' || choice.bundled || !choice.localName) return true;
  return typeof document === 'undefined' || document.fonts.check(`12px "${choice.localName}"`);
}

function readAppearance(): AppearanceSettings {
  try {
    const stored = localStorage.getItem(APPEARANCE_STORAGE_KEY);
    if (stored) return sanitizeAppearance(JSON.parse(stored) as Partial<AppearanceSettings>);
    const legacyTheme = localStorage.getItem('awm.appearance');
    if (legacyTheme === 'light' || legacyTheme === 'dark' || legacyTheme === 'system') {
      return { ...DEFAULT_APPEARANCE, theme: legacyTheme };
    }
  } catch {
    // A corrupt or unavailable local store must never block the desktop shell.
  }
  return { ...DEFAULT_APPEARANCE };
}

function persistAppearance(settings: AppearanceSettings): void {
  try {
    localStorage.setItem(APPEARANCE_STORAGE_KEY, JSON.stringify(settings));
  } catch {
    // Live settings remain useful when webview persistence is unavailable.
  }
}

function sanitizeAppearance(value: Partial<AppearanceSettings>): AppearanceSettings {
  return {
    theme: includes(['light', 'dark', 'system'], value.theme)
      ? value.theme
      : DEFAULT_APPEARANCE.theme,
    uiFont: UI_FONT_CHOICES.some((choice) => choice.id === value.uiFont)
      ? value.uiFont as UiFontId
      : DEFAULT_APPEARANCE.uiFont,
    uiFontScale: nearest(UI_FONT_SCALE_STEPS, value.uiFontScale, DEFAULT_APPEARANCE.uiFontScale),
    uiScale: nearest(UI_SCALE_STEPS, value.uiScale, DEFAULT_APPEARANCE.uiScale),
    terminalFont: TERMINAL_FONT_CHOICES.some((choice) => choice.id === value.terminalFont)
      ? value.terminalFont as TerminalFontId
      : DEFAULT_APPEARANCE.terminalFont,
    terminalFontSize: clampInteger(value.terminalFontSize, 10, 20, DEFAULT_APPEARANCE.terminalFontSize)
  };
}

function applyAppearance(settings: AppearanceSettings): void {
  const root = document.documentElement;
  const resolvedTheme = settings.theme === 'system'
    ? (window.matchMedia('(prefers-color-scheme: light)').matches ? 'light' : 'dark')
    : settings.theme;
  root.dataset.theme = resolvedTheme;
  root.dataset.themePreference = settings.theme;
  root.style.colorScheme = resolvedTheme;
  root.style.setProperty('--ui-font-family', uiFontCss(settings.uiFont));
  root.style.setProperty('--ui-font-scale', String(settings.uiFontScale));
  root.style.setProperty('--ui-font-scale-percent', `${settings.uiFontScale * 100}%`);
  root.style.setProperty('--ui-scale', String(settings.uiScale));
  root.style.setProperty('--ui-scale-inverse', String(1 / settings.uiScale));
  root.style.setProperty('--terminal-font-family', terminalFontCss(settings.terminalFont));
  root.style.setProperty('--terminal-font-size', `${settings.terminalFontSize}px`);
}

function applySystemTheme(): void {
  const settings = get(appearance);
  if (settings.theme === 'system') applyAppearance(settings);
}

function includes<T extends string>(choices: readonly T[], value: unknown): value is T {
  return typeof value === 'string' && choices.includes(value as T);
}

function nearest(
  choices: readonly number[],
  value: unknown,
  fallback: number
): number {
  if (typeof value !== 'number' || !Number.isFinite(value)) return fallback;
  return choices.reduce((best, choice) =>
    Math.abs(choice - value) < Math.abs(best - value) ? choice : best
  );
}

function clampInteger(value: unknown, min: number, max: number, fallback: number): number {
  if (typeof value !== 'number' || !Number.isFinite(value)) return fallback;
  return Math.min(max, Math.max(min, Math.round(value)));
}
