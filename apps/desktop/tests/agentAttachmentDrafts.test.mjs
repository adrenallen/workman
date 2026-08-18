import assert from 'node:assert/strict';
import test from 'node:test';

import {
  attachmentName,
  attachImagePaths,
  handleNativePromptDrop,
  isPlatformAbsolutePath
} from '../src/lib/agentAttachmentDrafts.ts';

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
