import assert from 'node:assert/strict';
import { readFile, readdir } from 'node:fs/promises';
import test from 'node:test';

const sourceRoot = new URL('../src/', import.meta.url);
const uiRoot = new URL('lib/components/ui/', sourceRoot);

async function source(relativePath) {
  return readFile(new URL(relativePath, sourceRoot), 'utf8');
}

const nonMenuContentComponents = new Set([
  'lib/components/ui/alert-dialog/alert-dialog-content.svelte',
  'lib/components/ui/collapsible/collapsible-content.svelte',
  'lib/components/ui/dialog/dialog-content.svelte',
  'lib/components/ui/tabs/tabs-content.svelte',
  'lib/components/ui/tooltip/tooltip-content.svelte'
]);

async function svelteSources(directory = sourceRoot) {
  const entries = await readdir(directory, { withFileTypes: true });
  const files = await Promise.all(entries.map(async (entry) => {
    const url = new URL(entry.name + (entry.isDirectory() ? '/' : ''), directory);
    if (entry.isDirectory()) return svelteSources(url);
    return entry.name.endsWith('.svelte') ? [url] : [];
  }));
  return files.flat();
}

async function floatingContentComponents() {
  const contentComponents = (await svelteSources(uiRoot))
    .filter((url) => url.pathname.endsWith('-content.svelte'))
    .map((url) => url.href.slice(sourceRoot.href.length))
    .sort();

  for (const component of nonMenuContentComponents) {
    assert.ok(contentComponents.includes(component), `explicit non-menu content wrapper still exists: ${component}`);
  }

  return contentComponents.filter((component) => !nonMenuContentComponents.has(component));
}

function floatingCallSiteTags(contents) {
  return contents.match(/<(?:ContextMenu|DropdownMenu|Popover|Select)\.(?:Content|SubContent)\b[^>]*>/gs) ?? [];
}

function classAttribute(tag) {
  const match = tag.match(/\bclass\s*=\s*(?:"([^"]*)"|'([^']*)'|\{([\s\S]*?)\})/);
  return match?.slice(1).find((value) => value !== undefined) ?? '';
}

test('floating menu and popover content is viewport-capped and internally scrollable', async () => {
  for (const component of await floatingContentComponents()) {
    const contents = await source(component);
    const heightCap = component.endsWith('/select/select-content.svelte')
      ? /!max-h-\(--bits-select-content-available-height\)/
      : /!max-h-\[calc\(100vh-16px\)\]/;
    assert.match(contents, heightCap, `${component} has an enforced available-height cap`);
    assert.match(contents, /!max-w-\[calc\(100vw-16px\)\]/, `${component} caps width with an enforced 8px viewport margin`);
    assert.match(contents, /!overflow-y-auto/, `${component} enforces internal scrolling`);
    assert.match(contents, /overscroll-contain/, `${component} contains scroll chaining`);
    assert.match(contents, /scroll-py-1/, `${component} keeps keyboard-focused items clear of its edges`);
    assert.match(contents, /\[scrollbar-width:thin\]/, `${component} uses a dense scrollbar`);
    assert.match(contents, /\[scrollbar-color:var\(--border-strong\)_transparent\]/, `${component} uses theme tokens for its scrollbar`);
  }
});

test('shared floating content explicitly enables collision handling', async () => {
  for (const component of await floatingContentComponents()) {
    const contents = await source(component);
    assert.match(contents, /avoidCollisions = true/, `${component} defaults collision avoidance on`);
    assert.match(contents, /collisionPadding = 8/, `${component} reserves the viewport margin`);
    assert.match(contents, /\{avoidCollisions\}/, `${component} forwards collision avoidance`);
    assert.match(contents, /\{collisionPadding\}/, `${component} forwards collision padding`);
  }
});

test('select keeps keyboard targets clear of its scroll controls', async () => {
  const selectContent = await source('lib/components/ui/select/select-content.svelte');
  const scrollUp = await source('lib/components/ui/select/select-scroll-up-button.svelte');
  const scrollDown = await source('lib/components/ui/select/select-scroll-down-button.svelte');

  assert.match(selectContent, /scroll-py-6/, 'the Select viewport reserves the scroll-control height');
  assert.match(scrollUp, /absolute top-0/, 'the up control does not shrink the Select viewport');
  assert.match(scrollDown, /absolute bottom-0/, 'the down control does not shrink the Select viewport');
});

test('floating content call sites preserve collision and reachability guarantees', async () => {
  const contextMenu = await source('lib/ContextMenu.svelte');
  assert.match(contextMenu, /avoidCollisions=\{true\}/);
  assert.match(contextMenu, /collisionPadding=\{8\}/);

  for (const url of await svelteSources()) {
    const contents = await readFile(url, 'utf8');
    const relativePath = url.href.slice(sourceRoot.href.length);
    for (const tag of floatingCallSiteTags(contents)) {
      assert.doesNotMatch(tag, /avoidCollisions=\{false\}/, `${relativePath} must not disable collision avoidance`);
      assert.doesNotMatch(tag, /collisionPadding=\{0\}/, `${relativePath} must not remove collision padding`);
      assert.doesNotMatch(
        classAttribute(tag),
        /\b(?:max-h-|overflow(?:-[xy])?-(?:auto|hidden|scroll|visible|clip)\b)/,
        `${relativePath} must not override the shared max-height or overflow contract`
      );
    }
  }
});
