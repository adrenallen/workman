import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { get, writable } from 'svelte/store';

import type { Notification, ProcessView } from './daemon';
import { isProjectReady, isTopLevelAgentNotification } from './notificationAttention.ts';
import { isWorkmanWindowFocused } from './windowAttention.ts';

export const NATIVE_NOTIFICATION_ACTION_EVENT = 'notification://action';

const preferencesKey = 'workman.native-notifications.v1';

export type NativeNotificationMode = 'all' | 'top_level' | 'project_ready';

export interface NativeNotificationPreferences {
  enabled: boolean;
  needsInput: boolean;
  mode: NativeNotificationMode;
  soundEnabled: boolean;
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
  sound_enabled?: boolean | null;
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
  mode: 'top_level',
  soundEnabled: true
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

export function setNativeNotificationMode(mode: NativeNotificationMode): void {
  savePreferences({ ...get(nativeNotificationPreferences), mode });
}

export function setNotificationSoundEnabled(soundEnabled: boolean): void {
  savePreferences({ ...get(nativeNotificationPreferences), soundEnabled });
}

function notificationAllowed(notification: Notification, processes: ProcessView[]): boolean {
  const preferences = get(nativeNotificationPreferences);
  if (!preferences.enabled) return false;
  if (notification.type === 'project_ready') {
    return preferences.mode === 'project_ready' && isProjectReady(notification.project_id, processes);
  }
  if (preferences.mode === 'project_ready' && (notification.type === 'agent_done' || notification.type === 'needs_input')) return false;
  if (notification.type === 'needs_input' && !preferences.needsInput) return false;
  return preferences.mode !== 'top_level' || isTopLevelAgentNotification(notification, processes);
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

export type NotificationSettingsTarget = 'notifications' | 'focus';

export async function openNativeNotificationSettings(target: NotificationSettingsTarget = 'notifications'): Promise<void> {
  try {
    await invoke('native_notification_open_settings', { target });
    nativeNotificationRuntime.update((current) => ({ ...current, error: null }));
  } catch (cause) {
    nativeNotificationRuntime.update((current) => ({ ...current, error: message(cause) }));
    throw cause;
  }
}

export async function deliverNativeNotification(
  notification: Notification,
  processes: ProcessView[] | (() => ProcessView[]) = [],
  isUnread: () => boolean = () => notification.read_at === null
): Promise<boolean> {
  const latestProcesses = () => typeof processes === 'function' ? processes() : processes;
  if (!notificationAllowed(notification, latestProcesses())) return false;
  if (!isUnread() || readNotificationIds.has(notification.id)) return false;
  try {
    if (await isWorkmanWindowFocused()) return false;
  } catch (cause) {
    nativeNotificationRuntime.update((current) => ({ ...current, error: message(cause) }));
    return false;
  }

  let permission = get(nativeNotificationRuntime).permission;
  if (permission.state === 'checking') {
    permission = await refreshNativeNotificationPermission();
  }
  if (!get(nativeNotificationPreferences).enabled) return false;
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
      if (await isWorkmanWindowFocused()) return false;
      const current = get(nativeNotificationPreferences);
      if (!notificationAllowed(notification, latestProcesses())) return false;
      if (!isUnread() || readNotificationIds.has(notification.id)) return false;
      await invoke('native_notification_show', {
        notificationId: notification.id,
        title: notificationTitle(notification),
        body: notification.body,
        sound: current.soundEnabled
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
  body: string,
  shouldSend: () => boolean = () => true
): Promise<boolean> {
  if (!get(nativeNotificationPreferences).enabled || !shouldSend()) return false;

  let permission = get(nativeNotificationRuntime).permission;
  if (permission.state === 'checking') {
    permission = await refreshNativeNotificationPermission();
  }
  if (!get(nativeNotificationPreferences).enabled || !shouldSend()) return false;
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
      // The user may switch to in-app only while permission or another delivery is pending.
      if (!get(nativeNotificationPreferences).enabled || !shouldSend()) return false;
      await invoke('native_notification_show', {
        notificationId: 0,
        title,
        body,
        sound: get(nativeNotificationPreferences).soundEnabled
      });
      nativeNotificationRuntime.update((current) => ({ ...current, error: null }));
      return true;
    });
  } catch (cause) {
    nativeNotificationRuntime.update((current) => ({ ...current, error: message(cause) }));
    return false;
  }
}

export async function syncDockUnreadBadge(unreadCount: number): Promise<void> {
  await enqueueNativeCommand(async () => {
    const count = get(nativeNotificationPreferences).enabled ? Math.max(0, unreadCount) : 0;
    try {
      await invoke('native_notification_set_badge', { count });
    } catch {
      // Keep older desktop shells working. Linux launchers may not implement either badge API.
      const fallbackCount = get(nativeNotificationPreferences).enabled ? count : 0;
      try { await getCurrentWindow().setBadgeCount(fallbackCount > 0 ? fallbackCount : undefined); }
      catch { /* The in-app count remains authoritative. */ }
    }
  });
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
    case 'project_ready':
      return 'Project ready';
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

export function parseNativeNotificationPreferences(value: unknown): NativeNotificationPreferences {
  if (!value || typeof value !== 'object') return { ...fallbackPreferences };
  const stored = value as Record<string, unknown>;
  const mode: NativeNotificationMode = stored.mode === 'all' || stored.mode === 'top_level' || stored.mode === 'project_ready'
    ? stored.mode
    : stored.waitForProject === true || stored.projectReady === true
      ? 'project_ready'
      : stored.topLevelOnly === false ? 'all' : 'top_level';
  return {
    enabled: typeof stored.enabled === 'boolean' ? stored.enabled : fallbackPreferences.enabled,
    needsInput: typeof stored.needsInput === 'boolean' ? stored.needsInput : fallbackPreferences.needsInput,
    mode,
    soundEnabled: typeof stored.soundEnabled === 'boolean' ? stored.soundEnabled
      : typeof stored.projectReadySound === 'boolean' ? stored.projectReadySound : fallbackPreferences.soundEnabled
  };
}

function loadPreferences(): NativeNotificationPreferences {
  try {
    return parseNativeNotificationPreferences(JSON.parse(localStorage.getItem(preferencesKey) ?? 'null'));
  } catch {
    return { ...fallbackPreferences };
  }
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
