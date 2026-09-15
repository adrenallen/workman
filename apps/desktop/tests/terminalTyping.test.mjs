import assert from 'node:assert/strict';
import test from 'node:test';
import { TerminalCompositionInput, TerminalInputBatch } from '../src/lib/terminalTyping.ts';

function compositionFixture() {
  const tasks = [];
  let activity = 0;
  const input = new TerminalCompositionInput(() => activity++, callback => tasks.push(callback));
  return { input, tasks, activity: () => activity, flush: () => { while (tasks.length) tasks.shift()(); } };
}

test('deferred IME/dead-key input retains provenance through the xterm commit task', () => {
  for (const event of [{ keyCode: 229 }, { key: 'Dead' }, { isComposing: true }]) {
    const { input, tasks, activity, flush } = compositionFixture();
    input.keyDown(event);
    assert.equal(activity(), 1);
    tasks.shift()(); // First task: xterm has not emitted its deferred commit yet.
    assert.equal(input.isUserInput(), true);
    flush();
    assert.equal(input.isUserInput(), false); // Later terminal replies remain non-user input.
  }
});

test('composition updates signal activity even before any text is committed', () => {
  const { input, activity, flush } = compositionFixture();
  input.start();
  flush();
  assert.equal(input.isUserInput(), false, 'protocol replies during an open composition stay non-user');
  input.update();
  flush();
  assert.equal(activity(), 2);
  assert.equal(input.isUserInput(), false, 'a missing compositionend cannot leave provenance stuck');
  input.end();
  assert.equal(input.isUserInput(), true);
  flush();
  assert.equal(input.isUserInput(), false);
});

test('older composition tasks cannot clear newer activity and disposal ignores late callbacks', () => {
  const { input, tasks, activity, flush } = compositionFixture();
  input.textInput();
  const first = tasks.shift();
  input.textInput();
  first();
  tasks.pop()();
  assert.equal(input.isUserInput(), true);
  input.dispose();
  input.textInput();
  flush();
  assert.equal(input.isUserInput(), false);
  assert.equal(activity(), 2);
});

test('ordinary non-composition keys do not leave a deferred user marker', () => {
  const { input, activity } = compositionFixture();
  input.keyDown({ key: 'x', keyCode: 88, isComposing: false });
  assert.equal(activity(), 0);
  assert.equal(input.isUserInput(), false);
});

test('input batching preserves typing, resets provenance, and supports text-free IME activity', () => {
  const batch = new TerminalInputBatch();
  const bytes = value => new TextEncoder().encode(value);
  batch.push(1, bytes('key'), true);
  batch.push(1, bytes('\x1b[0n'), false);
  assert.deepEqual(batch.drain(), { processId: 1, data: bytes('key\x1b[0n'), userInitiated: true });
  assert.equal(batch.drain(), null);
  batch.push(2, bytes('\x1b[I'), false);
  assert.deepEqual(batch.drain(), { processId: 2, data: bytes('\x1b[I'), userInitiated: false });
  batch.push(2, new Uint8Array(), true);
  assert.deepEqual(batch.drain(), { processId: 2, data: new Uint8Array(), userInitiated: true });
  batch.push(2, new Uint8Array(), false);
  assert.equal(batch.drain(), null);
});
