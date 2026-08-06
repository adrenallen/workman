import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { get, writable } from 'svelte/store';

import type { Notification } from './daemon';

export const NATIVE_NOTIFICATION_ACTION_EVENT = 'notification://action';

const preferencesKey = 'workman.native-notifications.v1';

export interface NativeNotificationPreferences {
  enabled: boolean;
  needsInput: boolean;
}

export type NativeNotificationPermissionState =
  | 'checking'
  | 'not_determined'
  | 'granted'
  | 'denied'
  | 'unknown'
  | 'unavailable';

export interface NativeNotificationPermission {
  state: NativeNotificationPermissionState;
  platform: string;
  detail: string | null;
}

export interface NativeNotificationRuntime {
  permission: NativeNotificationPermission;
  busy: boolean;
  error: string | null;
}

interface NativeNotificationAction {
  notification_id: number;
}

const fallbackPreferences: NativeNotificationPreferences = {
  enabled: true,
  needsInput: true
};

const checkingPermission: NativeNotificationPermission = {
  state: 'checking',
  platform: 'desktop',
  detail: 'Checking OS notification permission…'
};

export const nativeNotificationPreferences = writable<NativeNotificationPreferences>(
  loadPreferences()
);

export const nativeNotificationRuntime = writable<NativeNotificationRuntime>({
  permission: checkingPermission,
  busy: false,
  error: null
});

let permissionRequest: Promise<NativeNotificationPermission> | null = null;

export function setNativeNotificationsEnabled(enabled: boolean): void {
  savePreferences({ ...get(nativeNotificationPreferences), enabled });
}

export function setNeedsInputNotificationsEnabled(needsInput: boolean): void {
  savePreferences({ ...get(nativeNotificationPreferences), needsInput });
}

export async function refreshNativeNotificationPermission(): Promise<NativeNotificationPermission> {
  nativeNotificationRuntime.update((current) => ({ ...current, busy: true, error: null }));
  try {
    const permission = await invoke<NativeNotificationPermission>(
      'native_notification_permission_state'
    );
    nativeNotificationRuntime.set({ permission, busy: false, error: null });
    return permission;
  } catch (cause) {
    const error = message(cause);
    const permission: NativeNotificationPermission = {
      state: 'unavailable',
      platform: 'desktop',
      detail: 'This build cannot query OS notification permission.'
    };
    nativeNotificationRuntime.set({ permission, busy: false, error });
    return permission;
  }
}

export async function requestNativeNotificationPermission(): Promise<NativeNotificationPermission> {
  if (permissionRequest) return permissionRequest;

  nativeNotificationRuntime.update((current) => ({ ...current, busy: true, error: null }));
  permissionRequest = invoke<NativeNotificationPermission>('native_notification_request_permission')
    .then((permission) => {
      nativeNotificationRuntime.set({ permission, busy: false, error: null });
      return permission;
    })
    .catch((cause) => {
      const error = message(cause);
      nativeNotificationRuntime.update((current) => ({ ...current, busy: false, error }));
      throw cause;
    })
    .finally(() => {
      permissionRequest = null;
    });
  return permissionRequest;
}

export async function deliverNativeNotification(notification: Notification): Promise<boolean> {
  const preferences = get(nativeNotificationPreferences);
  if (!preferences.enabled) return false;
  if (notification.type === 'needs_input' && !preferences.needsInput) return false;
  try {
    if (await getCurrentWindow().isFocused()) return false;
  } catch (cause) {
    nativeNotificationRuntime.update((current) => ({ ...current, error: message(cause) }));
    return false;
  }

  let permission = get(nativeNotificationRuntime).permission;
  if (permission.state === 'checking') {
    permission = await refreshNativeNotificationPermission();
  }
  if (permission.state === 'not_determined') {
    try {
      permission = await requestNativeNotificationPermission();
    } catch {
      return false;
    }
  }
  if (permission.state !== 'granted') return false;

  try {
    await invoke('native_notification_show', {
      notificationId: notification.id,
      title: notificationTitle(notification),
      body: notification.body
    });
    nativeNotificationRuntime.update((current) => ({ ...current, error: null }));
    return true;
  } catch (cause) {
    nativeNotificationRuntime.update((current) => ({ ...current, error: message(cause) }));
    return false;
  }
}

export async function syncDockUnreadBadge(unreadCount: number): Promise<void> {
  try {
    await getCurrentWindow().setBadgeCount(unreadCount > 0 ? unreadCount : undefined);
  } catch {
    // Badge support varies by desktop environment; the in-app count remains authoritative.
  }
}

export function listenForNativeNotificationActions(
  onAction: (notificationId: number) => void
): Promise<UnlistenFn> {
  return listen<NativeNotificationAction>(NATIVE_NOTIFICATION_ACTION_EVENT, ({ payload }) => {
    onAction(payload.notification_id);
  });
}

function notificationTitle(notification: Notification): string {
  switch (notification.type) {
    case 'agent_done':
      return 'Agent finished';
    case 'needs_input':
      return 'Agent needs input';
    case 'process_crashed':
      return 'Process crashed';
    case 'timer_fired':
      return 'Timer fired';
    default:
      return 'Workman notification';
  }
}

function loadPreferences(): NativeNotificationPreferences {
  try {
    const stored = JSON.parse(localStorage.getItem(preferencesKey) ?? 'null');
    if (typeof stored?.enabled === 'boolean' && typeof stored?.needsInput === 'boolean') {
      return stored;
    }
  } catch {
    // Defaults keep notifications enabled when local storage is unavailable or malformed.
  }
  return fallbackPreferences;
}

function savePreferences(preferences: NativeNotificationPreferences): void {
  nativeNotificationPreferences.set(preferences);
  try {
    localStorage.setItem(preferencesKey, JSON.stringify(preferences));
  } catch {
    // Preferences still apply for this session when local storage is unavailable.
  }
}

function message(cause: unknown): string {
  return cause instanceof Error ? cause.message : String(cause);
}
