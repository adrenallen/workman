import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

import { hasRetainedTerminalOutput } from '../src/lib/terminalFirstPaint.ts';

test('only a genuinely empty PTY is eligible for the waiting placeholder', () => {
  assert.equal(hasRetainedTerminalOutput({ text: '', raw_end_offset: 0 }), false);
  assert.equal(hasRetainedTerminalOutput({ text: 'retained screen', raw_end_offset: 0 }), true);
  assert.equal(hasRetainedTerminalOutput({ text: '', raw_end_offset: 42 }), true);
});

test('live attach fetches and paints the daemon snapshot before raw replay setup', async () => {
  const source = await readFile(new URL('../src/lib/TerminalView.svelte', import.meta.url), 'utf8');
  const attachStart = source.indexOf('replayState = state;');
  const preview = source.indexOf('void loadLiveOutputPreview(state);', attachStart);
  const fonts = source.indexOf('await document.fonts.ready;', attachStart);
  const attach = source.indexOf('await client.attachTerminal(processId)', attachStart);

  assert.ok(attachStart >= 0 && preview > attachStart);
  assert.ok(preview < fonts && preview < attach, 'the retained screen request starts immediately');
  assert.match(
    source,
    /liveOutputRetained && liveOutputPreview[\s\S]*terminal-retained-preview/
  );
  assert.match(
    source,
    /liveOutputLoaded && !liveOutputRetained[\s\S]*Waiting for first output…/
  );
});

test('the retained preview stays visible until xterm parses replay bytes', async () => {
  const source = await readFile(new URL('../src/lib/TerminalView.svelte', import.meta.url), 'utf8');
  const handler = source.slice(
    source.indexOf('function handleTerminalFrame'),
    source.indexOf('async function loadLiveOutputPreview')
  );
  const write = handler.indexOf('terminal.write(');
  const parsed = handler.indexOf('hasOutput = true;');

  assert.ok(write >= 0 && parsed > write);
  assert.doesNotMatch(handler.slice(0, write), /hasOutput = true/);
});
