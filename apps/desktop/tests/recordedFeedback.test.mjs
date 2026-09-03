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
import {
  deliverFeedbackAgentInput,
  feedbackAgentInputSteps
} from '../src/lib/recordedFeedbackAgentDelivery.ts';

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

test('agent delivery preserves transcript and image order in one submitted CLI turn', async () => {
  const feedback = {
    title: 'Navigation feedback',
    blocks: [
      { kind: 'text', text: 'Move this button.' },
      { kind: 'image', snapshot_id: 11 },
      { kind: 'text', text: 'Then simplify the menu.' },
      { kind: 'image', snapshot_id: 12 }
    ],
    snapshots: [
      { id: 11, ordinal: 0, caption: 'Current button', image_path: '/feedback/one.png' },
      { id: 12, ordinal: 1, caption: '', image_path: '/feedback/two.png' }
    ]
  };
  const steps = feedbackAgentInputSteps(feedback);
  assert.deepEqual(steps.map((step) => step.kind), ['text', 'text', 'text', 'image', 'text', 'text', 'image', 'text']);

  const events = [];
  const decoder = new TextDecoder();
  await deliverFeedbackAgentInput(steps, {
    send: async (data) => events.push(`send:${decoder.decode(data)}`),
    writeImageToClipboard: async (path) => events.push(`image:${path}`),
    waitForImageImport: async () => events.push('wait')
  });
  assert.equal(events.filter((event) => event === 'send:\u0016').length, 2);
  assert.equal(events.at(-1), 'send:\r');
  assert.ok(events.indexOf('image:/feedback/one.png') < events.indexOf('image:/feedback/two.png'));
  assert.ok(events.some((event) => event.includes('Move this button.')));
  assert.ok(events.some((event) => event.includes('Then simplify the menu.')));
});

