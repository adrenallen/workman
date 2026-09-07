import assert from 'node:assert/strict';
import test from 'node:test';
import { ChangeSet, Text } from '@codemirror/state';
import { markdownCodeBlocks } from '../src/lib/markdownCodeBlocks.ts';
import { documentPosition, rememberDocumentPosition, markdownChange } from '../src/lib/documentViewMemory.ts';
import { editorChangeAction, markdownHash } from '../src/lib/scratchpadEditorSession.ts';
import { readScratchpadDraft, storeScratchpadDraft } from '../src/lib/scratchpadDraftMemory.ts';

test('code fences preserve literal Markdown, whitespace, and exact copy boundaries', () => {
  const code = '  **literal** [link](url)\n\n\tconsole.log(`hello`);\n';
  const source = `Before\n\n\`\`\`javascript\n${code}\`\`\`\nAfter`;
  const [block] = markdownCodeBlocks(source);
  assert.equal(block.language, 'javascript');
  assert.equal(source.slice(block.contentFrom, block.contentTo), code);
  assert.equal(source.slice(block.closingFrom, block.to), '```');
  assert.equal(markdownCodeBlocks(source).length, 1);
});

test('fences match marker type and length, including unclosed fences while typing', () => {
  const source = '````md\n```\n~~~\n````\n\n~~~text\nhello\n~~~';
  const blocks = markdownCodeBlocks(source);
  assert.equal(blocks.length, 2);
  assert.equal(source.slice(blocks[0].contentFrom, blocks[0].contentTo), '```\n~~~\n');
  const [open] = markdownCodeBlocks('```js\nconst x = 1;');
  assert.equal(open.closingFrom, null);
  assert.equal(open.contentTo, 18);
  assert.equal(markdownCodeBlocks('inline ``` text\n    ```\n```bad`info').length, 0);
});

test('code blocks spanning well beyond a viewport retain their opening fence', () => {
  const source = '```\n' + 'code\n'.repeat(1000) + '```';
  const [block] = markdownCodeBlocks(source);
  assert.equal(block.from, 0);
  assert.equal(block.contentTo, 5004);
});

test('external document changes map the cursor through the changed region only', () => {
  const before = 'Heading\nKeep editing this paragraph\nFooter';
  const after = 'New Heading\nKeep editing this paragraph\nFooter';
  const changes = ChangeSet.of(markdownChange(before, after), before.length);
  const cursor = before.indexOf('paragraph');
  assert.equal(changes.mapPos(cursor), after.indexOf('paragraph'));
  assert.equal(changes.apply(Text.of(before.split('\n'))).toString(), after);
});

test('scroll and cursor memory merge independently and stay separate per scratchpad', () => {
  rememberDocumentPosition('test:one', { top: 14000, left: 2 });
  rememberDocumentPosition('test:one', { anchor: 330, head: 337 });
  rememberDocumentPosition('test:two', { top: 44 });
  assert.deepEqual(documentPosition('test:one'), { top: 14000, left: 2, anchor: 330, head: 337 });
  assert.deepEqual(documentPosition('test:two'), { top: 44 });
});

test('unfinished drafts survive a pane switch and are cleared after saving', () => {
  const draft = { markdown: '# Edited', baseMarkdown: '# Old', baseRevision: 5 };
  storeScratchpadDraft('test:draft', draft);
  assert.deepEqual(readScratchpadDraft('test:draft'), draft);
  assert.equal(readScratchpadDraft('test:other'), null);
  storeScratchpadDraft('test:draft', null);
  assert.equal(readScratchpadDraft('test:draft'), null);
});

test('editor sync distinguishes new saves, local edits, and concurrent agent changes', () => {
  const session = { path: '/scratchpad.md', baseHash: 'base', observedHash: 'seen' };
  assert.equal(editorChangeAction(session, 'seen', 'local', true), 'unchanged');
  assert.equal(editorChangeAction(session, 'new', 'new', false), 'acknowledge');
  assert.equal(editorChangeAction(session, 'new', 'base', false), 'import');
  assert.equal(editorChangeAction(session, 'new', 'local', true), 'pending');
  assert.equal(editorChangeAction(session, 'new', 'agent', false), 'conflict');
});

test('editor fingerprints include whitespace and Unicode', async () => {
  assert.equal(await markdownHash('📝'), await markdownHash('📝'));
  assert.notEqual(await markdownHash('code'), await markdownHash('code\n'));
});
