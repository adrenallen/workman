import assert from 'node:assert/strict';
import { beforeEach, afterEach, test } from 'node:test';
import { mockIPC, mockWindows, clearMocks } from '@tauri-apps/api/mocks';
import { get } from 'svelte/store';
import { isAgentNotificationViewed, isTopLevelAgentNotification } from '../src/lib/notificationAttention.ts';
import {
  deliverNativeNotification, deliverNativeSystemNotification, dismissNativeNotifications, nativeNotificationPreferences,
  nativeNotificationRuntime, openNativeNotificationSettings, setNativeNotificationsEnabled, setNativeNotificationMode, syncDockUnreadBadge, keepNotificationDeliveryActive,
  setNotificationSoundEnabled, parseNativeNotificationPreferences
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
    if (command === 'native_notification_window_focused') return focused;
    return handle(command, args);
  });
  nativeNotificationPreferences.set({ enabled: true, needsInput: true, mode: 'top_level', soundEnabled: true });
  nativeNotificationRuntime.set({ permission: allowed, busy: false, error: null });
});
afterEach(() => { clearMocks(); delete globalThis.window; delete globalThis.localStorage; delete globalThis.document; });

const shown = () => calls.filter(({ command }) => command === 'native_notification_show');
const deferred = () => {
  let resolve;
  const promise = new Promise(done => { resolve = done; });
  return { promise, resolve };
};

const projectReady = id => ({ ...notification(id, 'project_ready'), process_id: null, body: 'Project 1 is ready for you.' });
const idleAgent = (id = 7, project_id = 1) => ({
  ...root, id, project_id, status: 'running', agent_state: {state:'idle', working:false}
});
const workingAgent = (id = 7, project_id = 1) => ({
  ...idleAgent(id, project_id), agent_state: {state:'working', working:true}
});

test('migrates legacy notification switches and sound while respecting explicit mode choices', () => {
  const defaults = {enabled:true, needsInput:true, mode:'top_level', soundEnabled:true};
  for (const value of [null, false, [], {}, {mode:'invalid', enabled:5, needsInput:'yes', soundEnabled:'yes'}]) {
    assert.deepEqual(parseNativeNotificationPreferences(value), defaults);
  }
  assert.deepEqual(parseNativeNotificationPreferences({topLevelOnly:false}), {...defaults, mode:'all'});
  for (const legacy of [{waitForProject:true}, {projectReady:true}, {projectReady:true,topLevelOnly:false}]) {
    assert.equal(parseNativeNotificationPreferences(legacy).mode, 'project_ready');
  }
  assert.deepEqual(parseNativeNotificationPreferences({enabled:false, needsInput:false, projectReadySound:false, waitForProject:true}), {
    enabled:false, needsInput:false, mode:'project_ready', soundEnabled:false
  });
  assert.deepEqual(parseNativeNotificationPreferences({mode:'all', soundEnabled:true, waitForProject:true, projectReadySound:false}), {...defaults,mode:'all'});
});

test('three exclusive modes select all agents, roots only, or ready projects', async () => {
  let id = 600;
  for (const [mode, expected] of [['all',[true,true,false]], ['top_level',[true,false,false]], ['project_ready',[false,false,true]]]) {
    setNativeNotificationMode(mode);
    const actual = [
      await deliverNativeNotification(notification(++id), [root]),
      await deliverNativeNotification(notification(++id), family),
      await deliverNativeNotification(projectReady(++id), [idleAgent()])
    ];
    assert.deepEqual(actual, expected, mode);
    assert.equal(await deliverNativeNotification(notification(++id, 'process_crashed'), family), true, 'urgent failures still alert');
  }
});

test('sound can be silenced for every computer alert without suppressing its banner', async () => {
  setNotificationSoundEnabled(false);
  assert.equal(await deliverNativeNotification(notification(650), [root]), true);
  assert.equal(await deliverNativeSystemNotification('Reminder', 'Take a break'), true);
  assert.ok(shown().every(({args}) => args.sound === false));
  setNotificationSoundEnabled(true);
  assert.equal(await deliverNativeNotification(notification(651), [root]), true);
  assert.equal(shown().at(-1).args.sound, true);
});

