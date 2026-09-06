import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { get } from 'svelte/store';
import {
  nativeNotificationPreferences, refreshNativeNotificationPermission,
  requestNativeNotificationPermission
} from './nativeNotifications.ts';

interface TestStatus {
  id: string | null;
  phase: 'idle' | 'scheduled' | 'delivering' | 'sent' | 'error' | 'cancelled';
  error: string | null;
}

/** Native code owns the deadline and retains the result while the WebView is suspended. */
export function createNotificationTest(onChange: (pending: boolean, message: string) => void) {
  let id: string | null = null;
  let completed = false;
  let disposed = false;
  let generation = 0;
  const enabled = () => get(nativeNotificationPreferences).enabled;
  const cancelNative = (testId: string) => invoke<TestStatus>('native_notification_cancel_test', { testId }).catch(() => undefined);
  const apply = (status: TestStatus) => {
    if (!id || status.id !== id) return;
    const pending = status.phase === 'scheduled' || status.phase === 'delivering';
    if (pending && completed) return;
    if (!pending) completed = true;
    const message = status.phase === 'delivering' ? 'Sending test to your operating system…'
      : pending ? 'Switch to another app. The test will be sent in 5 seconds.'
      : status.phase === 'sent' ? 'Test sent to your operating system.'
      : status.phase === 'error' ? `Test not sent. ${status.error ?? 'Check notification permission.'}` : '';
    onChange(pending, message);
  };
  const cancel = () => {
    const previous = id;
    const cancelledGeneration = ++generation;
    id = null;
    onChange(false, '');
    if (previous) void cancelNative(previous).then((status) => {
      if (disposed || id || generation !== cancelledGeneration || status?.phase !== 'delivering') return;
      // OS submission has already committed. Keep its result visible when the card is still open.
      id = previous;
      apply(status);
      void invoke<TestStatus>('native_notification_test_state').then(apply).catch(() => undefined);
    });
  };
  const schedule = async () => {
    cancel();
    if (!enabled()) return;
    const testId = crypto.randomUUID();
    id = testId;
    completed = false;
    const current = () => id === testId && enabled();
    onChange(true, 'Preparing notification test…');
    try {
      // Resolve permission while the user is here, before starting the native countdown.
      let permission = await refreshNativeNotificationPermission();
      if (!current()) return;
      if (permission.state === 'not_determined') permission = await requestNativeNotificationPermission();
      if (!current()) return;
      if (permission.state !== 'granted') throw new Error(permission.detail ?? 'Allow notifications in system settings.');
      const status = await invoke<TestStatus>('native_notification_schedule_test', {
        testId, sound: get(nativeNotificationPreferences).soundEnabled
      });
      // Cancellation can arrive before the native command is registered. Cancel again once it is.
      if (!current()) { await cancelNative(testId); return; }
      apply(status);
      apply(await invoke<TestStatus>('native_notification_test_state'));
    } catch (cause) {
      if (current()) apply({ id: testId, phase: 'error', error: cause instanceof Error ? cause.message : String(cause) });
    }
  };
  const watch = () => {
    disposed = false;
    let lastSound = get(nativeNotificationPreferences).soundEnabled;
    const stopPreferences = nativeNotificationPreferences.subscribe((preferences) => {
      if (!preferences.enabled || preferences.soundEnabled !== lastSound) cancel();
      lastSound = preferences.soundEnabled;
    });
    const refresh = () => {
      if (id) void invoke<TestStatus>('native_notification_test_state').then(apply).catch(() => undefined);
    };
    // Recover cached completion if background WebView event delivery was paused.
    window.addEventListener('focus', refresh);
    const unlisten = listen<TestStatus>('notification://test', ({ payload }) => { if (!disposed) apply(payload); })
      .catch(() => () => {});
    return () => {
      disposed = true;
      stopPreferences();
      cancel();
      window.removeEventListener('focus', refresh);
      void unlisten.then((stop) => stop()).catch(() => undefined);
    };
  };
  return { schedule, cancel, watch };
}
