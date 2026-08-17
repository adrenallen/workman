import assert from 'node:assert/strict';
import test from 'node:test';

import { submitCommandCreation } from '../src/lib/commandCreation.ts';

function commandDraft(saveMode) {
  return {
    id: -1,
    projectId: 217,
    kind: 'command',
    createdAt: 1,
    touched: true,
    name: `${saveMode} command`,
    command: 'npm run qa',
    workingDir: 'packages/app',
    environment: 'MODE=qa',
    restartWhenChanged: 'src/**',
    autoStart: false,
    autoRestart: true,
    saveMode
  };
}

function fakeClient() {
  const calls = [];
  return {
    calls,
    async control(method, params) {
      calls.push({ method, params });
      if (method === 'config.validate_working_dir') {
        return { absolute: '/tmp/todo217/packages/app', relative: 'packages/app' };
      }
      if (method === 'config.command_save') {
        return { id: 41, project_id: 217, name: params.name };
      }
      if (method === 'process.create') {
        return { ...params.process, id: 42 };
      }
      throw new Error(`unexpected method ${method}`);
    },
    async startProcess() {
      throw new Error('autoStart=false must not start the process');
    }
  };
}

for (const saveMode of ['yml', 'local']) {
  test(`${saveMode} submit snapshots the draft before synchronous optimistic removal`, async () => {
    const source = commandDraft(saveMode);
    let draftAvailable = true;
    const draft = new Proxy(source, {
      get(target, property, receiver) {
        if (!draftAvailable) throw new Error(`draft getter read after removal: ${String(property)}`);
        return Reflect.get(target, property, receiver);
      }
    });
    const client = fakeClient();

    const result = await submitCommandCreation(client, 217, draft, (input) => {
      assert.equal(input.working_dir, 'packages/app');
      draftAvailable = false;
      return -90;
    });

    assert.equal(result.optimisticId, -90);
    assert.equal(client.calls.filter((call) => call.method === 'config.validate_working_dir').length, 1);
    assert.equal(client.calls.filter((call) => call.method === 'config.command_save').length, saveMode === 'yml' ? 1 : 0);
    assert.equal(client.calls.filter((call) => call.method === 'process.create').length, saveMode === 'local' ? 1 : 0);
    const creation = client.calls.at(-1);
    const workingDir = saveMode === 'yml'
      ? creation.params.working_dir
      : creation.params.process.working_dir;
    assert.equal(workingDir, saveMode === 'yml' ? 'packages/app' : '/tmp/todo217/packages/app');
  });
}