test('project-ready banners are opt-in and request a configurable system sound', async () => {
  assert.equal(await deliverNativeNotification(projectReady(301), [idleAgent()]), false);
  setNativeNotificationMode('project_ready');
  assert.equal(await deliverNativeNotification(projectReady(302), [idleAgent()]), true);
  assert.deepEqual(shown()[0].args, {notificationId:302, title:'Project ready', body:'Project 1 is ready for you.', sound:true});
  setNotificationSoundEnabled(false);
  assert.equal(await deliverNativeNotification(projectReady(303), [idleAgent()]), true);
  assert.equal(shown()[1].args.sound, false);
  setNativeNotificationsEnabled(false);
  assert.equal(await deliverNativeNotification(projectReady(304), [idleAgent()]), false);
  assert.equal(shown().length, 2);
});

test('waiting for the project replaces individual completion/input banners and persists the choice', async () => {
  let saved;
  globalThis.localStorage.setItem = (_key, value) => { saved = JSON.parse(value); };
  setNativeNotificationMode('project_ready');
  assert.equal(saved.mode, 'project_ready');
  assert.equal(await deliverNativeNotification(notification(305), [idleAgent()]), false);
  assert.equal(await deliverNativeNotification(notification(306, 'needs_input'), [idleAgent()]), false);
  assert.equal(await deliverNativeNotification(projectReady(307), [workingAgent()]), false);
  assert.equal(await deliverNativeNotification(projectReady(308), [idleAgent()]), true);
  assert.equal(await deliverNativeNotification(notification(309, 'timer_fired'), [workingAgent()]), true);
  assert.deepEqual(shown().map(({args}) => args.notificationId), [308, 309]);
  setNativeNotificationMode('top_level');
  assert.equal(saved.mode, 'top_level', 'switching modes restores individual completion alerts');
  assert.equal(await deliverNativeNotification(notification(310), [idleAgent()]), true);
});

test('project readiness includes children and starting agents, but excludes other projects and stopped work', async () => {
  setNativeNotificationMode('project_ready');
  const parent = {...idleAgent(), agent_state:{state:'waiting',working:false}};
  const child = {...workingAgent(8), spawned_by_process_id:7};
  assert.equal(await deliverNativeNotification(projectReady(311), [parent, child]), false);
  assert.equal(await deliverNativeNotification(projectReady(312), [parent, {...child, status:'starting', agent_state:{state:'idle',working:false}}]), false);
  assert.equal(await deliverNativeNotification(projectReady(313), [parent, {...child,status:'stopped'}, workingAgent(9, 2)]), true);
  assert.equal(await deliverNativeNotification(projectReady(314), []), false, 'deleted projects must not alert');
});

test('resuming project work while permission is pending prevents a stale ding', async () => {
  setNativeNotificationMode('project_ready');
  const requested = deferred();
  const permission = deferred();
  nativeNotificationRuntime.set({permission:{...allowed,state:'not_determined'},busy:false,error:null});
  handle = command => {
    if (command === 'native_notification_request_permission') { requested.resolve(); return permission.promise; }
  };
  let processes = [idleAgent()];
  const pending = deliverNativeNotification(projectReady(315), () => processes);
  await requested.promise;
  processes = [workingAgent()];
  permission.resolve(allowed);
  assert.equal(await pending, false);
  assert.equal(shown().length, 0);
});

test('queued agent delivery honors switching to project-only notifications', async () => {
  const requested = deferred();
  const permission = deferred();
  nativeNotificationRuntime.set({permission:{...allowed,state:'not_determined'},busy:false,error:null});
  handle = command => {
    if (command === 'native_notification_request_permission') { requested.resolve(); return permission.promise; }
  };
  const pending = deliverNativeNotification(notification(316), [idleAgent()]);
  await requested.promise;
  setNativeNotificationMode('project_ready');
  permission.resolve(allowed);
  assert.equal(await pending, false);
  assert.equal(shown().length, 0);
});

test('reading a project-ready notification clears its OS entry and prevents a repeat sound', async () => {
  setNativeNotificationMode('project_ready');
  const ready = projectReady(317);
  assert.equal(await deliverNativeNotification(ready, [idleAgent()]), true);
  await dismissNativeNotifications([317]);
  assert.equal(await deliverNativeNotification(ready, [idleAgent()]), false);
  assert.equal(shown().length, 1);
  assert.deepEqual(calls.at(-1).args.notificationIds, [317]);
});

