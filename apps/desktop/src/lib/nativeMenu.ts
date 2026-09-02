import { invoke } from '@tauri-apps/api/core';
import { writable } from 'svelte/store';

import { nativeHotkeyAccelerator, type HotkeyPreferences } from './hotkeys';

export const NATIVE_MENU_EVENT = 'menu://action';

export type NativeMenuAction =
  | 'about'
  | 'settings'
  | 'check_updates'
  | 'previous_view'
  | 'toggle_project_rail'
  | 'toggle_section_rail';

export const nativeUpdateCheckRequest = writable(0);

export function syncNativeMenuAccelerators(preferences: HotkeyPreferences): Promise<void> {
  return setNativeMenuAccelerators({
    settings: nativeHotkeyAccelerator(preferences['open-settings']),
    previous_view: nativeHotkeyAccelerator(preferences['previous-view']),
    toggle_project_rail: nativeHotkeyAccelerator(preferences['toggle-project-rail']),
    toggle_section_rail: nativeHotkeyAccelerator(preferences['toggle-project-tree'])
  });
}

export function suspendNativeMenuAccelerators(): Promise<void> {
  return setNativeMenuAccelerators({
    settings: null,
    previous_view: null,
    toggle_project_rail: null,
    toggle_section_rail: null
  });
}

function setNativeMenuAccelerators(accelerators: Record<string, string | null>): Promise<void> {
  return invoke('desktop_set_menu_accelerators', { accelerators });
}

export function requestNativeUpdateCheck(): void {
  nativeUpdateCheckRequest.update((request) => request + 1);
}

export function consumeNativeUpdateCheckRequest(): void {
  nativeUpdateCheckRequest.set(0);
}
