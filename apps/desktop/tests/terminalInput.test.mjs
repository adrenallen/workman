import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

import {
  AGENT_TUI_CLIPBOARD_IMAGE_PASTE,
  clipboardImagePasteRoute,
  deferAgentImagePaste,
  localPathsFromUriList,
  pointIsInsideRect,
  shellEscapePath,
  shellEscapePaths,
  shouldForwardTerminalInput
} from '../src/lib/terminalInput.ts';

test('routes image paste by process kind and agent tool without changing text paste behavior', () => {
  assert.equal(clipboardImagePasteRoute('agent', 'claude_code'), 'agent-native-deferred');
  assert.equal(clipboardImagePasteRoute('agent', 'claude'), 'agent-native-deferred');
  assert.equal(clipboardImagePasteRoute('agent', 'codex'), 'agent-tui');
  assert.equal(clipboardImagePasteRoute('agent', 'future_agent'), 'agent-tui');
  assert.equal(clipboardImagePasteRoute('terminal'), 'shell-path');
  assert.equal(clipboardImagePasteRoute('command'), 'shell-path');
  assert.deepEqual(Array.from(new TextEncoder().encode(AGENT_TUI_CLIPBOARD_IMAGE_PASTE)), [0x16]);
});

test('defers synthetic agent image paste until after the clipboard event task', () => {
  const calls = [];
  let scheduled = null;
  deferAgentImagePaste(
    () => calls.push('forward'),
    (callback) => {
      calls.push('schedule');
      scheduled = callback;
    }
  );
  assert.deepEqual(calls, ['schedule']);
  scheduled();
  assert.deepEqual(calls, ['schedule', 'forward']);
});

test('agent image paste forwards the TUI shortcut while plain shells keep saved paths', async () => {
  const transfers = await readFile(
    new URL('../src/lib/terminalTransfers.ts', import.meta.url),
    'utf8'
  );
  const terminal = await readFile(new URL('../src/lib/TerminalView.svelte', import.meta.url), 'utf8');
  const noImages = transfers.indexOf('if (images.length === 0) return;');
  const preventDefault = transfers.indexOf('event.preventDefault();', noImages);
  const sharedPaste = transfers.indexOf('pasteImages(images)', preventDefault);
  const agentRoute = transfers.indexOf("if (route !== 'shell-path')");
  const shellSave = transfers.indexOf('saveClipboardImages(images)', agentRoute);

  assert.ok(noImages >= 0 && preventDefault > noImages, 'text-only paste must reach xterm unchanged');
  assert.ok(sharedPaste > preventDefault && agentRoute >= 0 && shellSave > agentRoute);
  assert.match(transfers, /deferAgentImagePaste\(forward\)/);
  assert.doesNotMatch(
    transfers.slice(transfers.indexOf('export function installTerminalTransfers')),
    /navigator\.clipboard\.(write|writeText)/
  );
  assert.match(terminal, /queueInput\(encoder\.encode\(AGENT_TUI_CLIPBOARD_IMAGE_PASTE\), true\)/);
});

test('context-menu paste reuses image routing and xterm bracketed text paste', async () => {
  const transfers = await readFile(
    new URL('../src/lib/terminalTransfers.ts', import.meta.url),
    'utf8'
  );
  const terminal = await readFile(new URL('../src/lib/TerminalView.svelte', import.meta.url), 'utf8');

  const nativeRead = transfers.indexOf("'terminal_read_clipboard'");
  const nativeWrite = transfers.indexOf("'terminal_write_clipboard_text'");
  const nativeAgentChord = transfers.indexOf('options.forwardAgentImagePaste();', nativeRead);
  const nativeShellPath = transfers.indexOf('insertPaths([clipboard.path])', nativeAgentChord);
  const clipboardRead = transfers.indexOf('navigator.clipboard.read()');
  const sharedPaste = transfers.indexOf('await pasteImages(images)', clipboardRead);
  const textPaste = transfers.indexOf('options.pasteText(text)', sharedPaste);

  assert.ok(nativeWrite >= 0 && nativeRead > nativeWrite);
  assert.ok(nativeRead >= 0 && nativeAgentChord > nativeRead && nativeShellPath > nativeAgentChord);
  assert.ok(clipboardRead > nativeShellPath && sharedPaste > clipboardRead && textPaste > sharedPaste);
  assert.doesNotMatch(
    transfers.slice(transfers.indexOf('export function installTerminalTransfers')),
    /navigator\.clipboard\.(write|writeText)/
  );
  assert.match(terminal, /pasteText: \(text\) => \{[\s\S]*pendingUserKeyTokens\.push\(\+\+nextUserKeyToken\);[\s\S]*instance\.paste\(text\);/);
  assert.match(terminal, /writeTerminalClipboardText\(selection\)/);
  assert.match(terminal, /writeTerminalClipboardText\(detail\.link\)/);
  assert.match(terminal, /oncontextmenu=\{showTerminalContextMenu\}/);
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
