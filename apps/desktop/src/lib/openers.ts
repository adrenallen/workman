import { invoke } from '@tauri-apps/api/core';
import { get, writable } from 'svelte/store';

export interface DetectedEditor {
  id: string;
  label: string;
  bundle_path: string;
}

export type EditorSelection = `detected:${string}` | 'custom';
export type TerminalSelection = 'system' | 'custom';
export type BrowserSelection = 'system' | 'custom';

export interface OpenersConfig {
  editor: {
    selection: EditorSelection;
    customTemplate: string;
  };
  terminal: {
    selection: TerminalSelection;
    customTemplate: string;
  };
  browser: {
    selection: BrowserSelection;
    customTemplate: string;
  };
  sidebar: {
    editorEnabled: boolean;
    finderEnabled: boolean;
    customEnabled: boolean;
    customLabel: string;
    customTemplate: string;
  };
}

export interface OpenersState {
  config: OpenersConfig;
  editors: DetectedEditor[];
  loaded: boolean;
  error: string | null;
}

const storageKey = 'gbuild.openers.v1';

const defaultConfig: OpenersConfig = {
  editor: {
    selection: 'custom',
    customTemplate: 'code {path}'
  },
  terminal: {
    selection: 'system',
    customTemplate: 'open -a Terminal {path}'
  },
  browser: {
    selection: 'system',
    customTemplate: 'open {path}'
  },
  sidebar: {
    editorEnabled: true,
    finderEnabled: true,
    customEnabled: false,
    customLabel: 'Open in…',
    customTemplate: ''
  }
};

export const openerSettings = writable<OpenersState>({
  config: structuredClone(defaultConfig),
  editors: [],
  loaded: false,
  error: null
});

let loading: Promise<OpenersState> | null = null;

export function ensureOpenersLoaded(): Promise<OpenersState> {
  if (get(openerSettings).loaded) return Promise.resolve(get(openerSettings));
  if (loading) return loading;

  loading = invoke<DetectedEditor[]>('shell_detect_editors')
    .then((editors) => {
      const stored = readStoredConfig();
      const config = normalizeConfig(stored, editors);
      const state = { config, editors, loaded: true, error: null } satisfies OpenersState;
      openerSettings.set(state);
      persist(config);
      return state;
    })
    .catch((cause) => {
      const config = normalizeConfig(readStoredConfig(), []);
      const state = {
        config,
        editors: [],
        loaded: true,
        error: message(cause)
      } satisfies OpenersState;
      openerSettings.set(state);
      return state;
    })
    .finally(() => {
      loading = null;
    });
  return loading;
}

export function setOpenersConfig(config: OpenersConfig): void {
  const current = get(openerSettings);
  const normalized = normalizeConfig(config, current.editors);
  openerSettings.set({ ...current, config: normalized });
  persist(normalized);
}

export function editorForSelection(
  config: OpenersConfig,
  editors: DetectedEditor[]
): DetectedEditor | null {
  if (!config.editor.selection.startsWith('detected:')) return null;
  const id = config.editor.selection.slice('detected:'.length);
  return editors.find((editor) => editor.id === id) ?? null;
}

export function editorActionLabel(config: OpenersConfig, editors: DetectedEditor[]): string {
  const editor = editorForSelection(config, editors);
  return editor ? `Open in ${editor.label}` : 'Open in editor';
}

export function customActionLabel(config: OpenersConfig): string {
  return config.sidebar.customLabel.trim() || 'Open in…';
}

export function templateError(template: string): string | null {
  if (!template.trim()) return 'Enter a command template.';
  if (!template.includes('{path}')) return 'Include {path} where the project path belongs.';
  if (template.includes('\0')) return 'NUL bytes are not allowed.';
  if (template.length > 4096) return 'Keep the command template under 4,096 characters.';
  return null;
}

export async function openProjectEditor(path: string, state = get(openerSettings)): Promise<void> {
  const detected = editorForSelection(state.config, state.editors);
  if (detected) {
    await invoke('shell_open_with', {
      path,
      opener: { kind: 'detected', id: detected.id }
    });
    return;
  }
  const error = templateError(state.config.editor.customTemplate);
  if (error) throw new Error(error);
  await openCustomPath(path, state.config.editor.customTemplate);
}

export function openProjectFinder(path: string): Promise<void> {
  return invoke('shell_open_path', { path, target: 'finder' });
}

export async function openProjectCustom(path: string, state = get(openerSettings)): Promise<void> {
  const error = templateError(state.config.sidebar.customTemplate);
  if (error) throw new Error(error);
  await openCustomPath(path, state.config.sidebar.customTemplate);
}

export function openCustomPath(path: string, template: string): Promise<void> {
  return invoke('shell_open_with', {
    path,
    opener: { kind: 'custom', template }
  });
}

function readStoredConfig(): unknown {
  if (typeof localStorage === 'undefined') return null;
  try {
    const stored = localStorage.getItem(storageKey);
    return stored ? JSON.parse(stored) : null;
  } catch {
    return null;
  }
}

function persist(config: OpenersConfig): void {
  if (typeof localStorage === 'undefined') return;
  localStorage.setItem(storageKey, JSON.stringify(config));
}

function normalizeConfig(input: unknown, editors: DetectedEditor[]): OpenersConfig {
  const source = record(input);
  const editor = record(source.editor);
  const terminal = record(source.terminal);
  const browser = record(source.browser);
  const sidebar = record(source.sidebar);
  const detectedDefault = editors.find((candidate) => candidate.id === 'vscode') ?? editors[0];
  const requestedEditor = string(editor.selection, '');
  const requestedEditorId = requestedEditor.startsWith('detected:')
    ? requestedEditor.slice('detected:'.length)
    : null;
  const selection: EditorSelection = requestedEditor === 'custom'
    ? 'custom'
    : requestedEditorId && editors.some((candidate) => candidate.id === requestedEditorId)
      ? `detected:${requestedEditorId}`
      : detectedDefault
        ? `detected:${detectedDefault.id}`
        : 'custom';

  return {
    editor: {
      selection,
      customTemplate: string(editor.customTemplate, defaultConfig.editor.customTemplate)
    },
    terminal: {
      selection: terminal.selection === 'custom' ? 'custom' : 'system',
      customTemplate: string(terminal.customTemplate, defaultConfig.terminal.customTemplate)
    },
    browser: {
      selection: browser.selection === 'custom' ? 'custom' : 'system',
      customTemplate: string(browser.customTemplate, defaultConfig.browser.customTemplate)
    },
    sidebar: {
      editorEnabled: boolValue(sidebar.editorEnabled, defaultConfig.sidebar.editorEnabled),
      finderEnabled: boolValue(sidebar.finderEnabled, defaultConfig.sidebar.finderEnabled),
      customEnabled: boolValue(sidebar.customEnabled, defaultConfig.sidebar.customEnabled),
      customLabel: string(sidebar.customLabel, defaultConfig.sidebar.customLabel),
      customTemplate: string(sidebar.customTemplate, defaultConfig.sidebar.customTemplate)
    }
  };
}

function record(value: unknown): Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
    ? value as Record<string, unknown>
    : {};
}

function string(value: unknown, fallback: string): string {
  return typeof value === 'string' ? value : fallback;
}

function boolValue(value: unknown, fallback: boolean): boolean {
  return typeof value === 'boolean' ? value : fallback;
}

function message(cause: unknown): string {
  return cause instanceof Error ? cause.message : String(cause);
}
