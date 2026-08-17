import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

test('typing bypasses ordinary RPC timeout tracking and has a dedicated native queue', async () => {
  const daemon = await readFile(new URL('../src/lib/daemon.ts', import.meta.url), 'utf8');
  const sendInput = daemon.slice(
    daemon.indexOf('sendInput(processId:'),
    daemon.indexOf('submitInput(processId:')
  );
  const native = await readFile(
    new URL('../src-tauri/src/lib.rs', import.meta.url),
    'utf8'
  );

  assert.match(sendInput, /this\.inputQueue\.push/);
  assert.match(sendInput, /this\.flushInputQueue\(\)/);
  assert.doesNotMatch(sendInput, /this\.request/);
  assert.match(daemon, /invoke\('daemon_send_input'/);
  assert.match(native, /input_sender: mpsc::Sender<TerminalInput>/);
  assert.match(native, /fn daemon_send_input/);
  assert.match(native, /let frame = encode_terminal_input/);
  assert.match(native, /Message::Binary\(frame\.into\(\)\)/);
  assert.match(native, /b"WRI1"/);
});

test('reconnect preserves xterm state, accepts input, and resumes replay at its parsed offset', async () => {
  const terminal = await readFile(
    new URL('../src/lib/TerminalView.svelte', import.meta.url),
    'utf8'
  );
  const app = await readFile(new URL('../src/App.svelte', import.meta.url), 'utf8');
  const disconnected = terminal.slice(
    terminal.indexOf('if (!isConnected) {'),
    terminal.indexOf('const resumingConnection')
  );

  assert.match(disconnected, /inputEnabled = true/);
  assert.doesNotMatch(disconnected, /instance\.reset\(\)/);
  assert.match(terminal, /client\.attachTerminal\(processId, requestedOffset\)/);
  assert.match(terminal, /if \(!resumingConnection\) \{\s*replayPreviewAllowed = true/);
  assert.match(terminal, /if \(!resumingConnection\) \{\s*await client\.resizeTerminal/);
  assert.match(terminal, /Keystrokes are queued/);
  assert.match(app, /if \(!connected\) return;/);
});