test('opening system settings keeps permission denied until the OS reports a change', async () => {
  const denied = { ...allowed, state: 'denied' };
  nativeNotificationRuntime.set({ permission: denied, busy: false, error: 'Old error' });
  await openNativeNotificationSettings();
  assert.deepEqual(calls, [{ command: 'native_notification_open_settings', args: {} }]);
  assert.deepEqual(get(nativeNotificationRuntime), { permission: denied, busy: false, error: null });
  assert.equal(get(nativeNotificationPreferences).enabled, true);
});

test('settings launch failures stay visible and can be retried without changing preferences', async () => {
  nativeNotificationRuntime.set({ permission: { ...allowed, state: 'denied' }, busy: false, error: null });
  handle = () => { throw new Error('The desktop notification settings app could not be found.'); };
  await assert.rejects(openNativeNotificationSettings(), /could not be found/);
  assert.match(get(nativeNotificationRuntime).error, /could not be found/);
  assert.equal(get(nativeNotificationRuntime).permission.state, 'denied');
  handle = () => undefined;
  await openNativeNotificationSettings();
  assert.equal(get(nativeNotificationRuntime).error, null);
  assert.equal(get(nativeNotificationPreferences).enabled, true);
});

test('a selected agent stays unread while switched away, minimized, or behind another view', () => {
  assert.equal(isAgentNotificationViewed(7, false, true, 7), false);
  assert.equal(isAgentNotificationViewed(7, true, false, 7), false);
  assert.equal(isAgentNotificationViewed(7, true, true, null), false);
  assert.equal(isAgentNotificationViewed(7, true, true, 8), false);
  assert.equal(isAgentNotificationViewed(7, true, true, 7), true);
});

test('an inactive WebView delivers the selected agent banner with sound even if native key focus is stale', async () => {
  focused = true;
  globalThis.document = { hidden: false, hasFocus: () => false };
  assert.equal(await deliverNativeNotification(notification(680), [root]), true);
  assert.equal(shown()[0].args.sound, true);
});

test('a cancelled scheduled test cannot send after permission finishes', async () => {
  const requested = deferred();
  const permission = deferred();
  let current = true;
  nativeNotificationRuntime.set({ permission: { ...allowed, state: 'not_determined' }, busy: false, error: null });
  handle = command => {
    if (command === 'native_notification_request_permission') { requested.resolve(); return permission.promise; }
  };
  const pending = deliverNativeSystemNotification('Test', 'Notification sound test', () => current);
  await requested.promise;
  current = false;
  permission.resolve(allowed);
  assert.equal(await pending, false);
  assert.equal(shown().length, 0);
});

test('in-app only persists, suppresses computer alerts, and clears badges without reading notifications', async () => {
  const unread = notification(101);
  assert.equal(await deliverNativeNotification(unread, [root]), true);
  assert.equal(shown()[0].args.notificationId, 101);
  assert.equal(shown()[0].args.title, 'Agent finished');
  await syncDockUnreadBadge(2);
  let saved;
  globalThis.localStorage.setItem = (_key, value) => { saved = JSON.parse(value); };
  setNativeNotificationsEnabled(false);
  assert.deepEqual(saved, { enabled: false, needsInput: true, mode: 'top_level', soundEnabled: true });
  await syncDockUnreadBadge(2);
  const badges = calls.filter(({ command }) => command === 'native_notification_set_badge');
  assert.deepEqual(badges.map(({ args }) => args.count), [2, 0]);
  assert.equal(await deliverNativeNotification(notification(102), [root]), false);
  assert.equal(await deliverNativeSystemNotification('Reminder', 'Take a break'), false);
  assert.equal(shown().length, 1);
  assert.equal(unread.read_at, null);
  assert.equal(calls.some(({ command }) => command === 'native_notification_request_permission'), false);
  setNativeNotificationsEnabled(true);
  await syncDockUnreadBadge(2);
  assert.equal(calls.at(-1).args.count, 2);
  assert.equal(await deliverNativeNotification(notification(112), [root]), true);
});

