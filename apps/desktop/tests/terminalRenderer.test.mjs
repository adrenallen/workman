import assert from 'node:assert/strict';
import test from 'node:test';

import {
  shouldAttemptWebglRecovery,
  webglRecoveryDelay
} from '../src/lib/terminalRenderer.ts';

test('WebGL recovery waits for the terminal and document to be visible', () => {
  const ready = {
    terminalVisible: true,
    documentVisible: true,
    hasRenderer: false,
    recovering: true
  };
  assert.equal(shouldAttemptWebglRecovery(ready), true);
  assert.equal(shouldAttemptWebglRecovery({ ...ready, terminalVisible: false }), false);
  assert.equal(shouldAttemptWebglRecovery({ ...ready, documentVisible: false }), false);
  assert.equal(shouldAttemptWebglRecovery({ ...ready, hasRenderer: true }), false);
  assert.equal(shouldAttemptWebglRecovery({ ...ready, recovering: false }), false);
});

test('WebGL context restoration retries are prompt and bounded', () => {
  assert.deepEqual([0, 1, 2, 3].map(webglRecoveryDelay), [0, 100, 500, 2_000]);
  assert.equal(webglRecoveryDelay(4), null);
});
