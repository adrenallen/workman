import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

import {
  hasRetainedTerminalOutput,
  isUnstyledRetainedSnapshot,
  rawReplayHasGap,
  shouldShowRetainedPreview
} from '../src/lib/terminalFirstPaint.ts';

test('only a genuinely empty PTY is eligible for the waiting placeholder', () => {
  assert.equal(hasRetainedTerminalOutput({ text: '', raw_end_offset: 0 }), false);
  assert.equal(hasRetainedTerminalOutput({ text: 'retained screen', raw_end_offset: 0 }), true);
  assert.equal(hasRetainedTerminalOutput({ text: '', raw_end_offset: 42 }), true);
});

test('the escape-free preview is transitional and gaps are reported', () => {
  assert.equal(shouldShowRetainedPreview({ text: 'retained screen' }, false), true);
  assert.equal(shouldShowRetainedPreview({ text: 'retained screen' }, true), false);
  assert.equal(shouldShowRetainedPreview({ text: 'retained screen' }, true, true), true);
  assert.equal(isUnstyledRetainedSnapshot({ text: 'retained screen', raw_end_offset: 0 }), true);
  assert.equal(isUnstyledRetainedSnapshot({ text: 'retained screen', raw_end_offset: 1 }), false);
  assert.equal(rawReplayHasGap(0, 128), true);
  assert.equal(rawReplayHasGap(128, 128), false);
});

test('live attach fetches and paints the daemon snapshot before raw replay setup', async () => {
  const source = await readFile(new URL('../src/lib/TerminalView.svelte', import.meta.url), 'utf8');
  const attachStart = source.indexOf('replayState = state;');
  const preview = source.indexOf('void loadLiveOutputPreview(state);', attachStart);
  const fonts = source.indexOf('await document.fonts.ready;', attachStart);
  const attach = source.indexOf('await client.attachTerminal(processId,', attachStart);

  assert.ok(attachStart >= 0 && preview > attachStart);
  assert.ok(preview < fonts && preview < attach, 'the retained screen request starts immediately');
  assert.match(
    source,
    /liveOutputRetained && shouldShowRetainedPreview[\s\S]*terminal-retained-preview/
  );
  assert.match(
    source,
    /liveOutputLoaded && !liveOutputRetained[\s\S]*Waiting for first output…/
  );
});

test('the retained preview stays visible until xterm completes raw replay', async () => {
  const source = await readFile(new URL('../src/lib/TerminalView.svelte', import.meta.url), 'utf8');
  const handler = source.slice(
    source.indexOf('function handleTerminalFrame'),
    source.indexOf('async function loadLiveOutputPreview')
  );
  const write = handler.indexOf('terminal.write(');
  const parsed = handler.indexOf('hasOutput = true;');

  assert.ok(write >= 0 && parsed > write);
  assert.doesNotMatch(handler.slice(0, write), /hasOutput = true/);
  assert.match(
    source,
    /shouldShowRetainedPreview\(\{ text: liveOutputPreview \}, replayComplete, retainedSnapshotOnly\)/
  );
  assert.match(source, /function finishReplayIfReady[\s\S]*replayComplete = true/);
  assert.match(source, /function armReplayWatchdog[\s\S]*Styled terminal replay stalled/);
  assert.match(source, /state\.parsedThrough = Math\.max[\s\S]*armReplayWatchdog\(state\)/);
  assert.match(source, /clearReplayWarning\(\);[\s\S]*replayUnavailableMessage = null;[\s\S]*replayComplete = true/);
  assert.match(source, /Styled terminal replay is unavailable/);
  assert.match(source, /Unstyled retained snapshot · live output will replace it/);
});

test('retained replay focuses immediately and preserves physical input', async () => {
  const source = await readFile(new URL('../src/lib/TerminalView.svelte', import.meta.url), 'utf8');
  const replayStart = source.indexOf('replayState = state;');
  const earlyFocus = source.indexOf('instance.focus();', replayStart);
  const attach = source.indexOf('await client.attachTerminal(processId,', replayStart);
  const queueInput = source.slice(
    source.indexOf('function queueInput'),
    source.indexOf('function flushInput')
  );

  assert.ok(replayStart >= 0 && earlyFocus > replayStart && earlyFocus < attach);
  assert.match(source, /consumePendingUserKeyToken\(\)/);
  assert.match(queueInput, /shouldForwardTerminalInput\(inputEnabled, userInitiated\)/);
});
