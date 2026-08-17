import { get, writable } from 'svelte/store';

export type ThemePreference = 'light' | 'dark' | 'system';
export type UiFontId =
  | 'system'
  | 'inter'
  | 'source-sans'
  | 'archivo'
  | 'avenir-next'
  | 'helvetica-neue';
export type TerminalFontId = 'jetbrains-mono' | 'sf-mono' | 'menlo' | 'profile';
export type TerminalThemeId = 'graphite' | 'paper' | 'classic' | 'custom' | 'imported';

export interface TerminalProfileStyle {
  fontFamily: string | null;
  fontSize: number | null;
  lineHeight: number | null;
  letterSpacing: number | null;
  cursorStyle: 'block' | 'underline' | 'bar' | null;
  cursorBlink: boolean | null;
  drawBoldTextInBrightColors: boolean | null;
}

export interface ImportedTerminalAppearance {
  imported: boolean;
  source: string | null;
  profile: string | null;
  palette: TerminalPalette | null;
  terminalStyle: TerminalProfileStyle | null;
}

export interface TerminalPalette {
  background: string;
  foreground: string;
  cursor: string;
  selection: string;
  black: string;
  red: string;
  green: string;
  yellow: string;
  blue: string;
  magenta: string;
  cyan: string;
  white: string;
  brightBlack: string;
  brightRed: string;
  brightGreen: string;
  brightYellow: string;
  brightBlue: string;
  brightMagenta: string;
  brightCyan: string;
  brightWhite: string;
}

export interface TerminalThemeSetting {
  id: TerminalThemeId;
  name: string;
  source: string | null;
  palette: TerminalPalette;
}

export interface TerminalThemePreset {
  id: Exclude<TerminalThemeId, 'custom' | 'imported'>;
  name: string;
  description: string;
  palette: TerminalPalette;
}

export interface AppearanceSettings {
  theme: ThemePreference;
  uiFont: UiFontId;
  uiFontScale: number;
  uiScale: number;
  terminalFont: TerminalFontId;
  terminalFontSize: number;
  terminalTheme: TerminalThemeSetting;
  terminalProfileStyle: TerminalProfileStyle | null;
}

export interface FontChoice<T extends string> {
  id: T;
  label: string;
  css: string;
  bundled?: boolean;
  localName?: string;
}

export const APPEARANCE_STORAGE_KEY = 'workman.appearance.v2';

export const TERMINAL_COLOR_KEYS = [
  'black', 'red', 'green', 'yellow', 'blue', 'magenta', 'cyan', 'white',
  'brightBlack', 'brightRed', 'brightGreen', 'brightYellow',
  'brightBlue', 'brightMagenta', 'brightCyan', 'brightWhite'
] as const satisfies readonly (keyof TerminalPalette)[];

