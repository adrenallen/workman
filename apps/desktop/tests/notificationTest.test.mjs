import assert from 'node:assert/strict';
import { beforeEach, afterEach, test } from 'node:test';
import { mockIPC, clearMocks } from '@tauri-apps/api/mocks';
import { createNotificationTest } from '../src/lib/notificationTest.ts';
import { nativeNotificationPreferences } from '../src/lib/nativeNotifications.ts';

let calls, handle, updates, controller, status, focus;
const granted = { state: 'granted', platform: 'macos', detail: null };
const deferred = () => { let resolve; const promise = new Promise(done => { resolve = done; }); return { promise, resolve }; };
beforeEach(() => {
  globalThis.window = {
    addEventListener(event, listener) { if (event === 'focus') focus = listener; },
    removeEventListener() { focus = undefined; }
  };
  calls = []; updates = []; status = { id: null, phase: 'idle', error: null };
  handle = command => command === 'native_notification_permission_state' ? granted : status;
  mockIPC(async (command, args) => {
    calls.push({ command, args });
    if (command === 'native_notification_schedule_test') status = { id: args.testId, phase: 'scheduled', error: null };
    return handle(command, args);
  });
  nativeNotificationPreferences.set({ enabled: true, soundEnabled: true, mode: 'top_level', needsInput: true });
  controller = createNotificationTest((pending, message) => updates.push({ pending, message }));
});
afterEach(() => { clearMocks(); delete globalThis.window; });

test('schedules immediately in native code and restores the native result when focus returns', async () => {
  const stop = controller.watch();
  await controller.schedule();
  const scheduled = calls.find(call => call.command === 'native_notification_schedule_test');
  assert.equal(scheduled.args.sound, true);
  assert.match(scheduled.args.testId, /^[\da-f-]{36}$/);
  assert.deepEqual(updates.at(-1), { pending: true, message: 'Switch to another app. The test will be sent in 5 seconds.' });
  status = { ...status, phase: 'sent' };
  focus();
  await new Promise(resolve => setImmediate(resolve));
  assert.deepEqual(updates.at(-1), { pending: false, message: 'Test accepted by your operating system. If no banner or sound appeared, check the notification settings below.' });
  stop();
  assert.equal(focus, undefined);
});

test('requests permission before starting the countdown and reports a denial without scheduling', async () => {
  handle = command => command === 'native_notification_permission_state' ? { ...granted, state: 'not_determined' }
    : { ...granted, state: 'denied', detail: 'Enable notifications in system settings.' };
  await controller.schedule();
  assert.deepEqual(calls.map(call => call.command), ['native_notification_permission_state', 'native_notification_request_permission']);
  assert.equal(updates.at(-1).pending, false);
  assert.match(updates.at(-1).message, /Enable notifications/);
});

test('cancelling while permission is pending prevents a late native schedule', async () => {
  const permission = deferred();
  handle = () => permission.promise;
  const scheduling = controller.schedule();
  controller.cancel();
  permission.resolve(granted);
  await scheduling;
  assert.ok(!calls.some(call => call.command === 'native_notification_schedule_test'));
  assert.deepEqual(updates.at(-1), { pending: false, message: '' });
});

test('cancels again when a pending native schedule registers after cancellation', async () => {
  const registered = deferred();
  handle = command => command === 'native_notification_permission_state' ? granted
    : command === 'native_notification_schedule_test' ? registered.promise : status;
  const scheduling = controller.schedule();
  await new Promise(resolve => setImmediate(resolve));
  controller.cancel();
  registered.resolve(status);
  await scheduling;
  const cancels = calls.filter(call => call.command === 'native_notification_cancel_test');
  assert.equal(cancels.length, 2);
  assert.equal(cancels[0].args.testId, cancels[1].args.testId);
  assert.deepEqual(updates.at(-1), { pending: false, message: '' });
});

test('in-app-only disables tests; failed native delivery is visible and retryable', async () => {
  nativeNotificationPreferences.update(value => ({ ...value, enabled: false }));
  await controller.schedule();
  assert.equal(calls.length, 0);
  nativeNotificationPreferences.update(value => ({ ...value, enabled: true, soundEnabled: false }));
  handle = command => command === 'native_notification_permission_state' ? granted
    : command === 'native_notification_test_state' ? { ...status, phase: 'error', error: 'Notification service unavailable.' } : status;
  await controller.schedule();
  assert.equal(calls.find(call => call.command === 'native_notification_schedule_test').args.sound, false);
  assert.equal(updates.at(-1).pending, false);
  assert.match(updates.at(-1).message, /Notification service unavailable/);
  handle = command => command === 'native_notification_permission_state' ? granted : status;
  await controller.schedule();
  assert.equal(updates.at(-1).pending, true);
});

test('in-flight delivery stays pending and a stale scheduled reply cannot undo completion', async () => {
  const stop = controller.watch();
  await controller.schedule();
  status = { ...status, phase: 'delivering' };
  focus();
  await new Promise(resolve => setImmediate(resolve));
  assert.deepEqual(updates.at(-1), { pending: true, message: 'Sending test to your operating system…' });
  const stale = deferred();
  handle = () => stale.promise;
  focus();
  handle = () => ({ ...status, phase: 'sent' });
  focus();
  await new Promise(resolve => setImmediate(resolve));
  stale.resolve({ ...status, phase: 'scheduled' });
  await new Promise(resolve => setImmediate(resolve));
  assert.deepEqual(updates.at(-1), { pending: false, message: 'Test accepted by your operating system. If no banner or sound appeared, check the notification settings below.' });
  stop();
});

test('changing computer/sound preferences cancels pending native tests', async () => {
  const stop = controller.watch();
  await controller.schedule();
  const firstId = status.id;
  nativeNotificationPreferences.update(value => ({ ...value, soundEnabled: false }));
  assert.ok(calls.some(call => call.command === 'native_notification_cancel_test' && call.args.testId === firstId));
  assert.deepEqual(updates.at(-1), { pending: false, message: '' });
  await controller.schedule();
  const secondId = status.id;
  nativeNotificationPreferences.update(value => ({ ...value, enabled: false }));
  assert.ok(calls.some(call => call.command === 'native_notification_cancel_test' && call.args.testId === secondId));
  assert.deepEqual(updates.at(-1), { pending: false, message: '' });
  stop();
});

test('a native delivery already committed remains visible after cancellation', async () => {
  const stop = controller.watch();
  await controller.schedule();
  status = { ...status, phase: 'delivering' };
  controller.cancel();
  await new Promise(resolve => setImmediate(resolve));
  assert.deepEqual(updates.at(-1), { pending: true, message: 'Sending test to your operating system…' });
  status = { ...status, phase: 'sent' };
  focus();
  await new Promise(resolve => setImmediate(resolve));
  assert.deepEqual(updates.at(-1), { pending: false, message: 'Test accepted by your operating system. If no banner or sound appeared, check the notification settings below.' });
  stop();
  await new Promise(resolve => setImmediate(resolve));
  assert.deepEqual(updates.at(-1), { pending: false, message: '' });
});
