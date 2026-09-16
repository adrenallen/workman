import assert from 'node:assert/strict';
import test from 'node:test';
import { terminalFrameContent } from '../src/lib/terminalCheckpoint.ts';

const encode = value => [...new TextEncoder().encode(value)];

test('a screen checkpoint restores geometry without advancing the raw stream', () => {
  const ansi = '\x1bc\x1b[48;2;52;56;64m› Ask Codex\x1b[K';
  const frame = { checkpoint: true, start_offset: 17000000,
    data: encode(JSON.stringify({ rows: 48, columns: 180, ansi })) };
  const checkpoint = terminalFrameContent(frame);
  assert.deepEqual(checkpoint.geometry, { rows: 48, columns: 180 });
  assert.equal(new TextDecoder().decode(checkpoint.data), ansi);
  assert.equal(checkpoint.endOffset, 17000000);
  const live = terminalFrameContent({ start_offset: checkpoint.endOffset, data: encode('⠁') });
  assert.equal(live.endOffset, 17000003);
  assert.equal(live.geometry, undefined);
});

test('raw replay remains byte exact, including incomplete UTF-8 and escape sequences', () => {
  const data = [0x1b, 0x5b, 0x33, 0x31, 0x6d, 0xe2, 0xa0];
  assert.deepEqual(terminalFrameContent({ start_offset: 90, data }), {
    data: Uint8Array.from(data), endOffset: 97
  });
});

test('invalid checkpoint geometry is rejected before resizing the terminal', () => {
  for (const rows of [0, -1, 1.5, 65536, '24']) {
    assert.throws(() => terminalFrameContent({ checkpoint: true, start_offset: 0,
      data: encode(JSON.stringify({ rows, columns: 80, ansi: '' })) }), /Invalid terminal/);
  }
});
