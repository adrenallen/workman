import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

const root = new URL('../', import.meta.url);

async function source(path) {
  return readFile(new URL(path, root), 'utf8');
}

test('runtime doctor renders capture mode, resolved PATH, and rc-file diagnostics', async () => {
  const doctor = await source('src/lib/settings/RuntimeDoctor.svelte');
  const agentTypes = await source('src/lib/agentTools.ts');
  const settingsTypes = await source('src/lib/settings.ts');

  assert.match(doctor, /captureModeLabel\(health\.environment_capture_mode\)/);
  assert.match(doctor, /health\.resolved_path \|\| 'PATH unavailable'/);
  assert.match(doctor, /health\.environment_capture_error/);
  assert.match(doctor, /tool\.path_diagnostic/);
  assert.match(doctor, /Refresh health/);
  assert.match(agentTypes, /path_diagnostic: string \| null;/);
  assert.match(agentTypes, /environment_capture_mode:/);
  assert.match(agentTypes, /environment_capture_error: string \| null;/);
  assert.match(settingsTypes, /capture_mode:/);
  assert.match(settingsTypes, /capture_error: string \| null;/);
});
