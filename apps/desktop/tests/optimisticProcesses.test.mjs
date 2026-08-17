import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

import {
  createOptimisticProcess,
  failOptimisticProcess
} from '../src/lib/optimisticProcesses.ts';

const project = {
  id: 7,
  path: '/tmp/workspace',
  name: 'workspace',
  display_name: null,
  icon: null,
  selected: true,
  sort_order: 0
};

test('failed optimistic agents retain an independent exact retry payload', () => {
  const input = {
    project_id: 7,
    agent_template_id: 12,
    name: 'reviewer',
    extra_args: ['--model', 'fast model'],
    prompt: 'Review the patch.'
  };
  const optimistic = createOptimisticProcess({
    id: -1,
    project,
    kind: 'agent',
    name: 'reviewer',
    agentToolId: 3,
    retry: 'agent',
    agentSpawnInput: input
  });
  input.extra_args.push('--mutated-after-create');

  const failed = failOptimisticProcess(optimistic, new Error('spawn failed'));
  assert.deepEqual(failed.agentSpawnInput, {
    project_id: 7,
    agent_template_id: 12,
    name: 'reviewer',
    extra_args: ['--model', 'fast model'],
    prompt: 'Review the patch.'
  });
  assert.equal(failed.error, 'spawn failed');
});

test('the optimistic retry path resubmits its saved agent payload', async () => {
  const app = await readFile(new URL('../src/App.svelte', import.meta.url), 'utf8');
  assert.match(app, /spawnAgent\(tool, optimistic\.agentSpawnInput\)/);
});

test('failed optimistic commands retain and restore their complete draft', async () => {
  const commandDraft = {
    ...createCommandDraft(),
    name: 'Vite',
    command: 'npm run dev',
    environment: 'PORT=3000',
    saveMode: 'local',
    touched: true
  };
  const optimistic = createOptimisticProcess({
    id: -9,
    project,
    kind: 'command',
    name: commandDraft.name,
    command: commandDraft.command,
    retry: 'command',
    commandDraft
  });
  commandDraft.environment = 'MUTATED=1';
  assert.equal(optimistic.commandDraft.environment, 'PORT=3000');

  const app = await readFile(new URL('../src/App.svelte', import.meta.url), 'utf8');
  assert.match(app, /retry === 'command' && optimistic\.commandDraft/);
  assert.match(app, /creationDrafts = \[\.\.\.creationDrafts, restored\]/);
});

function createCommandDraft() {
  return {
    id: -1,
    projectId: 7,
    kind: 'command',
    createdAt: 1,
    touched: false,
    name: '',
    command: '',
    workingDir: '',
    environment: '',
    restartWhenChanged: '',
    autoStart: true,
    autoRestart: false,
    saveMode: 'yml'
  };
}
