import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

const terminalViewUrl = new URL('../src/lib/TerminalView.svelte', import.meta.url);

test('stopped, exited, and crashed processes replay through the styled xterm surface', async () => {
  const source = await readFile(terminalViewUrl, 'utf8');

  assert.match(
    source,
    /return status === 'running' \|\| status === 'stopped' \|\| status === 'exited' \|\| status === 'crashed'/
  );
  assert.match(source, /!supportsTerminalPlayback\(processStatus\)/);
  assert.match(source, /client\.attachTerminal\(state\.processId, requestedOffset/);
  assert.match(source, /!supportsTerminalPlayback\(process\.status\)/);
  assert.doesNotMatch(source, /client\.renderedProcessOutput\(processId\)/);
});

test('ended processes keep xterm read only and expose a lifecycle-specific bottom action bar', async () => {
  const source = await readFile(terminalViewUrl, 'utf8');

  assert.match(source, /class="terminal-host"[\s\S]*class:is-hidden=\{processStarting\}/);
  assert.match(source, /class="process-ended-bar"/);
  assert.match(source, /processNeverRun \? 'Run command' : 'Run again'/);
  assert.match(source, /process\.agent_session_id \? 'Resume agent' : 'Start agent'/);
  assert.match(source, /return 'Start terminal'/);
  assert.match(source, /terminalInput\.readOnly = processStatus !== 'running'/);
  assert.match(source, /terminal output, read only/);
  assert.match(source, /inputEnabled = process\.status === 'running'/);
  assert.match(source, /shouldForwardTerminalInput\(inputEnabled, userInitiated\) \|\| process\.status !== 'running'/);
});

test('a live terminal buffer is preserved across the transition to read-only output', async () => {
  const source = await readFile(terminalViewUrl, 'utf8');

  assert.match(source, /const transitionedToReadOnly = attachedProcessId === processId/);
  assert.match(source, /const resumingConnection = sameProcessSession \|\| transitionedToReadOnly/);
  assert.match(source, /if \(!resumingConnection\) \{[\s\S]*instance\.reset\(\)/);
});
