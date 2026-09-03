import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

import {
  agentDraftImageToken,
  agentDraftPromptInputSteps,
  attachmentName,
  attachImagePaths,
  handleNativePromptDrop,
  insertAgentDraftImageTokens,
  isPlatformAbsolutePath,
  removeAgentDraftAttachment
} from '../src/lib/agentAttachmentDrafts.ts';

test('pasted image tokens are inserted at the exact prompt selection', () => {
  assert.deepEqual(insertAgentDraftImageTokens('move this button', 5, 9, 0, 2), {
    prompt: 'move [Image 1] [Image 2] button',
    caret: 24
  });
  assert.equal(agentDraftImageToken(2), '[Image 3]');
});

test('removing an image removes its token and renumbers later placeholders', () => {
  assert.deepEqual(
    removeAgentDraftAttachment(
      'Before [Image 1] middle [Image 2] after',
      ['/tmp/one.png', '/tmp/two.png'],
      '/tmp/one.png'
    ),
    {
      prompt: 'Before middle [Image 1] after',
      attachments: ['/tmp/two.png']
    }
  );
});

test('prompt image tokens become real image steps in exact text order', () => {
  assert.deepEqual(
    agentDraftPromptInputSteps(
      'Words before [Image 2] words between [Image 1] words after',
      ['/tmp/one.png', '/tmp/two.png']
    ),
    [
      { kind: 'text', text: 'Words before ' },
      { kind: 'image', path: '/tmp/two.png' },
      { kind: 'text', text: ' words between ' },
      { kind: 'image', path: '/tmp/one.png' },
      { kind: 'text', text: ' words after' }
    ]
  );
  assert.deepEqual(agentDraftPromptInputSteps('Legacy prompt', ['/tmp/one.png']), [
    { kind: 'text', text: 'Legacy prompt' },
    { kind: 'text', text: '\n\n' },
    { kind: 'image', path: '/tmp/one.png' }
  ]);
});

test('new-agent creation defers tokenized prompts for real inline image delivery', async () => {
  const [app, panel, spawning] = await Promise.all([
    readFile(new URL('../src/App.svelte', import.meta.url), 'utf8'),
    readFile(new URL('../src/lib/NewAgentDraftPanel.svelte', import.meta.url), 'utf8'),
    readFile(new URL('../../../crates/workmand/src/mcp/agent_spawning.rs', import.meta.url), 'utf8')
  ]);
  assert.match(panel, /agentDraftImageToken\(index\)/);
  assert.match(panel, /insertAgentDraftImageTokens/);
  assert.match(app, /feedbackId !== null \|\| Boolean\(submission\.input\.attachments\?\.length\)/);
  assert.match(app, /agentDraftPromptInputSteps\(/);
  assert.match(app, /result\.deferred_attachments \?\? \[\]/);
  assert.match(spawning, /result\.deferred_attachments = deferred_attachments/);
});

test('native attachment paths filter extensions, dedupe, and respect the eight-item cap', () => {
  const current = ['/tmp/current.png'];
  const candidates = [
    '/tmp/current.png',
    '/tmp/one.jpg',
    '/tmp/not-an-image.txt',
    'relative/two.png',
    'C:\\images\\two.WEBP',
    '/tmp/three.gif',
    '/tmp/four.bmp',
    '/tmp/five.tif',
    '/tmp/six.tiff',
    '/tmp/seven.jpeg',
    '/tmp/over-cap.png'
  ];
  const result = attachImagePaths(current, candidates);

  assert.deepEqual(result.attachments, [
    '/tmp/current.png',
    '/tmp/one.jpg',
    'C:\\images\\two.WEBP',
    '/tmp/three.gif',
    '/tmp/four.bmp',
    '/tmp/five.tif',
    '/tmp/six.tiff',
    '/tmp/seven.jpeg'
  ]);
  assert.equal(result.added.length, 7);
  assert.equal(result.capReached, true);
});

test('native prompt drop changes hover state and attaches only drops inside the prompt rect', () => {
  const rect = { left: 100, top: 50, right: 500, bottom: 350 };
  const inside = { x: 400, y: 300 };
  const outside = { x: 40, y: 300 };
  const current = ['/tmp/current.png'];

  assert.deepEqual(
    handleNativePromptDrop({ type: 'over', paths: [], position: inside }, rect, 2, current),
    { dropActive: true, selection: null }
  );
  assert.deepEqual(
    handleNativePromptDrop({ type: 'over', paths: [], position: outside }, rect, 2, current),
    { dropActive: false, selection: null }
  );
  assert.deepEqual(
    handleNativePromptDrop(
      { type: 'drop', paths: ['/tmp/new.png'], position: inside },
      rect,
      2,
      current
    ).selection?.added,
    ['/tmp/new.png']
  );
  assert.equal(
    handleNativePromptDrop(
      { type: 'drop', paths: ['/tmp/new.png'], position: outside },
      rect,
      2,
      current
    ).selection,
    null
  );
  assert.deepEqual(
    handleNativePromptDrop({ type: 'leave', paths: [], position: inside }, rect, 2, current),
    { dropActive: false, selection: null }
  );
});

test('platform path helpers accept Unix and Windows roots and split either separator', () => {
  assert.equal(isPlatformAbsolutePath('/tmp/image.png'), true);
  assert.equal(isPlatformAbsolutePath('C:\\Users\\g\\image.png'), true);
  assert.equal(isPlatformAbsolutePath('D:/images/image.png'), true);
  assert.equal(isPlatformAbsolutePath('relative/image.png'), false);
  assert.equal(attachmentName('C:\\Users\\g\\image.png'), 'image.png');
  assert.equal(attachmentName('/tmp/image.png'), 'image.png');
});
