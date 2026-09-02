import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

import {
  compileFeedbackTimeline,
  moveFeedbackBlock,
  removeFeedbackBlock,
  replaceFeedbackText
} from '../src/lib/recordedFeedbackTimeline.ts';
import { defaultHotkeyPreferences } from '../src/lib/hotkeys.ts';
import { recordingHotkeyBindings } from '../src/lib/recordedFeedbackHotkeys.ts';

test('timeline preserves image anchors and uses the containing segment fallback', () => {
  const segments = [
    { start_ms: 100, end_ms: 1_000, text: 'I do not like this button' },
    { start_ms: 1_100, end_ms: 1_600, text: 'and this menu.' },
    { start_ms: 2_500, end_ms: 3_000, text: 'The footer also jumps.' }
  ];
  const snapshots = [
    { id: 11, ordinal: 0, anchor_ms: 600, anchor_samples: 9_600 },
    { id: 12, ordinal: 1, anchor_ms: 2_000, anchor_samples: 32_000 }
  ];
  assert.deepEqual(compileFeedbackTimeline(segments, snapshots), [
    { kind: 'text', text: 'I do not like this button', start_ms: 100, end_ms: 1_000 },
    { kind: 'image', snapshot_id: 11 },
    { kind: 'text', text: 'and this menu.', start_ms: 1_100, end_ms: 1_600 },
    { kind: 'image', snapshot_id: 12 },
    { kind: 'text', text: 'The footer also jumps.', start_ms: 2_500, end_ms: 3_000 }
  ]);
});

test('silence produces an image-only timeline in stable sample order', () => {
  const snapshots = [
    { id: 2, ordinal: 1, anchor_ms: 100, anchor_samples: 2_000 },
    { id: 1, ordinal: 0, anchor_ms: 100, anchor_samples: 1_000 }
  ];
  assert.deepEqual(compileFeedbackTimeline([], snapshots), [
    { kind: 'image', snapshot_id: 1 },
    { kind: 'image', snapshot_id: 2 }
  ]);
});

test('document editing helpers are immutable and reject invalid operations', () => {
  const blocks = [{ kind: 'text', text: 'Before', start_ms: 0, end_ms: 1 }];
  const edited = replaceFeedbackText(blocks, 0, 'After');
  assert.equal(blocks[0].text, 'Before');
  assert.equal(edited[0].text, 'After');
  assert.equal(moveFeedbackBlock(blocks, -1, 0), blocks);
  assert.deepEqual(removeFeedbackBlock(edited, 0), []);
});

test('recording shortcuts have unique session-scoped defaults', () => {
  const preferences = defaultHotkeyPreferences();
  const entries = Object.entries(recordingHotkeyBindings(preferences));
  assert.equal(entries.length, 7);
  assert.equal(new Set(entries.map(([, value]) => value)).size, entries.length);
  assert.ok(!entries.some(([, value]) => /Shift\+(?:Digit)?[34]$/.test(value)),
    'defaults must not take over the macOS screenshot shortcuts');
});

test('recorded feedback is wired through preflight, durable events, review, and delivery', async () => {
  const [app, tree, preflight, detail, native, daemon, release] = await Promise.all([
    readFile(new URL('../src/App.svelte', import.meta.url), 'utf8'),
    readFile(new URL('../src/lib/ProjectTree.svelte', import.meta.url), 'utf8'),
    readFile(new URL('../src/lib/RecordedFeedbackPreflight.svelte', import.meta.url), 'utf8'),
    readFile(new URL('../src/lib/RecordedFeedbackDetailView.svelte', import.meta.url), 'utf8'),
    readFile(new URL('../src-tauri/src/recorded_feedback/macos.rs', import.meta.url), 'utf8'),
    readFile(new URL('../../../crates/workmand/src/recorded_feedback.rs', import.meta.url), 'utf8'),
    readFile(new URL('../../../scripts/release.sh', import.meta.url), 'utf8')
  ]);
  assert.match(tree, /feedback: 'Feedback'/);
  assert.match(app, /listen<NativeFeedbackSnapshot>\('feedback:\/\/snapshot'/);
  assert.match(app, /compileFeedbackTimeline\(result\.segments, current\.snapshots\)/);
  assert.match(preflight, /Audio, images, and transcription stay on this computer/);
  assert.match(detail, /onSendAgent/);
  assert.match(detail, /onSendScratchpad/);
  assert.match(native, /\.content_protected\(true\)/);
  assert.match(native, /\.nonactivating_panel\(\)/);
  assert.match(native, /pub\(crate\) async fn feedback_capture_snapshot/);
  assert.match(native, /run_on_main_thread/);
  assert.match(native, /capture_in_progress/);
  assert.match(native, /"feedback_id": feedback_id, "project_id": project_id/);
  assert.match(native, /pub\(crate\) fn feedback_abort/);
  assert.match(daemon, /\.join\(format!\("r\{\}", feedback\.revision\)\)/);
  assert.match(daemon, /scratchpad_packet_content/);
  assert.match(daemon, /remove_abandoned_packet_builds/);
  assert.match(app, /\['feedback_not_found', 'feedback_invalid_state', 'project_not_found'\]/);
  assert.match(detail, /if \(feedback\.status === 'ready'\) await afterSave\(onArchive\)/);
  const shortcutHandler = app.indexOf('function handleConfiguredHotkey');
  const recordingGuard = app.indexOf('recordingHotkeyActions as readonly string[]', shortcutHandler);
  const preventDefault = app.indexOf('event.preventDefault()', shortcutHandler);
  assert.ok(recordingGuard > shortcutHandler && recordingGuard < preventDefault,
    'inactive recording shortcuts must return before the app swallows the key');
  assert.match(release, /com\.apple\.security\.device\.audio-input/);
});
