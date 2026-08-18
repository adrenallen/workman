import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

const dialogUrl = new URL('../src/lib/RegisterProjectDialog.svelte', import.meta.url);
const appUrl = new URL('../src/App.svelte', import.meta.url);
const daemonUrl = new URL('../src/lib/daemon.ts', import.meta.url);

test('folder selection opens a focused title step before the registration RPC', async () => {
  const dialog = await readFile(dialogUrl, 'utf8');
  const app = await readFile(appUrl, 'utf8');

  assert.match(app, /defaultTitle: registrationTitleForPath\(path, projects\)/);
  assert.match(app, /registerProjectDialog = \{/);
  assert.match(app, /client\.register\([\s\S]*?state\.path,[\s\S]*?resolvedProjectTitle/);
  assert.match(dialog, /bind:ref=\{titleInput\}/);
  assert.match(dialog, /titleInput\?\.focus\(\)/);
  assert.match(dialog, /titleInput\?\.select\(\)/);
  assert.match(dialog, /onEscapeKeydown=\{keepDefault\}/);
  assert.match(dialog, /submit\(defaultTitle\)/);
  assert.match(dialog, /Esc registers as/);
  assert.match(dialog, /event\.metaKey && !event\.ctrlKey/);
  assert.match(dialog, /onclick=\{onBack\}/);
  assert.match(app, /onBack=\{\(\) => void changeRegisterProjectFolder\(\)\}/);
});

test('registration sends an optional display name in the original round trip', async () => {
  const daemon = await readFile(daemonUrl, 'utf8');

  assert.match(daemon, /register\(path: string, displayName\?: string\)/);
  assert.match(daemon, /projects\.register', \{ path, display_name: displayName \}/);
});
