import assert from 'node:assert/strict';
import { beforeEach, afterEach, test } from 'node:test';
import { mockIPC, clearMocks } from '@tauri-apps/api/mocks';
import { get } from 'svelte/store';
import { notificationSound, refreshNotificationSound, chooseNotificationSound, resetNotificationSound, selectNotificationSound, previewNotificationSound } from '../src/lib/notificationSound.ts';

const system = {supported:true,preset:'system',name:null,detail:null};
const custom = {...system,preset:'custom',name:'My ding.wav'};
const doom = {...system,preset:'doom',name:'Doom'};
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

test('unsupported platforms cannot import or select Doom, but can query the fallback', async () => {
  const windows = {...system,supported:false,detail:'Windows uses the system sound.'};
  handle = () => windows;
  await refreshNotificationSound();
  await chooseNotificationSound();
  await selectNotificationSound('doom');
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
  await selectNotificationSound('doom');
  assert.equal(calls.length, 1);
  resolve('/tmp/My ding.wav');
  await choosing;
  assert.deepEqual(calls.map(call => call.command), ['plugin:dialog|open','native_notification_import_sound']);
  assert.deepEqual(get(notificationSound), {info:custom,busy:false,error:null});
});

test('selects the bundled Doom sound without a file dialog and restores the system default', async () => {
  handle = (_command, args) => args.preset === 'doom' ? doom : system;
  await selectNotificationSound('doom');
  assert.deepEqual(calls, [{command:'native_notification_select_sound',args:{preset:'doom'}}]);
  assert.deepEqual(get(notificationSound).info, doom);
  await selectNotificationSound('system');
  assert.deepEqual(calls.at(-1), {command:'native_notification_select_sound',args:{preset:'system'}});
  assert.deepEqual(get(notificationSound).info, system);
});

test('a failed preset selection retains the saved sound and can be retried', async () => {
  notificationSound.set({info:custom,busy:false,error:null});
  handle = () => { throw new Error('Could not save the notification sound'); };
  await selectNotificationSound('doom');
  assert.deepEqual(get(notificationSound).info, custom);
  assert.equal(get(notificationSound).busy, false);
  assert.match(get(notificationSound).error, /Could not save/);
  handle = () => doom;
  await selectNotificationSound('doom');
  assert.deepEqual(get(notificationSound), {info:doom,busy:false,error:null});
});

test('previews the saved sound without changing it, requesting permission, or sending a banner', async () => {
  for (const info of [system, doom, custom, { ...system, supported: false }]) {
    notificationSound.set({ info, busy: false, error: null });
    calls = [];
    handle = () => null;
    await previewNotificationSound();
    assert.deepEqual(calls, [{ command: 'native_notification_preview_sound', args: {} }]);
    assert.deepEqual(get(notificationSound), { info, busy: false, error: null });
  }
});

test('preview waits for loaded settings and prevents overlapping playback or selection changes', async () => {
  notificationSound.set({ info: null, busy: false, error: null });
  await previewNotificationSound();
  assert.equal(calls.length, 0);
  notificationSound.set({ info: doom, busy: false, error: null });
  let finish;
  handle = () => new Promise(resolve => { finish = resolve; });
  const playing = previewNotificationSound();
  assert.equal(get(notificationSound).busy, true);
  await previewNotificationSound();
  await selectNotificationSound('system');
  await chooseNotificationSound();
  assert.equal(calls.length, 1);
  finish(null);
  await playing;
  assert.deepEqual(get(notificationSound), { info: doom, busy: false, error: null });
});

test('preview failures remain visible and retryable without losing the selected sound', async () => {
  notificationSound.set({ info: custom, busy: false, error: null });
  handle = () => { throw new Error('Audio player is unavailable.'); };
  await previewNotificationSound();
  assert.match(get(notificationSound).error, /Audio player is unavailable/);
  assert.equal(get(notificationSound).busy, false);
  assert.deepEqual(get(notificationSound).info, custom);
  handle = () => null;
  await previewNotificationSound();
  assert.deepEqual(get(notificationSound), { info: custom, busy: false, error: null });
});

test('volume writes share the sound operation guard and preserve saved state on errors', async () => {
  const { setNotificationSoundVolume } = await import('../src/lib/notificationSound.ts');
  await setNotificationSoundVolume(50);
  assert.equal(calls.length, 0);
  const adjustable = { ...doom, volume: 100, volume_supported: true };
  notificationSound.set({ info: adjustable, busy: false, error: null });
  for (const invalid of [-1, 101, NaN, Infinity]) await setNotificationSoundVolume(invalid);
  assert.equal(calls.length, 0);
  let finish;
  handle = () => new Promise(resolve => { finish = resolve; });
  const saving = setNotificationSoundVolume(49.8);
  await previewNotificationSound();
  await setNotificationSoundVolume(70);
  assert.deepEqual(calls, [{ command: 'native_notification_set_sound_volume', args: { volume: 50 } }]);
  finish({ ...adjustable, volume: 50 });
  await saving;
  assert.equal(get(notificationSound).info.volume, 50);
  handle = () => { throw new Error('Could not save volume'); };
  await setNotificationSoundVolume(20);
  assert.equal(get(notificationSound).info.volume, 50);
  assert.match(get(notificationSound).error, /Could not save volume/);
});
