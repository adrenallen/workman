import assert from 'node:assert/strict';
import { readFile, readdir } from 'node:fs/promises';
import test from 'node:test';

const sourceRoot = new URL('../src/', import.meta.url);

async function source(relativePath) {
  return readFile(new URL(relativePath, sourceRoot), 'utf8');
}

const floatingContentComponents = [
  'lib/components/ui/context-menu/context-menu-content.svelte',
  'lib/components/ui/context-menu/context-menu-sub-content.svelte',
  'lib/components/ui/dropdown-menu/dropdown-menu-content.svelte',
  'lib/components/ui/dropdown-menu/dropdown-menu-sub-content.svelte',
  'lib/components/ui/popover/popover-content.svelte'
];

async function svelteSources(directory = sourceRoot) {
  const entries = await readdir(directory, { withFileTypes: true });
  const files = await Promise.all(entries.map(async (entry) => {
    const url = new URL(entry.name + (entry.isDirectory() ? '/' : ''), directory);
    if (entry.isDirectory()) return svelteSources(url);
    return entry.name.endsWith('.svelte') ? [url] : [];
  }));
  return files.flat();
}

test('floating menu and popover content is viewport-capped and internally scrollable', async () => {
  for (const component of floatingContentComponents) {
    const contents = await source(component);
    assert.match(contents, /max-h-\[calc\(100vh-16px\)\]/, `${component} caps height with an 8px viewport margin`);
    assert.match(contents, /max-w-\[calc\(100vw-16px\)\]/, `${component} caps width with an 8px viewport margin`);
    assert.match(contents, /overflow-y-auto/, `${component} scrolls internally`);
    assert.match(contents, /overscroll-contain/, `${component} contains scroll chaining`);
    assert.match(contents, /scroll-py-1/, `${component} keeps keyboard-focused items clear of its edges`);
    assert.match(contents, /\[scrollbar-width:thin\]/, `${component} uses a dense scrollbar`);
    assert.match(contents, /\[scrollbar-color:var\(--border-strong\)_transparent\]/, `${component} uses theme tokens for its scrollbar`);
  }
});

test('shared floating content explicitly enables collision handling', async () => {
  for (const component of floatingContentComponents) {
    const contents = await source(component);
    assert.match(contents, /avoidCollisions = true/, `${component} defaults collision avoidance on`);
    assert.match(contents, /collisionPadding = 8/, `${component} reserves the viewport margin`);
    assert.match(contents, /\{avoidCollisions\}/, `${component} forwards collision avoidance`);
    assert.match(contents, /\{collisionPadding\}/, `${component} forwards collision padding`);
  }
});

test('project context menu pins collision handling and no call site disables it', async () => {
  const contextMenu = await source('lib/ContextMenu.svelte');
  assert.match(contextMenu, /avoidCollisions=\{true\}/);
  assert.match(contextMenu, /collisionPadding=\{8\}/);

  const allSources = await Promise.all((await svelteSources()).map((url) => readFile(url, 'utf8')));
  const callSites = allSources.join('\n');
  assert.doesNotMatch(callSites, /avoidCollisions=\{false\}/);
  assert.doesNotMatch(callSites, /collisionPadding=\{0\}/);
});
