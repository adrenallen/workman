import assert from 'node:assert/strict';
import { afterEach, beforeEach, test } from 'node:test';
import { mockIPC, clearMocks } from '@tauri-apps/api/mocks';
import { createAgentReadDwell, isWorkmanWindowFocused } from '../src/lib/windowAttention.ts';

let now, nextId, timers, originalSet, originalClear;
beforeEach(() => {
  globalThis.window = {};
  now = 0;
  nextId = 0;
  timers = new Map();
  originalSet = globalThis.setTimeout;
  originalClear = globalThis.clearTimeout;
  globalThis.setTimeout = (callback, delay) => {
    const id = ++nextId;
    timers.set(id, { callback, at: now + delay });
    return id;
  };
  globalThis.clearTimeout = id => timers.delete(id);
});
afterEach(() => {
  globalThis.setTimeout = originalSet;
  globalThis.clearTimeout = originalClear;
  clearMocks();
  delete globalThis.document;
  delete globalThis.window;
});
function advance(ms) {
  now += ms;
  for (const [id, timer] of [...timers]) {
    if (timer.at <= now) {
      timers.delete(id);
      timer.callback();
    }
  }
}
const agent = { processId: 7, projectId: 1 };

test('selected unread output needs three continuous seconds of viewing despite status refreshes', () => {
  const read = [];
  const dwell = createAgentReadDwell(target => read.push(target));
  dwell.update(agent);
  advance(1_000);
  dwell.update({ ...agent });
  advance(1_999);
  assert.equal(read.length, 0);
  advance(1);
  assert.deepEqual(read, [agent]);
  dwell.update({ ...agent });
  advance(5_000);
  assert.equal(read.length, 1);
});

test('blur, hidden views, or switching agents reset the dwell and never read the previous agent', () => {
  const read = [];
  const dwell = createAgentReadDwell(target => read.push(target));
  dwell.update(agent);
  advance(2_000);
  dwell.update(null);
  advance(8_000);
  assert.equal(read.length, 0);
  dwell.update(agent);
  advance(2_000);
  const other = { processId: 8, projectId: 1 };
  dwell.update(other);
  advance(2_999);
  assert.equal(read.length, 0);
  advance(1);
  assert.deepEqual(read, [other]);
});

test('a pending native focus check is invalidated by blur or a newer unread cycle', () => {
  let stillCurrent;
  const dwell = createAgentReadDwell((_target, current) => { stillCurrent = current; });
  dwell.update(agent);
  advance(3_000);
  assert.equal(stillCurrent(), true);
  const old = stillCurrent;
  dwell.reset();
  dwell.update(agent);
  assert.equal(old(), false);
  advance(3_000);
  assert.equal(stillCurrent(), true);
  dwell.update(null);
  assert.equal(stillCurrent(), false);
});

test('losing WebView focus overrides stale native key-window focus', async () => {
  let checks = 0;
  globalThis.document = { hidden: false, hasFocus: () => false };
  mockIPC(() => { checks++; return true; });
  assert.equal(await isWorkmanWindowFocused(), false);
  assert.equal(checks, 0);
  globalThis.document = { hidden: true, hasFocus: () => true };
  assert.equal(await isWorkmanWindowFocused(), false);
});

test('native application activation must agree with WebView focus and be rechecked after waiting', async () => {
  let active = true;
  let resolve;
  globalThis.document = { hidden: false, hasFocus: () => active };
  mockIPC(command => {
    assert.equal(command, 'native_notification_window_focused');
    return false;
  });
  assert.equal(await isWorkmanWindowFocused(), false);
  mockIPC(() => new Promise(done => { resolve = done; }));
  const pending = isWorkmanWindowFocused();
  active = false;
  resolve(true);
  assert.equal(await pending, false);
  active = true;
  mockIPC(() => true);
  assert.equal(await isWorkmanWindowFocused(), true);
});
