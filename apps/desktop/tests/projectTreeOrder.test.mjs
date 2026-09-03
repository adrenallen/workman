import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

import {
  defaultProjectTreeGroupOrder,
  normalizeProjectTreeGroupOrder,
  projectTreeGroupOrderStorageKey
} from '../src/lib/projectTree.ts';

test('project tree groups default to feedback at the bottom', () => {
  assert.equal(defaultProjectTreeGroupOrder.at(-1), 'feedback');
  assert.equal(projectTreeGroupOrderStorageKey, 'workman.tree.group-order.v1');
});

test('saved project tree group order is complete, unique, and forward compatible', () => {
  assert.deepEqual(normalizeProjectTreeGroupOrder([
    'feedback', 'todos', 'feedback', 'unknown', 'agents'
  ]), [
    'feedback', 'todos', 'agents', 'terminals', 'commands', 'scratchpads'
  ]);
  assert.deepEqual(normalizeProjectTreeGroupOrder(null), [...defaultProjectTreeGroupOrder]);
});

test('project tree wires every group header to the persisted handle-only reorder action', async () => {
  const tree = await readFile(new URL('../src/lib/ProjectTree.svelte', import.meta.url), 'utf8');
  assert.match(tree, /use:reorderItem=\{groupReorderOptions\(group\)\}/);
  assert.match(tree, /handle: '\.group-drag-handle'/);
  assert.match(tree, /localStorage\.setItem\(projectTreeGroupOrderStorageKey/);
});
