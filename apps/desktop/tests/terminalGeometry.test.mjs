import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

const terminalViewUrl = new URL('../src/lib/TerminalView.svelte', import.meta.url);

test('terminal reattachment is keyed by the live PTY generation', async () => {
  const source = await readFile(terminalViewUrl, 'utf8');

  assert.match(source, /let attachedProcessPid: number \| null = null;/);
  assert.match(source, /const processPid = process\.pid;/);
  assert.match(source, /attachedProcessPid === processPid/);
  assert.match(source, /attachedProcessPid = processPid;/);
});

test('current geometry is published before attach and while stopped', async () => {
  const source = await readFile(terminalViewUrl, 'utf8');
  const attachStart = source.indexOf('void (async () => {');
  const resizeBeforeAttach = source.indexOf('await client.resizeTerminal(', attachStart);
  const attach = source.indexOf('await client.attachTerminal(processId,', attachStart);
  assert.ok(attachStart >= 0 && resizeBeforeAttach > attachStart && attach > resizeBeforeAttach);

  const scheduleStart = source.indexOf('function scheduleFit(): void');
  const scheduleEnd = source.indexOf('function fitTerminal()', scheduleStart);
  const schedule = source.slice(scheduleStart, scheduleEnd);
  assert.match(schedule, /if \(!instance \|\| !fitTerminal\(\)\) return;/);
  assert.match(schedule, /if \(!connected\) return;/);
  assert.doesNotMatch(schedule, /process\.status !== 'running'/);
  assert.match(schedule, /\.resizeTerminal\(/);

  const hiddenHost = source.match(/\.terminal-host\.is-hidden \{(?<rules>[\s\S]*?)\}/)?.groups?.rules;
  assert.ok(hiddenHost, 'expected stopped terminal host styles');
  assert.match(hiddenHost, /opacity:\s*0;/);
  assert.doesNotMatch(hiddenHost, /visibility:\s*hidden;/);
});
