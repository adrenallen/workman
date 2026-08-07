import assert from 'node:assert/strict';

import { encodeTerminalKey } from '../../apps/desktop/src/lib/terminalKeys.ts';

const plain = { kittyFlags: 0, modifyOtherKeys: 0 };
const kitty = { kittyFlags: 1, modifyOtherKeys: 0 };
const modifyOtherKeys = { kittyFlags: 0, modifyOtherKeys: 2 };
const key = (name, modifiers = {}) => ({
  key: name,
  altKey: false,
  ctrlKey: false,
  metaKey: false,
  shiftKey: false,
  ...modifiers
});

assert.equal(encodeTerminalKey(key('ArrowLeft', { altKey: true }), plain), '\x1bb');
assert.equal(encodeTerminalKey(key('ArrowRight', { altKey: true }), plain), '\x1bf');
assert.equal(encodeTerminalKey(key('Backspace', { altKey: true }), plain), '\x1b\x7f');

assert.equal(encodeTerminalKey(key('ArrowLeft', { altKey: true }), kitty), '\x1b[1;3D');
assert.equal(encodeTerminalKey(key('ArrowRight', { altKey: true }), kitty), '\x1b[1;3C');
assert.equal(encodeTerminalKey(key('Backspace', { altKey: true }), kitty), '\x1b[127;3u');

assert.equal(
  encodeTerminalKey(key('ArrowLeft', { altKey: true }), modifyOtherKeys),
  '\x1b[1;3D'
);
assert.equal(
  encodeTerminalKey(key('ArrowRight', { altKey: true }), modifyOtherKeys),
  '\x1b[1;3C'
);
assert.equal(
  encodeTerminalKey(key('Backspace', { altKey: true }), modifyOtherKeys),
  '\x1b[27;3;127~'
);

assert.equal(encodeTerminalKey(key('Enter', { shiftKey: true }), kitty), '\x1b[13;2u');
assert.equal(
  encodeTerminalKey(key('Enter', { altKey: true }), modifyOtherKeys),
  '\x1b[27;3;13~'
);

for (const event of [
  key('e', { altKey: true }),
  key('Dead', { altKey: true }),
  key('é', { altKey: true }),
  key('ArrowLeft', { altKey: true, shiftKey: true }),
  key('ArrowRight', { metaKey: true }),
  key('Backspace')
]) {
  assert.equal(encodeTerminalKey(event, plain), null);
}

console.log('terminal key mappings passed');
