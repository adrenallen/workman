import assert from 'node:assert/strict';
import test from 'node:test';

import {
  localPathsFromUriList,
  pointIsInsideRect,
  shellEscapePath,
  shellEscapePaths
} from '../src/lib/terminalInput.ts';

test('accepts only local file URLs from a URI list', () => {
  assert.deepEqual(
    localPathsFromUriList([
      '# Finder drag payload',
      'file:///tmp/drop%20image%20one.png',
      'file://localhost/tmp/quote%27s%20%E9%9B%AA.png',
      'file://remote.example/tmp/refused.png',
      'https://example.com/refused.png',
      'not a URL'
    ].join('\r\n')),
    ['/tmp/drop image one.png', "/tmp/quote's 雪.png"]
  );
});

test('shell-escapes spaces, quotes, metacharacters, backslashes, and unicode', () => {
  assert.equal(shellEscapePath('/tmp/plain.png'), '/tmp/plain.png');
  assert.equal(shellEscapePath('/tmp/image one.png'), '/tmp/image\\ one.png');
  assert.equal(shellEscapePath("/tmp/one's \"quote\".png"), "/tmp/one\\'s\\ \\\"quote\\\".png");
  assert.equal(shellEscapePath('/tmp/cash$bang!\\file'), '/tmp/cash\\$bang\\!\\\\file');
  assert.equal(shellEscapePath('/tmp/café-雪.png'), '/tmp/café-雪.png');
});

test('joins multiple paths with one space and no Enter or trailing bytes', () => {
  assert.equal(
    shellEscapePaths(['/tmp/one file.png', '/tmp/two.png']),
    '/tmp/one\\ file.png /tmp/two.png'
  );
  assert.equal(shellEscapePaths(['/tmp/two.png']).endsWith('\n'), false);
});

test('rejects path values that cannot be typed losslessly', () => {
  assert.throws(() => shellEscapePath(''));
  assert.throws(() => shellEscapePath('/tmp/line\nbreak'));
  assert.throws(() => shellEscapePath('/tmp/nul\0byte'));
  assert.throws(() => shellEscapePaths([]));
});

test('converts Tauri physical drop coordinates to the CSS viewport', () => {
  const rect = { left: 100, top: 50, right: 500, bottom: 350 };
  assert.equal(pointIsInsideRect({ x: 400, y: 300 }, rect, 2), true);
  assert.equal(pointIsInsideRect({ x: 50, y: 300 }, rect, 2), false);
  assert.equal(pointIsInsideRect({ x: 400, y: 300 }, rect, 0), false);
});
