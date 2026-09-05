import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { get, writable } from 'svelte/store';

import type { Notification, ProcessView } from './daemon';
import { isTopLevelAgentNotification } from './notificationAttention.ts';

export const NATIVE_NOTIFICATION_ACTION_EVENT = 'notification://action';

const preferencesKey = 'workman.native-notifications.v1';

export interface NativeNotificationPreferences {
  enabled: boolean;
  needsInput: boolean;
  topLevelOnly: boolean;
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
  needsInput: true,
  topLevelOnly: true
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
let nativeCommandQueue: Promise<unknown> = Promise.resolve();
const readNotificationIds = new Set<number>();
const dismissedNotificationIds = new Set<number>();

function enqueueNativeCommand<T>(action: () => Promise<T>): Promise<T> {
  const next = nativeCommandQueue.catch(() => undefined).then(action);
  nativeCommandQueue = next;
  return next;
}

export function setNativeNotificationsEnabled(enabled: boolean): void {
  savePreferences({ ...get(nativeNotificationPreferences), enabled });
}

export function setNeedsInputNotificationsEnabled(needsInput: boolean): void {
  savePreferences({ ...get(nativeNotificationPreferences), needsInput });
}

export function setTopLevelNotificationsOnly(topLevelOnly: boolean): void {
  savePreferences({ ...get(nativeNotificationPreferences), topLevelOnly });
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

export async function deliverNativeNotification(
  notification: Notification,
  processes: ProcessView[] = [],
  isUnread: () => boolean = () => notification.read_at === null
): Promise<boolean> {
  const preferences = get(nativeNotificationPreferences);
  if (!preferences.enabled) return false;
  if (notification.type === 'needs_input' && !preferences.needsInput) return false;
  if (preferences.topLevelOnly && !isTopLevelAgentNotification(notification, processes)) return false;
  if (!isUnread() || readNotificationIds.has(notification.id)) return false;
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
    return await enqueueNativeCommand(async () => {
      // Permission sheets and earlier deliveries can take time. Recheck before displaying so a
      // banner cannot arrive after the user has returned to Workman or read the matching agent.
      const current = get(nativeNotificationPreferences);
      if (!current.enabled || (notification.type === 'needs_input' && !current.needsInput)) return false;
      if (current.topLevelOnly && !isTopLevelAgentNotification(notification, processes)) return false;
      if (await getCurrentWindow().isFocused()) return false;
      if (!isUnread() || readNotificationIds.has(notification.id)) return false;
      await invoke('native_notification_show', {
        notificationId: notification.id,
        title: notificationTitle(notification),
        body: notification.body
      });
      nativeNotificationRuntime.update((current) => ({ ...current, error: null }));
      return true;
    });
  } catch (cause) {
    nativeNotificationRuntime.update((current) => ({ ...current, error: message(cause) }));
    return false;
  }
}

/** Call only after a confirmed read; failed optimistic updates must retain OS notifications. */
export async function dismissNativeNotifications(notificationIds: number[]): Promise<void> {
  const ids = [...new Set(notificationIds)].filter((id) => id > 0 && !dismissedNotificationIds.has(id));
  if (ids.length === 0) return;
  for (const id of ids) readNotificationIds.add(id);
  try {
    await enqueueNativeCommand(() => invoke('native_notification_dismiss', { notificationIds: ids }));
    for (const id of ids) dismissedNotificationIds.add(id);
  } catch (cause) {
    // The authoritative read list retries removal on the next refresh.
    nativeNotificationRuntime.update((current) => ({ ...current, error: message(cause) }));
  }
}

export async function deliverNativeSystemNotification(
  title: string,
  body: string
): Promise<boolean> {
  if (!get(nativeNotificationPreferences).enabled) return false;

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
      notificationId: 0,
      title,
      body
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
    await invoke('native_notification_set_badge', { count: Math.max(0, unreadCount) });
  } catch {
    // Keep older desktop shells working. Linux launchers may not implement either badge API.
    try { await getCurrentWindow().setBadgeCount(unreadCount > 0 ? unreadCount : undefined); }
    catch { /* The in-app count remains authoritative. */ }
  }
}

/** WebView2/Linux can freeze hidden pages; an active Web Lock keeps delivery work alive. */
export function keepNotificationDeliveryActive(): () => void {
  if (typeof navigator === 'undefined' || !navigator.locks) return () => {};
  const controller = new AbortController();
  let release = (): void => {};
  const held = new Promise<void>((resolve) => { release = resolve; });
  void navigator.locks.request('workman-notification-delivery', {
    mode: 'shared', signal: controller.signal
  }, () => held).catch(() => undefined);
  return () => { release(); controller.abort(); };
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
    case 'todo_assigned_to_you':
      return 'Todo assigned to you';
    case 'mentioned_in_comment':
      return 'Mentioned in a comment';
    default:
      return 'Workman notification';
  }
}

function loadPreferences(): NativeNotificationPreferences {
  try {
    const stored = JSON.parse(localStorage.getItem(preferencesKey) ?? 'null');
    if (typeof stored?.enabled === 'boolean' && typeof stored?.needsInput === 'boolean') {
      return { ...stored, topLevelOnly: typeof stored.topLevelOnly === 'boolean' ? stored.topLevelOnly : true };
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
