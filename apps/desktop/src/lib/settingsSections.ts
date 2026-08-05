import { writable } from 'svelte/store';

import { appNavigation } from './navigation';

export type SettingsSectionId =
  | 'appearance'
  | 'terminal'
  | 'sidebar'
  | 'hotkeys'
  | 'agents'
  | 'tools'
  | 'mcp'
  | 'daemon';

export interface SettingsSectionDefinition {
  id: SettingsSectionId;
  label: string;
  icon: string;
  description: string;
  local: boolean;
}

export const settingsSections: SettingsSectionDefinition[] = [
  { id: 'appearance', label: 'Appearance', icon: 'Aa', description: 'Theme and interface type', local: true },
  { id: 'terminal', label: 'Terminal', icon: '>_', description: 'Terminal typography', local: true },
  { id: 'sidebar', label: 'Sidebar', icon: '▥', description: 'Rails and project tree', local: true },
  { id: 'hotkeys', label: 'Hotkeys', icon: '⌘', description: 'Keyboard reference', local: true },
  { id: 'agents', label: 'Agents', icon: '◎', description: 'Runtimes and tools', local: false },
  { id: 'tools', label: 'Tools', icon: '⌁', description: 'External openers', local: true },
  { id: 'mcp', label: 'MCP', icon: '◇', description: 'Agent connection', local: false },
  { id: 'daemon', label: 'Daemon', icon: '◉', description: 'Local runtime', local: false }
];

const storageKey = 'awm.settings.section.v1';
const fallbackSection: SettingsSectionId = 'appearance';

function isSettingsSection(value: unknown): value is SettingsSectionId {
  return settingsSections.some((section) => section.id === value);
}

function sectionFromLocation(): SettingsSectionId | null {
  if (typeof window === 'undefined') return null;
  const query = new URLSearchParams(window.location.search).get('settings');
  if (isSettingsSection(query)) return query;
  const hash = window.location.hash.match(/^#settings[/:]([a-z-]+)$/i)?.[1]?.toLowerCase();
  return isSettingsSection(hash) ? hash : null;
}

function loadSettingsSection(): SettingsSectionId {
  const linked = sectionFromLocation();
  if (linked) return linked;
  try {
    const stored = localStorage.getItem(storageKey);
    return isSettingsSection(stored) ? stored : fallbackSection;
  } catch {
    return fallbackSection;
  }
}

export const settingsSection = writable<SettingsSectionId>(loadSettingsSection());

export function selectSettingsSection(section: SettingsSectionId): void {
  settingsSection.set(section);
  try {
    localStorage.setItem(storageKey, section);
  } catch {
    // Settings remain navigable when webview storage is unavailable.
  }
}

/**
 * Public deep-link seam for cards and future navigation surfaces. Selection is
 * committed before App resolves the existing settings navigation request.
 */
export function openSettingsSection(section: SettingsSectionId, projectId?: number): number {
  selectSettingsSection(section);
  return appNavigation.navigate({ type: 'settings', projectId }, 'api');
}
