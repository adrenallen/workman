import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
import ts from 'typescript';
import { emit } from '@tauri-apps/api/event';
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks';
import { appearance, currentAppearance } from '../src/lib/appearance.ts';

import {
  clearRecoveredDaemonTimeout,
  DaemonRequestTimeoutError
} from '../src/lib/daemonLog.ts';

// Compile the real client so its TypeScript parameter properties work in Node.
const sourcePath = new URL('../src/lib/daemon.ts', import.meta.url);
const source = await readFile(sourcePath, 'utf8');
const output = ts.transpileModule(source, {
  compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2022 }
}).outputText.replace(/from (['"])([^'"]+)\1/g, (_match, _quote, specifier) => {
  const url = specifier.startsWith('.')
    ? new URL(`${specifier}.ts`, sourcePath).href
    : import.meta.resolve(specifier);
  return `from ${JSON.stringify(url)}`;
});
const { DaemonClient } = await import(
  `data:text/javascript;base64,${Buffer.from(output).toString('base64')}`
);

async function startClient(t) {
  const previousWindow = globalThis.window;
  globalThis.window = { crypto: globalThis.crypto };
  const sent = [];
  mockIPC((command, payload) => {
    if (command === 'daemon_status') return { status: 'connected' };
    if (command === 'daemon_send') {
      sent.push(JSON.parse(payload.message));
      return;
    }
    assert.fail(`Unexpected native call: ${command}`);
  }, { shouldMockEvents: true });
  const client = new DaemonClient();
  t.after(() => {
    client.close();
    clearMocks();
    if (previousWindow === undefined) delete globalThis.window;
    else globalThis.window = previousWindow;
  });
  await client.start(() => {}, () => {});
  return { client, sent };
}

function reply(response) {
  return emit('daemon://message', { kind: 'text', data: JSON.stringify(response) });
}

test('fresh, resumed, restarted and shell launches publish their colors before starting the PTY', async (t) => {
  const { client, sent } = await startClient(t);
  const original = currentAppearance();
  t.after(() => appearance.set(original));
  appearance.set({ ...original, terminalTheme: {
    ...original.terminalTheme,
    palette: { ...original.terminalTheme.palette, background: '#f1efe8' }
  } });
  for (const [launch, method] of [
    [() => client.spawnAgent({ project_id: 6, agent_tool_id: 2, extra_args: [] }), 'agents.spawn'],
    [() => client.startProcess(1), 'process.start'],
    [() => client.restartProcess(1), 'process.restart'],
    [() => client.spawnTerminal(6), 'process.spawn_terminal']
  ]) {
    sent.length = 0;
    const pending = launch();
    assert.equal(sent.length, 1, 'launch must wait for the palette acknowledgment');
    assert.equal(sent[0].method, 'settings.terminal_colors');
    assert.deepEqual(sent[0].params.background, [241, 239, 232]);
    await reply({ id: sent[0].id, ok: true, result: sent[0].params });
    await new Promise(setImmediate);
    assert.equal(sent[1].method, method);
    await reply({ id: sent[1].id, ok: true, result: {} });
    await pending;
  }
});

test('a late daemon reply clears a flattened timeout without retrying the request or clearing other errors', async (t) => {
  const { client, sent } = await startClient(t);
  t.mock.timers.enable({ apis: ['setTimeout'] });
  let banner = null;
  const stop = client.onResponsive(() => {
    banner = clearRecoveredDaemonTimeout(banner);
  });
  const request = client.control('process.send_input', { process_id: 1 }).catch((cause) => {
    assert.ok(cause instanceof DaemonRequestTimeoutError);
    // Terminal callbacks forward only the message to the app's error banner.
    banner = cause.message;
  });
  t.mock.timers.tick(5_000);
  await request;
  assert.equal(banner, 'The daemon did not answer in time');

  await reply({ id: sent[0].id });
  assert.notEqual(banner, null, 'an invalid response does not prove recovery');
  await reply({ id: sent[0].id, ok: true, result: {} });
  assert.equal(banner, null, 'the response still clears the banner after the request expired');
  assert.equal(sent.length, 1, 'recovery must not resend an operation');

  banner = 'Could not save the project settings';
  await reply({ id: sent[0].id, ok: true, result: {} });
  assert.equal(banner, 'Could not save the project settings');

  stop();
  banner = 'The daemon did not answer in time';
  await reply({ id: sent[0].id, ok: true, result: {} });
  assert.notEqual(banner, null, 'unsubscribed views are not updated');
});

test('reconnection and fresh status snapshots clear timeout banners while disconnection does not', async (t) => {
  const { client } = await startClient(t);
  let banner = 'The daemon did not respond in time';
  client.onResponsive(() => { banner = clearRecoveredDaemonTimeout(banner); });

  await emit('daemon://status', { status: 'disconnected' });
  assert.notEqual(banner, null);
  await emit('daemon://status', { status: 'connected' });
  assert.equal(banner, null);

  banner = 'Error: The daemon did not answer in time';
  await reply({ event: 'process.statuses', processes: [] });
  assert.equal(banner, null);
});
