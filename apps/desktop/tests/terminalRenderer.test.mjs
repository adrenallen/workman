import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

import {
  WEBGL_STABLE_RESET_MS,
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
  assert.equal(WEBGL_STABLE_RESET_MS, 30_000);
});

test('context loss shares one recovery budget and unsupported renderers stay latched', async () => {
  const source = await readFile(new URL('../src/lib/TerminalView.svelte', import.meta.url), 'utf8');
  const contextLoss = source.slice(
    source.indexOf('addon.onContextLoss'),
    source.indexOf('instance.loadAddon(addon)')
  );
  assert.doesNotMatch(contextLoss, /webglRecoveryAttempt\s*=\s*0/);
  assert.match(source, /delay === null[\s\S]*webglUnavailable = true/);
  assert.match(source, /if \(webglRecoveryTimer \|\| webglUnavailable\) return/);
  assert.match(source, /armWebglStabilityReset[\s\S]*WEBGL_STABLE_RESET_MS/);
});
