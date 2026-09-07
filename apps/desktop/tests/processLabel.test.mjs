import assert from 'node:assert/strict';
import test from 'node:test';
import { processLabel } from '../src/lib/processLabel.ts';

const terminal = { id: 12, kind: 'terminal', name: 'Terminal', working_dir: '/Users/demo/project' };
test('terminal defaults follow the directory; explicit names override it', () => {
  for (const name of ['Terminal', 'Terminal 2', 'terminal--12', 'terminal--12-2', '']) {
    assert.equal(processLabel({ ...terminal, name }), '~/project');
  }
  for (const name of ['database server', 'Terminal tools', 'terminal--123']) {
    assert.equal(processLabel({ ...terminal, name, working_dir: '/different/path' }), name);
  }
  assert.equal(processLabel({ ...terminal, working_dir: '/srv/api' }), '/srv/api');
  assert.equal(processLabel({ ...terminal, kind: 'agent' }), 'Terminal');
  assert.equal(processLabel({ ...terminal, kind: 'command', name: 'Build' }), 'Build');
});