export const TERMINAL_THEME_PRESETS: readonly TerminalThemePreset[] = [
  {
    id: 'graphite',
    name: 'Graphite',
    description: 'Soft charcoal with a restrained, readable spectrum.',
    palette: {
      background: '#202326', foreground: '#D7D9D5', cursor: '#A7C7B7', selection: '#3A4B52',
      black: '#353A3E', red: '#D8877E', green: '#8FBF8F', yellow: '#D6B56E',
      blue: '#82AFC5', magenta: '#B69AC8', cyan: '#7EB7B3', white: '#C9CCC7',
      brightBlack: '#687078', brightRed: '#E79A90', brightGreen: '#A4D4A4',
      brightYellow: '#E6C985', brightBlue: '#9CC5D8', brightMagenta: '#CCB2DA',
      brightCyan: '#98CFCC', brightWhite: '#F2F2EE'
    }
  },
  {
    id: 'paper',
    name: 'Paper',
    description: 'Warm light canvas with ink-forward ANSI colors.',
    palette: {
      background: '#F1EFE8', foreground: '#333638', cursor: '#3E7064', selection: '#CEDDD7',
      black: '#3B3D3F', red: '#A84A44', green: '#4E7651', yellow: '#986F2D',
      blue: '#456D85', magenta: '#765B85', cyan: '#3F7774', white: '#D4D1C9',
      brightBlack: '#707578', brightRed: '#C05B53', brightGreen: '#618C64',
      brightYellow: '#AD843D', brightBlue: '#5A8298', brightMagenta: '#8C709A',
      brightCyan: '#528D89', brightWhite: '#FFFFFF'
    }
  },
  {
    id: 'classic',
    name: 'Classic',
    description: 'True black and crisp colors for maximum separation.',
    palette: {
      background: '#000000', foreground: '#D7E2DC', cursor: '#7BD1B5', selection: '#355C55',
      black: '#11191B', red: '#DC7D76', green: '#79C69F', yellow: '#D7AD65',
      blue: '#78AECD', magenta: '#BCA0CF', cyan: '#72C8C2', white: '#D7E2DC',
      brightBlack: '#62706D', brightRed: '#EF958E', brightGreen: '#99DAB8',
      brightYellow: '#E8C37F', brightBlue: '#98C7DF', brightMagenta: '#D0B7DD',
      brightCyan: '#94DCD6', brightWhite: '#F2F6F3'
    }
  }
] as const;

const DEFAULT_TERMINAL_THEME: TerminalThemeSetting = settingFromPreset(TERMINAL_THEME_PRESETS[0]);

