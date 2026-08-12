import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

import {
  AGENT_TUI_CLIPBOARD_IMAGE_PASTE,
  clipboardImagePasteRoute,
  localPathsFromUriList,
  pointIsInsideRect,
  shellEscapePath,
  shellEscapePaths,
  shouldForwardTerminalInput
} from '../src/lib/terminalInput.ts';

test('routes image paste by process kind without changing text paste behavior', () => {
  assert.equal(clipboardImagePasteRoute('agent'), 'agent-tui');
  assert.equal(clipboardImagePasteRoute('terminal'), 'shell-path');
  assert.equal(clipboardImagePasteRoute('command'), 'shell-path');
  assert.deepEqual(Array.from(new TextEncoder().encode(AGENT_TUI_CLIPBOARD_IMAGE_PASTE)), [0x16]);
});

test('agent image paste forwards the TUI shortcut while plain shells keep saved paths', async () => {
  const transfers = await readFile(
    new URL('../src/lib/terminalTransfers.ts', import.meta.url),
    'utf8'
  );
  const terminal = await readFile(new URL('../src/lib/TerminalView.svelte', import.meta.url), 'utf8');
  const noImages = transfers.indexOf('if (images.length === 0) return;');
  const preventDefault = transfers.indexOf('event.preventDefault();', noImages);
  const agentRoute = transfers.indexOf("options.imagePasteRoute() === 'agent-tui'", preventDefault);
  const shellSave = transfers.indexOf('saveClipboardImages(images)', agentRoute);

  assert.ok(noImages >= 0 && preventDefault > noImages, 'text-only paste must reach xterm unchanged');
  assert.ok(agentRoute > preventDefault && shellSave > agentRoute);
  assert.match(transfers, /options\.forwardAgentImagePaste\(\);/);
  assert.doesNotMatch(transfers, /navigator\.clipboard\.(write|writeText)/);
  assert.match(terminal, /queueInput\(encoder\.encode\(AGENT_TUI_CLIPBOARD_IMAGE_PASTE\), true\)/);
});

test('accepts physical input during replay without forwarding replay-generated replies', () => {
  assert.equal(shouldForwardTerminalInput(false, true), true);
  assert.equal(shouldForwardTerminalInput(false, false), false);
  assert.equal(shouldForwardTerminalInput(true, false), true);
});

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
