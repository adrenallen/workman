import assert from 'node:assert/strict';
import test from 'node:test';
import { insertDictation } from '../src/lib/voiceInput.ts';

test('dictation replaces the saved selection while preserving surrounding instructions', () => {
  assert.deepEqual(insertDictation('Please old task now', 'Please old task now', { start: 7, end: 15 }, 'review the PR'), { text: 'Please review the PR now', caret: 20 });
  assert.deepEqual(insertDictation('', '', { start: 0, end: 0 }, ' New task. '), { text: 'New task.', caret: 9 });
});

test('dictation appends safely if instructions change while speech is being transcribed', () => {
  assert.equal(insertDictation('An edited prompt', 'Original', { start: 0, end: 8 }, 'More instructions.').text, 'An edited prompt More instructions.');
  assert.equal(insertDictation('First line\n', 'First line\n', { start: 11, end: 11 }, 'Second line').text, 'First line\nSecond line');
});

test('silence leaves existing instructions intact', () => {
  assert.equal(insertDictation('Keep this', 'Keep this', { start: 0, end: 9 }, '  ').text, 'Keep this');
});
