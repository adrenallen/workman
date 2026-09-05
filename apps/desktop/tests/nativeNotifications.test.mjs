import assert from 'node:assert/strict';
import { beforeEach, afterEach, test } from 'node:test';
import { mockIPC, mockWindows, clearMocks } from '@tauri-apps/api/mocks';
import { get } from 'svelte/store';
import { isAgentNotificationViewed, isTopLevelAgentNotification } from '../src/lib/notificationAttention.ts';
import {
  deliverNativeNotification, dismissNativeNotifications, nativeNotificationPreferences,
  nativeNotificationRuntime, setTopLevelNotificationsOnly, syncDockUnreadBadge
} from '../src/lib/nativeNotifications.ts';

const allowed = { state: 'granted', platform: 'macos', detail: null };
let focused;
let calls;
let handle;
const notification = (id, type = 'agent_done') => ({
  id, type, process_id: 7, project_id: 1, todo_id: null, comment_id: null,
  body: 'Builder finished', created_at: 1, read_at: null
});
const root = { id: 7, kind: 'agent', spawned_by_process_id: null };
const child = { ...root, spawned_by_process_id: 3 };
const family = [child, { ...root, id: 3 }];

beforeEach(() => {
  globalThis.window = {};
  globalThis.localStorage = { setItem() {} };
  focused = false;
  calls = [];
  handle = () => undefined;
  mockWindows('main');
  mockIPC(async (command, args) => {
    calls.push({ command, args });
    if (command === 'plugin:window|is_focused') return focused;
    return handle(command, args);
  });
  nativeNotificationPreferences.set({ enabled: true, needsInput: true, topLevelOnly: true });
  nativeNotificationRuntime.set({ permission: allowed, busy: false, error: null });
});
afterEach(() => { clearMocks(); delete globalThis.window; delete globalThis.localStorage; });

const shown = () => calls.filter(({ command }) => command === 'native_notification_show');
const deferred = () => {
  let resolve;
  const promise = new Promise(done => { resolve = done; });
  return { promise, resolve };
};

test('a selected agent stays unread while switched away, minimized, or behind another view', () => {
  assert.equal(isAgentNotificationViewed(7, false, true, 7), false);
  assert.equal(isAgentNotificationViewed(7, true, false, 7), false);
  assert.equal(isAgentNotificationViewed(7, true, true, null), false);
  assert.equal(isAgentNotificationViewed(7, true, true, 8), false);
  assert.equal(isAgentNotificationViewed(7, true, true, 7), true);
});

test('background completion gets a banner and Dock count clears independently of banner preference', async () => {
  assert.equal(await deliverNativeNotification(notification(101), [root]), true);
  assert.equal(shown()[0].args.notificationId, 101);
  assert.equal(shown()[0].args.title, 'Agent finished');
  nativeNotificationPreferences.update(value => ({ ...value, enabled: false }));
  await syncDockUnreadBadge(2);
  await syncDockUnreadBadge(0);
  const badges = calls.filter(({ command }) => command === 'plugin:window|set_badge_count');
  assert.deepEqual(badges.map(({ args }) => args.value), [2, undefined]);
  assert.equal(await deliverNativeNotification(notification(102), [root]), false);
});

test('top-level filtering leaves other notification types alone and has a persistent opt-out', async () => {
  assert.equal(await deliverNativeNotification(notification(103), family), false);
  assert.equal(await deliverNativeNotification(notification(104, 'needs_input'), family), false);
  assert.equal(isTopLevelAgentNotification(notification(105, 'timer_fired'), [child]), true);
  assert.equal(isTopLevelAgentNotification(notification(106), []), true);
  assert.equal(isTopLevelAgentNotification(notification(106), [child]), true, 'an orphan promoted to a root keeps its banner');
  let saved;
  globalThis.localStorage.setItem = (_key, value) => { saved = JSON.parse(value); };
  setTopLevelNotificationsOnly(false);
  assert.equal(saved.topLevelOnly, false);
  assert.equal(get(nativeNotificationPreferences).needsInput, true);
  assert.equal(await deliverNativeNotification(notification(107), family), true);
});

test('returning to the app while the permission sheet is open suppresses the stale banner', async () => {
  const requested = deferred();
  const permission = deferred();
  nativeNotificationRuntime.set({ permission: { ...allowed, state: 'not_determined' }, busy: false, error: null });
  handle = command => {
    if (command === 'native_notification_request_permission') {
      requested.resolve();
      return permission.promise;
    }
  };
  const pending = deliverNativeNotification(notification(108), [root]);
  await requested.promise;
  focused = true;
  permission.resolve(allowed);
  assert.equal(await pending, false);
  assert.equal(shown().length, 0);
});

test('reading while permission is pending prevents a late banner even if the window stays unfocused', async () => {
  const requested = deferred();
  const permission = deferred();
  nativeNotificationRuntime.set({ permission: { ...allowed, state: 'not_determined' }, busy: false, error: null });
  handle = command => {
    if (command === 'native_notification_request_permission') {
      requested.resolve();
      return permission.promise;
    }
  };
  const pending = deliverNativeNotification(notification(109), [root]);
  await requested.promise;
  await dismissNativeNotifications([109]);
  permission.resolve(allowed);
  assert.equal(await pending, false);
  assert.equal(shown().length, 0);
});

test('a read racing with native delivery removes the notification after delivery completes', async () => {
  const started = deferred();
  const sending = deferred();
  handle = command => {
    if (command === 'native_notification_show') { started.resolve(); return sending.promise; }
  };
  const pending = deliverNativeNotification(notification(110), [root]);
  await started.promise;
  const removal = dismissNativeNotifications([110, 110]);
  assert.equal(calls.some(({ command }) => command === 'native_notification_dismiss'), false);
  sending.resolve();
  await pending;
  await removal;
  assert.deepEqual(calls.filter(({ command }) => command.startsWith('native_notification_')).map(({ command }) => command), [
    'native_notification_show', 'native_notification_dismiss'
  ]);
  assert.deepEqual(calls.at(-1).args.notificationIds, [110]);
});

test('failed macOS removal retries on the next read sync and never re-delivers the read item', async () => {
  let attempts = 0;
  handle = command => {
    if (command === 'native_notification_dismiss' && ++attempts === 1) throw new Error('temporarily unavailable');
  };
  await dismissNativeNotifications([111]);
  assert.equal(get(nativeNotificationRuntime).error, 'temporarily unavailable');
  assert.equal(await deliverNativeNotification(notification(111), [root]), false);
  await dismissNativeNotifications([111]);
  await dismissNativeNotifications([111]);
  assert.equal(attempts, 2);
});
