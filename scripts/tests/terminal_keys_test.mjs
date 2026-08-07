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
assert.equal(encodeTerminalKey(key('Delete', { altKey: true }), plain), '\x1bd');
assert.equal(encodeTerminalKey(key('ArrowLeft', { metaKey: true }), plain), '\x01');
assert.equal(encodeTerminalKey(key('ArrowRight', { metaKey: true }), plain), '\x05');
assert.equal(encodeTerminalKey(key('Backspace', { metaKey: true }), plain), '\x15');

assert.equal(encodeTerminalKey(key('ArrowLeft', { altKey: true }), kitty), '\x1b[98;3u');
assert.equal(encodeTerminalKey(key('ArrowRight', { altKey: true }), kitty), '\x1b[102;3u');
assert.equal(encodeTerminalKey(key('Backspace', { altKey: true }), kitty), '\x1b[127;3u');
assert.equal(encodeTerminalKey(key('Delete', { altKey: true }), kitty), '\x1b[100;3u');
assert.equal(encodeTerminalKey(key('ArrowLeft', { metaKey: true }), kitty), '\x1b[97;5u');
assert.equal(encodeTerminalKey(key('ArrowRight', { metaKey: true }), kitty), '\x1b[101;5u');
assert.equal(encodeTerminalKey(key('Backspace', { metaKey: true }), kitty), '\x1b[117;5u');

assert.equal(
  encodeTerminalKey(key('ArrowLeft', { altKey: true }), modifyOtherKeys),
  '\x1b[27;3;98~'
);
assert.equal(
  encodeTerminalKey(key('ArrowRight', { altKey: true }), modifyOtherKeys),
  '\x1b[27;3;102~'
);
assert.equal(
  encodeTerminalKey(key('Backspace', { altKey: true }), modifyOtherKeys),
  '\x1b[27;3;127~'
);
assert.equal(
  encodeTerminalKey(key('Delete', { altKey: true }), modifyOtherKeys),
  '\x1b[27;3;100~'
);
assert.equal(
  encodeTerminalKey(key('ArrowLeft', { metaKey: true }), modifyOtherKeys),
  '\x1b[27;5;97~'
);
assert.equal(
  encodeTerminalKey(key('ArrowRight', { metaKey: true }), modifyOtherKeys),
  '\x1b[27;5;101~'
);
assert.equal(
  encodeTerminalKey(key('Backspace', { metaKey: true }), modifyOtherKeys),
  '\x1b[27;5;117~'
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
  key('Delete', { metaKey: true }),
  key('c', { metaKey: true }),
  key('v', { metaKey: true }),
  key('w', { metaKey: true }),
  key('q', { metaKey: true }),
  key('Backspace')
]) {
  assert.equal(encodeTerminalKey(event, plain), null);
}

console.log('terminal key mappings passed');