test('switching to in-app only during permission requests cancels pending agent and system alerts', async () => {
  const requested = deferred();
  const permission = deferred();
  nativeNotificationRuntime.set({ permission: { ...allowed, state: 'not_determined' }, busy: false, error: null });
  handle = command => {
    if (command === 'native_notification_request_permission') {
      requested.resolve();
      return permission.promise;
    }
  };
  const system = deliverNativeSystemNotification('Reminder', 'Take a break');
  const agent = deliverNativeNotification(notification(113), [root]);
  await requested.promise;
  setNativeNotificationsEnabled(false);
  permission.resolve(allowed);
  assert.deepEqual(await Promise.all([system, agent]), [false, false]);
  assert.equal(shown().length, 0);
});

test('queued computer notifications recheck the mode after earlier delivery finishes', async () => {
  const started = deferred();
  const sending = deferred();
  handle = command => {
    if (command === 'native_notification_show') { started.resolve(); return sending.promise; }
  };
  const first = deliverNativeSystemNotification('First', 'Already sending');
  await started.promise;
  const queued = deliverNativeSystemNotification('Second', 'Waiting');
  setNativeNotificationsEnabled(false);
  sending.resolve();
  assert.deepEqual(await Promise.all([first, queued]), [true, false]);
  assert.equal(shown().length, 1);
});

test('in-app only prevents a permission prompt after a pending status check', async () => {
  const checking = deferred();
  const permission = deferred();
  nativeNotificationRuntime.set({ permission: { ...allowed, state: 'checking' }, busy: false, error: null });
  handle = command => {
    if (command === 'native_notification_permission_state') {
      checking.resolve();
      return permission.promise;
    }
  };
  const pending = deliverNativeSystemNotification('Reminder', 'Take a break');
  await checking.promise;
  setNativeNotificationsEnabled(false);
  permission.resolve({ ...allowed, state: 'not_determined' });
  assert.equal(await pending, false);
  assert.equal(calls.some(({ command }) => command === 'native_notification_request_permission'), false);
});

test('top-level filtering leaves other notification types alone and has a persistent opt-out', async () => {
  assert.equal(await deliverNativeNotification(notification(103), family), false);
  assert.equal(await deliverNativeNotification(notification(104, 'needs_input'), family), false);
  assert.equal(isTopLevelAgentNotification(notification(105, 'timer_fired'), [child]), true);
  assert.equal(isTopLevelAgentNotification(notification(106), []), true);
  assert.equal(isTopLevelAgentNotification(notification(106), [child]), true, 'an orphan promoted to a root keeps its banner');
  let saved;
  globalThis.localStorage.setItem = (_key, value) => { saved = JSON.parse(value); };
  setNativeNotificationMode('all');
  assert.equal(saved.mode, 'all');
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
  assert.deepEqual(calls.filter(({ command }) => ['native_notification_show', 'native_notification_dismiss'].includes(command)).map(({ command }) => command), [
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

test('older desktop builds fall back to the existing badge command', async () => {
  handle = command => {
    if (command === 'native_notification_set_badge') throw new Error('unknown command');
  };
  await syncDockUnreadBadge(3);
  await syncDockUnreadBadge(0);
  setNativeNotificationsEnabled(false);
  await syncDockUnreadBadge(3);
  assert.deepEqual(calls.filter(({ command }) => command === 'plugin:window|set_badge_count').map(({ args }) => args.value), [3, undefined, undefined]);
});

test('background notification activity releases its Web Lock when the app unmounts', async () => {
  const previous = Object.getOwnPropertyDescriptor(globalThis, 'navigator');
  let signal;
  let held;
  Object.defineProperty(globalThis, 'navigator', { configurable: true, value: { locks: {
    request: (_name, options, callback) => {
      assert.equal(options.mode, 'shared');
      signal = options.signal;
      held = callback();
      return held;
    }
  } } });
  try {
    const stop = keepNotificationDeliveryActive();
    assert.equal(signal.aborted, false);
    let released = false;
    void held.then(() => { released = true; });
    await Promise.resolve();
    assert.equal(released, false);
    stop();
    await held;
    assert.equal(released, true);
    assert.equal(signal.aborted, true);
  } finally {
    if (previous) Object.defineProperty(globalThis, 'navigator', previous);
    else delete globalThis.navigator;
  }
});
