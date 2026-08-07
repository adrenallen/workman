import assert from 'node:assert/strict';
import test from 'node:test';

import {
  CONTEXT_ACTION_IDS,
  DESTRUCTIVE_CONTEXT_ACTION_IDS,
  contextActionIcon
} from '../src/lib/contextMenuIcons.ts';

test('every context action has an explicit icon', () => {
  for (const id of CONTEXT_ACTION_IDS) {
    assert.equal(typeof contextActionIcon(id), 'string', `missing icon for ${id}`);
  }
});

test('project creation actions use their matching section icons', () => {
  assert.deepEqual(
    Object.fromEntries([
      'project-settings',
      'new-agent',
      'new-terminal',
      'add-command',
      'new-todo',
      'new-scratchpad'
    ].map((id) => [id, contextActionIcon(id)])),
    {
      'project-settings': 'settings',
      'new-agent': 'bot',
      'new-terminal': 'square-terminal',
      'add-command': 'play',
      'new-todo': 'circle-check',
      'new-scratchpad': 'notebook-text'
    }
  );
});

test('trash is reserved for destructive actions', () => {
  const trashActions = CONTEXT_ACTION_IDS.filter((id) => contextActionIcon(id) === 'trash-2');
  assert.deepEqual(trashActions, [...DESTRUCTIVE_CONTEXT_ACTION_IDS]);
});
