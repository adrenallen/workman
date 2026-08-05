import { writable } from 'svelte/store';

export const NATIVE_MENU_EVENT = 'menu://action';

export type NativeMenuAction =
  | 'about'
  | 'settings'
  | 'check_updates'
  | 'toggle_project_rail'
  | 'toggle_section_rail';

export const nativeUpdateCheckRequest = writable(0);

export function requestNativeUpdateCheck(): void {
  nativeUpdateCheckRequest.update((request) => request + 1);
}

export function consumeNativeUpdateCheckRequest(): void {
  nativeUpdateCheckRequest.set(0);
}