test('recorded feedback is wired through preflight, durable events, review, and delivery', async () => {
  const [app, tree, preflight, toolbar, overlay, detail, native, capability, daemon, release, devInstall] = await Promise.all([
    readFile(new URL('../src/App.svelte', import.meta.url), 'utf8'),
    readFile(new URL('../src/lib/ProjectTree.svelte', import.meta.url), 'utf8'),
    readFile(new URL('../src/lib/RecordedFeedbackPreflight.svelte', import.meta.url), 'utf8'),
    readFile(new URL('../src/lib/RecordedFeedbackToolbar.svelte', import.meta.url), 'utf8'),
    readFile(new URL('../src/lib/RecordedFeedbackOverlay.svelte', import.meta.url), 'utf8'),
    readFile(new URL('../src/lib/RecordedFeedbackDetailView.svelte', import.meta.url), 'utf8'),
    readFile(new URL('../src-tauri/src/recorded_feedback/macos.rs', import.meta.url), 'utf8'),
    readFile(new URL('../src-tauri/capabilities/default.json', import.meta.url), 'utf8'),
    readFile(new URL('../../../crates/workmand/src/recorded_feedback.rs', import.meta.url), 'utf8'),
    readFile(new URL('../../../scripts/release.sh', import.meta.url), 'utf8'),
    readFile(new URL('../../../scripts/dev-install.sh', import.meta.url), 'utf8')
  ]);
  assert.match(tree, /feedback: 'Feedback'/);
  assert.match(app, /listen<NativeFeedbackSnapshot>\('feedback:\/\/snapshot'/);
  assert.match(app, /compileFeedbackTimeline\(result\.segments, current\.snapshots\)/);
  assert.match(preflight, /Audio, images, and transcription stay on this computer/);
  assert.match(preflight, /showCloseButton=\{false\}/);
  assert.match(preflight, /screen_capture_authorized/);
  assert.match(native, /let screen_capture_authorized = CGPreflightScreenCaptureAccess\(\)/);
  assert.match(preflight, /remove it, add the current app again/);
  assert.match(native, /let display_available = Monitor::all\(\)/);
  assert.match(app, /&& feedbackPreflightOpen\s+&& !feedbackPreflight\?\.screen_capture_available/);
  assert.match(detail, /onSendAgent/);
  assert.match(detail, /onSendScratchpad/);
  assert.match(native, /\.content_protected\(true\)/);
  assert.match(native, /\.nonactivating_panel\(\)/);
  assert.match(native, /PanelLevel::Custom\(1001\)/);
  assert.match(native, /toolbar\.order_front_regardless\(\)/);
  assert.match(native, /pub\(crate\) async fn feedback_capture_snapshot/);
  assert.match(native, /run_on_main_thread/);
  assert.match(native, /capture_in_progress/);
  assert.match(native, /"feedback_id": feedback_id, "project_id": project_id/);
  assert.match(native, /pub\(crate\) fn feedback_abort/);
  assert.match(native, /panel\.to_window\(\)/);
  assert.match(native, /pub\(crate\) fn feedback_raise_toolbar/);
  assert.match(daemon, /\.join\(format!\("r\{\}", feedback\.revision\)\)/);
  assert.match(daemon, /scratchpad_packet_content/);
  assert.match(daemon, /remove_abandoned_packet_builds/);
  assert.match(app, /\['feedback_not_found', 'feedback_invalid_state', 'project_not_found'\]/);
  assert.match(detail, /if \(feedback\.status === 'ready'\) await afterSave\(onArchive\)/);
  assert.match(toolbar, /getCurrentWindow\(\)\.startDragging\(\)/);
  assert.match(toolbar, /> Snap region/);
  assert.match(toolbar, /Snap display/);
  assert.match(toolbar, /snapshot_count/);
  assert.match(toolbar, /Screenshot saved/);
  assert.match(toolbar, /invoke\('feedback_raise_toolbar'\)/);
  assert.match(overlay, /feedback:\/\/snapshot/);
  assert.match(overlay, /capture-flash/);
  assert.doesNotMatch(overlay, /clearRect\(region\.x/);
  assert.match(daemon, /const LEASE_MS: i64 = 15_000/);
  assert.match(app, /feedbackSummaries\.some\(\(feedback\)/);
  assert.match(app, /deliverFeedbackAgentInput\(feedbackAgentInputSteps\(feedback\)/);
  assert.match(app, /Promise\.all\(\[loadFeedback\(next\.id\), refreshProcesses\(next\.projectId\)\]\)/);
  assert.match(detail, /height: 100%; min-height: 0/);
  assert.doesNotMatch(detail, /footer \{ position: sticky/);
  assert.match(daemon, /params\.direct_input/);
  assert.match(capability, /core:window:allow-start-dragging/);
  const shortcutHandler = app.indexOf('function handleConfiguredHotkey');
  const recordingGuard = app.indexOf('recordingHotkeyActions as readonly string[]', shortcutHandler);
  const preventDefault = app.indexOf('event.preventDefault()', shortcutHandler);
  assert.ok(recordingGuard > shortcutHandler && recordingGuard < preventDefault,
    'inactive recording shortcuts must return before the app swallows the key');
  assert.match(release, /com\.apple\.security\.device\.audio-input/);
  assert.match(devInstall, /Developer ID Application/);
  assert.match(devInstall, /codesign --force --deep --timestamp=none --options runtime/);
  const signSource = devInstall.indexOf('--entitlements "$repo_root/apps/desktop/src-tauri/Entitlements.plist" "$source_app"');
  const copySource = devInstall.indexOf('ditto "$source_app" "$app_stage"');
  assert.ok(signSource >= 0 && copySource > signSource,
    'the indexed source app must have the stable identity before it is copied');
});

test('recorded feedback is platform-gated and its sidebar section is optional', async () => {
  const [app, native, availability, tree, quickJump, sidebar, hotkeys] = await Promise.all([
    readFile(new URL('../src/App.svelte', import.meta.url), 'utf8'),
    readFile(new URL('../src-tauri/src/recorded_feedback.rs', import.meta.url), 'utf8'),
    readFile(new URL('../src/lib/recordedFeedbackAvailability.ts', import.meta.url), 'utf8'),
    readFile(new URL('../src/lib/ProjectTree.svelte', import.meta.url), 'utf8'),
    readFile(new URL('../src/lib/QuickJumpPalette.svelte', import.meta.url), 'utf8'),
    readFile(new URL('../src/lib/settings/SidebarCard.svelte', import.meta.url), 'utf8'),
    readFile(new URL('../src/lib/settings/HotkeysCard.svelte', import.meta.url), 'utf8')
  ]);

  assert.match(native, /pub\(crate\) fn feedback_capability\(\)/);
  assert.match(native, /supported: cfg!\(target_os = "macos"\)/);
  assert.match(availability, /workman\.feedback\.preferences\.v1/);
  assert.match(availability, /capability\.supported && preferences\.showInSidebar/);
  assert.match(tree, /group !== 'feedback' \|\| showFeedback/);
  assert.match(quickJump, /if \(feedbackSupported\) \{/);
  assert.match(sidebar, /id="feedback-sidebar-visible"/);
  assert.match(sidebar, /Hiding it keeps existing recordings and hotkeys available/);
  assert.match(hotkeys, /group\.id !== 'feedback' \|\| \$recordedFeedbackSupported/);
  assert.match(app, /void refreshRecordedFeedbackCapability\(\)/);
  assert.match(app, /if \(selection\?\.kind === 'feedback'\) clearSelection\(\)/);
  assert.match(app, /next\.kind === 'feedback' && !\$recordedFeedbackSupported/);
  const shortcutHandler = app.indexOf('function handleConfiguredHotkey');
  const supportGuard = app.indexOf("action === 'start-feedback' && !$recordedFeedbackSupported", shortcutHandler);
  const preventDefault = app.indexOf('event.preventDefault()', shortcutHandler);
  assert.ok(supportGuard > shortcutHandler && supportGuard < preventDefault,
    'unsupported feedback launch shortcuts must not be swallowed');
});
