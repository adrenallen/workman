import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

import { processCycleDirection } from '../src/lib/terminalKeys.ts';

function keyEvent(key, overrides = {}) {
  return {
    key,
    altKey: false,
    ctrlKey: false,
    metaKey: true,
    shiftKey: false,
    ...overrides
  };
}

test('recognizes only bare Command-Up and Command-Down process cycling', () => {
  assert.equal(processCycleDirection(keyEvent('ArrowUp')), -1);
  assert.equal(processCycleDirection(keyEvent('ArrowDown')), 1);

  for (const modifier of ['altKey', 'ctrlKey', 'shiftKey']) {
    assert.equal(processCycleDirection(keyEvent('ArrowUp', { [modifier]: true })), null);
    assert.equal(processCycleDirection(keyEvent('ArrowDown', { [modifier]: true })), null);
  }

  assert.equal(processCycleDirection(keyEvent('ArrowLeft')), null);
  assert.equal(processCycleDirection(keyEvent('PageDown')), null);
  assert.equal(processCycleDirection(keyEvent('ArrowDown', { metaKey: false })), null);
});

test('terminal routes process cycling before user-key tracking and terminal encoding', async () => {
  const terminal = await readFile(
    new URL('../src/lib/TerminalView.svelte', import.meta.url),
    'utf8'
  );
  const handler = terminal.indexOf('instance.attachCustomKeyEventHandler');
  const cycle = terminal.indexOf('processCycleDirection(event)', handler);
  const userKey = terminal.indexOf('let userKeyToken', handler);
  const encode = terminal.indexOf('encodeTerminalKey(event', handler);

  assert.ok(handler >= 0 && cycle > handler && userKey > cycle && encode > userKey);
  assert.match(terminal, /const cycleDirection = onCycleProcess \? processCycleDirection\(event\) : null/);
  assert.match(terminal, /if \(cycleDirection !== null\) \{[\s\S]*event\.preventDefault\(\);[\s\S]*event\.stopPropagation\(\);[\s\S]*event\.type === 'keydown'[\s\S]*onCycleProcess\?\.\(cycleDirection\)[\s\S]*return false/);
});

test('app reuses the cycle predicate and wires terminal cycling back to the main frame', async () => {
  const app = await readFile(new URL('../src/App.svelte', import.meta.url), 'utf8');

  assert.match(app, /const cycleDirection = processCycleDirection\(event\);/);
  assert.match(app, /onCycleProcess=\{\(direction\) => cycleProcess\(direction, 'main'\)\}/);
  assert.match(app, /returnPanel === 'main'[\s\S]*tick\(\)\.then[\s\S]*focusPanel\('main'\)[\s\S]*terminalView\?\.focusInput\(\)/);
});
