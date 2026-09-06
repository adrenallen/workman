import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { get, writable } from 'svelte/store';

export interface NotificationSoundInfo {
  supported: boolean;
  preset: 'system' | 'doom' | 'custom';
  name: string | null;
  detail: string | null;
}

export const notificationSound = writable<{
  info: NotificationSoundInfo | null;
  busy: boolean;
  error: string | null;
}>({ info: null, busy: false, error: null });

async function updateSound(action: () => Promise<NotificationSoundInfo | null>): Promise<void> {
  if (get(notificationSound).busy) return;
  notificationSound.update((current) => ({ ...current, busy: true, error: null }));
  try {
    const info = await action();
    notificationSound.update((current) => ({ ...current, info: info ?? current.info }));
  } catch (cause) {
    notificationSound.update((current) => ({ ...current, error: cause instanceof Error ? cause.message : String(cause) }));
  } finally {
    notificationSound.update((current) => ({ ...current, busy: false }));
  }
}

export function refreshNotificationSound(): Promise<void> {
  return updateSound(() => invoke<NotificationSoundInfo>('native_notification_sound_state'));
}

/** A deliberate preview is available even when automatic notifications or their sound are off. */
export function previewNotificationSound(): Promise<void> {
  if (!get(notificationSound).info) return Promise.resolve();
  return updateSound(async () => {
    await invoke('native_notification_preview_sound');
    return null;
  });
}

export function selectNotificationSound(preset: 'system' | 'doom'): Promise<void> {
  if (preset === 'doom' && !get(notificationSound).info?.supported) return Promise.resolve();
  return updateSound(() => invoke<NotificationSoundInfo>('native_notification_select_sound', { preset }));
}

export function chooseNotificationSound(): Promise<void> {
  if (!get(notificationSound).info?.supported) return Promise.resolve();
  return updateSound(async () => {
    const path = await open({
      title: 'Choose a notification sound',
      multiple: false,
      directory: false,
      filters: [{ name: 'WAV audio', extensions: ['wav'] }]
    });
    if (typeof path !== 'string') return null;
    return invoke<NotificationSoundInfo>('native_notification_import_sound', { path });
  });
}

export function resetNotificationSound(): Promise<void> {
  return updateSound(() => invoke<NotificationSoundInfo>('native_notification_reset_sound'));
}