export const DEFAULT_APPEARANCE: Readonly<AppearanceSettings> = {
  theme: 'system',
  uiFont: 'system',
  uiFontScale: 1,
  uiScale: 1,
  terminalFont: 'jetbrains-mono',
  terminalFontSize: 13,
  terminalTheme: DEFAULT_TERMINAL_THEME,
  terminalProfileStyle: null
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

export function terminalFontCss(
  id: TerminalFontId,
  profileStyle: TerminalProfileStyle | null = null
): string {
  if (id === 'profile' && profileStyle?.fontFamily) {
    return `"${profileStyle.fontFamily.replaceAll('\\', '\\\\').replaceAll('"', '\\"')}", monospace`;
  }
  return TERMINAL_FONT_CHOICES.find((choice) => choice.id === id)?.css
    ?? TERMINAL_FONT_CHOICES[0].css;
}

/** Resolve the few xterm options represented by a native profile; all other metrics stay native. */
export function terminalProfileXtermOptions(profileStyle: TerminalProfileStyle | null): {
  cursorBlink: boolean;
  cursorStyle: 'block' | 'underline' | 'bar';
  drawBoldTextInBrightColors: boolean;
  letterSpacing: number;
  lineHeight: number;
} {
  return {
    cursorBlink: profileStyle?.cursorBlink ?? false,
    cursorStyle: profileStyle?.cursorStyle ?? 'block',
    drawBoldTextInBrightColors: profileStyle?.drawBoldTextInBrightColors ?? true,
    letterSpacing: profileStyle?.letterSpacing ?? 0,
    lineHeight: profileStyle?.lineHeight ?? 1
  };
}

export function terminalAppearancePatchFromImport(
  report: ImportedTerminalAppearance,
  current: AppearanceSettings
): Partial<AppearanceSettings> | null {
  if (!report.imported || !report.palette) return null;
  const style = report.terminalStyle ?? null;
  return {
    terminalTheme: {
      id: 'imported',
      name: report.profile ?? report.source ?? 'Imported',
      source: report.source,
      palette: report.palette
    },
    terminalProfileStyle: style,
    terminalFont: style?.fontFamily ? 'profile' : current.terminalFont,
    terminalFontSize: style?.fontSize === null || style?.fontSize === undefined
      ? current.terminalFontSize
      : Math.round(style.fontSize)
  };
}

export function shouldAutoImportTerminalProfile(
  settings: AppearanceSettings,
  alreadyAttempted: boolean
): boolean {
  return !alreadyAttempted && settings.terminalTheme.id === 'graphite';
}

export function terminalThemeFromPreset(id: TerminalThemePreset['id']): TerminalThemeSetting {
  return settingFromPreset(
    TERMINAL_THEME_PRESETS.find((preset) => preset.id === id) ?? TERMINAL_THEME_PRESETS[0]
  );
}

export function terminalContrastRatio(palette: TerminalPalette): number {
  const foreground = relativeLuminance(palette.foreground);
  const background = relativeLuminance(palette.background);
  const lighter = Math.max(foreground, background);
  const darker = Math.min(foreground, background);
  return (lighter + 0.05) / (darker + 0.05);
}

export function terminalXtermTheme(palette: TerminalPalette): Record<string, string> {
  return {
    background: palette.background,
    foreground: palette.foreground,
    cursor: palette.cursor,
    cursorAccent: palette.background,
    selectionBackground: palette.selection,
    black: palette.black,
    red: palette.red,
    green: palette.green,
    yellow: palette.yellow,
    blue: palette.blue,
    magenta: palette.magenta,
    cyan: palette.cyan,
    white: palette.white,
    brightBlack: palette.brightBlack,
    brightRed: palette.brightRed,
    brightGreen: palette.brightGreen,
    brightYellow: palette.brightYellow,
    brightBlue: palette.brightBlue,
    brightMagenta: palette.brightMagenta,
    brightCyan: palette.brightCyan,
    brightWhite: palette.brightWhite
  };
}

export function installedUiFonts(): FontChoice<UiFontId>[] {
  return UI_FONT_CHOICES.filter((choice) => isFontAvailable(choice));
}

export function installedTerminalFonts(
  profileStyle: TerminalProfileStyle | null = null
): FontChoice<TerminalFontId>[] {
  const choices = TERMINAL_FONT_CHOICES.filter((choice) => isFontAvailable(choice));
  if (profileStyle?.fontFamily) {
    choices.push({
      id: 'profile',
      label: `${profileStyle.fontFamily} · terminal profile`,
      css: terminalFontCss('profile', profileStyle),
      localName: profileStyle.fontFamily
    });
  }
  return choices;
}

function isFontAvailable<T extends string>(choice: FontChoice<T>): boolean {
  if (choice.id === 'system' || choice.bundled || !choice.localName) return true;
  return typeof document === 'undefined' || document.fonts.check(`12px "${choice.localName}"`);
}

function readAppearance(): AppearanceSettings {
  try {
    const stored = localStorage.getItem(APPEARANCE_STORAGE_KEY);
    if (stored) return sanitizeAppearance(JSON.parse(stored) as Partial<AppearanceSettings>);
    const legacyTheme = localStorage.getItem('workman.appearance');
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
    terminalFont: (
      TERMINAL_FONT_CHOICES.some((choice) => choice.id === value.terminalFont)
      || (value.terminalFont === 'profile' && sanitizeTerminalProfileStyle(value.terminalProfileStyle))
    ) ? value.terminalFont as TerminalFontId : DEFAULT_APPEARANCE.terminalFont,
    terminalFontSize: clampInteger(value.terminalFontSize, 10, 20, DEFAULT_APPEARANCE.terminalFontSize),
    terminalTheme: sanitizeTerminalTheme(value.terminalTheme),
    terminalProfileStyle: sanitizeTerminalProfileStyle(value.terminalProfileStyle)
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
  root.style.setProperty(
    '--terminal-font-family',
    terminalFontCss(settings.terminalFont, settings.terminalProfileStyle)
  );
  root.style.setProperty('--terminal-font-size', `${settings.terminalFontSize}px`);
  root.style.setProperty(
    '--terminal-line-height',
    String(terminalProfileXtermOptions(settings.terminalProfileStyle).lineHeight)
  );
  root.style.setProperty(
    '--terminal-letter-spacing',
    `${terminalProfileXtermOptions(settings.terminalProfileStyle).letterSpacing}px`
  );
  root.style.setProperty('--terminal-background', settings.terminalTheme.palette.background);
  root.style.setProperty('--terminal-foreground', settings.terminalTheme.palette.foreground);
  root.style.setProperty('--terminal-cursor', settings.terminalTheme.palette.cursor);
  root.style.setProperty('--terminal-selection', settings.terminalTheme.palette.selection);
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

function settingFromPreset(preset: TerminalThemePreset): TerminalThemeSetting {
  return {
    id: preset.id,
    name: preset.name,
    source: null,
    palette: { ...preset.palette }
  };
}

function sanitizeTerminalTheme(value: unknown): TerminalThemeSetting {
  if (!value || typeof value !== 'object') return cloneTerminalTheme(DEFAULT_TERMINAL_THEME);
  const candidate = value as Partial<TerminalThemeSetting>;
  const id = includes(
    ['graphite', 'paper', 'classic', 'custom', 'imported'] as const,
    candidate.id
  ) ? candidate.id : 'graphite';
  const fallback = id === 'custom' || id === 'imported'
    ? DEFAULT_TERMINAL_THEME
    : terminalThemeFromPreset(id);
  const paletteValue = candidate.palette && typeof candidate.palette === 'object'
    ? candidate.palette as Partial<TerminalPalette>
    : {};
  const palette = Object.fromEntries(
    (['background', 'foreground', 'cursor', 'selection', ...TERMINAL_COLOR_KEYS] as const)
      .map((key) => [key, sanitizeHex(paletteValue[key], fallback.palette[key])])
  ) as unknown as TerminalPalette;
  return {
    id,
    name: typeof candidate.name === 'string' && candidate.name.trim()
      ? candidate.name.trim().slice(0, 80)
      : fallback.name,
    source: typeof candidate.source === 'string' && candidate.source.trim()
      ? candidate.source.trim().slice(0, 160)
      : null,
    palette
  };
}

function sanitizeTerminalProfileStyle(value: unknown): TerminalProfileStyle | null {
  if (!value || typeof value !== 'object') return null;
  const candidate = value as Partial<TerminalProfileStyle>;
  const fontFamily = typeof candidate.fontFamily === 'string' && candidate.fontFamily.trim()
    ? candidate.fontFamily.trim().slice(0, 160)
    : null;
  const fontSize = finiteNumber(candidate.fontSize, 6, 72);
  const lineHeight = finiteNumber(candidate.lineHeight, 1, 3);
  const letterSpacing = finiteNumber(candidate.letterSpacing, -5, 20);
  const cursorStyle = includes(['block', 'underline', 'bar'] as const, candidate.cursorStyle)
    ? candidate.cursorStyle
    : null;
  return {
    fontFamily,
    fontSize,
    lineHeight,
    letterSpacing: letterSpacing === null ? null : Math.round(letterSpacing),
    cursorStyle,
    cursorBlink: typeof candidate.cursorBlink === 'boolean' ? candidate.cursorBlink : null,
    drawBoldTextInBrightColors: typeof candidate.drawBoldTextInBrightColors === 'boolean'
      ? candidate.drawBoldTextInBrightColors
      : null
  };
}

function finiteNumber(value: unknown, min: number, max: number): number | null {
  return typeof value === 'number' && Number.isFinite(value)
    ? Math.min(max, Math.max(min, value))
    : null;
}

function cloneTerminalTheme(theme: TerminalThemeSetting): TerminalThemeSetting {
  return { ...theme, palette: { ...theme.palette } };
}

function sanitizeHex(value: unknown, fallback: string): string {
  return typeof value === 'string' && /^#[0-9a-f]{6}$/i.test(value)
    ? value.toUpperCase()
    : fallback;
}

function relativeLuminance(hex: string): number {
  const channels = [1, 3, 5].map((offset) => Number.parseInt(hex.slice(offset, offset + 2), 16) / 255);
  const [red, green, blue] = channels.map((channel) =>
    channel <= 0.04045 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4
  );
  return 0.2126 * red + 0.7152 * green + 0.0722 * blue;
}
