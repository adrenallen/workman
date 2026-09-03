import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

const sourceRoot = new URL('../src/', import.meta.url);

async function source(relativePath) {
  return readFile(new URL(relativePath, sourceRoot), 'utf8');
}

test('middle click is claimed only for removable and archivable project tree rows', async () => {
  const tree = await source('lib/ProjectTree.svelte');

  assert.match(tree, /if \(event\.button !== 1\) return;/);
  assert.match(tree, /event\.preventDefault\(\);[\s\S]*event\.stopPropagation\(\);[\s\S]*onMiddleClick\(target\);/);
  assert.match(tree, /handleMiddleClick\(event, todoTarget\(todo\)\)/);
  assert.match(tree, /handleMiddleClick\(event, processTarget\(process\)\)/);
  assert.equal(tree.match(/handleMiddleClick\(event, processTarget\(process\)\)/g)?.length, 2);
  assert.match(tree, /handleMiddleClick\(event, scratchpadTarget\(scratchpad\)\)/);
  assert.match(tree, /handleMiddleClick\(event, feedbackTarget\(item\)\)/);

  const commandBlock = tree.match(/\{:else if group === 'commands'\}([\s\S]*?)\{:else\}/)?.[1] ?? '';
  assert.doesNotMatch(commandBlock, /handleMiddleClick/);
});

test('middle click force-removes processes and uses recoverable coordination actions', async () => {
  const app = await source('App.svelte');
  const handler = app.match(/async function runTreeMiddleClick[\s\S]*?\n  async function commitTreeRename/)?.[0] ?? '';

  assert.match(handler, /process\.kind === 'command'/);
  assert.match(handler, /planAgentCascade\(processes, \[process\], true\)/);
  assert.match(handler, /client\.control\('process\.kill'/);
  assert.match(handler, /await client\.closeProcess\(process\.id\)/);
  assert.match(handler, /runTodoContextAction\('complete-todo', target\)/);
  assert.match(handler, /runScratchpadContextAction\('archive-scratchpad', target\)/);
  assert.match(handler, /setFeedbackArchived\(target, true\)/);
  assert.doesNotMatch(handler, /confirmInApp/);
});
