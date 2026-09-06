import assert from 'node:assert/strict';
import { beforeEach, afterEach, test } from 'node:test';
import { mockIPC, clearMocks } from '@tauri-apps/api/mocks';
import { get } from 'svelte/store';
import { notificationSound, refreshNotificationSound, chooseNotificationSound, resetNotificationSound } from '../src/lib/notificationSound.ts';

const system = {supported:true,name:null,detail:null};
const custom = {...system,name:'My ding.wav'};
let calls, handle;
beforeEach(() => {
  globalThis.window = {};
  calls = [];
  handle = () => system;
  notificationSound.set({info:system,busy:false,error:null});
  mockIPC((command,args) => { calls.push({command,args}); return handle(command,args); });
});
afterEach(() => { clearMocks(); delete globalThis.window; });

test('loads the persisted native choice, imports a selected file, and resets to system default', async () => {
  handle = command => command === 'plugin:dialog|open' ? '/tmp/My ding.wav' : custom;
  await refreshNotificationSound();
  assert.deepEqual(get(notificationSound).info, custom);
  await chooseNotificationSound();
  assert.deepEqual(calls.at(-1), {command:'native_notification_import_sound',args:{path:'/tmp/My ding.wav'}});
  const dialog = calls.find(call => call.command === 'plugin:dialog|open');
  assert.deepEqual(dialog.args.options.filters, [{name:'WAV audio',extensions:['wav']}]);
  handle = () => system;
  await resetNotificationSound();
  assert.equal(calls.at(-1).command, 'native_notification_reset_sound');
  assert.deepEqual(get(notificationSound), {info:system,busy:false,error:null});
});

test('cancelled selection does not replace the sound and invalid files preserve the previous choice', async () => {
  notificationSound.set({info:custom,busy:false,error:null});
  handle = () => null;
  await chooseNotificationSound();
  assert.deepEqual(get(notificationSound).info, custom);
  assert.equal(calls.length, 1);
  handle = command => {
    if (command === 'plugin:dialog|open') return '/tmp/invalid.wav';
    throw new Error('Choose a non-empty sound shorter than 30 seconds.');
  };
  await chooseNotificationSound();
  assert.equal(get(notificationSound).busy, false);
  assert.match(get(notificationSound).error, /shorter than 30 seconds/);
  assert.deepEqual(get(notificationSound).info, custom);
  handle = () => system;
  await resetNotificationSound();
  assert.equal(get(notificationSound).error, null);
});

test('unsupported platforms cannot open the import dialog, but can query the fallback', async () => {
  const windows = {supported:false,name:null,detail:'Windows uses the system sound.'};
  handle = () => windows;
  await refreshNotificationSound();
  await chooseNotificationSound();
  assert.deepEqual(calls.map(call => call.command), ['native_notification_sound_state']);
  assert.deepEqual(get(notificationSound).info, windows);
});

test('only one sound operation runs while the dialog or import is pending', async () => {
  let resolve;
  const pending = new Promise(done => { resolve = done; });
  handle = command => command === 'plugin:dialog|open' ? pending : custom;
  const choosing = chooseNotificationSound();
  assert.equal(get(notificationSound).busy, true);
  await chooseNotificationSound();
  await resetNotificationSound();
  assert.equal(calls.length, 1);
  resolve('/tmp/My ding.wav');
  await choosing;
  assert.deepEqual(calls.map(call => call.command), ['plugin:dialog|open','native_notification_import_sound']);
  assert.deepEqual(get(notificationSound), {info:custom,busy:false,error:null});
});
