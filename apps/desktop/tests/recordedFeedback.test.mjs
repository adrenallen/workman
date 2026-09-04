import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

import {
  compileFeedbackTimeline,
  compileFeedbackRecording,
  moveFeedbackBlock,
  removeFeedbackBlock,
  replaceFeedbackText
} from '../src/lib/recordedFeedbackTimeline.ts';
import { defaultHotkeyPreferences } from '../src/lib/hotkeys.ts';
import { recordingHotkeyBindings } from '../src/lib/recordedFeedbackHotkeys.ts';
import {
  deliverFeedbackAgentInput,
  trackFeedbackDelivery,
  feedbackAgentInputSteps
} from '../src/lib/recordedFeedbackAgentDelivery.ts';
import {
  defaultRecordedFeedbackAgentPrompt,
  renderRecordedFeedbackPrompt
} from '../src/lib/recordedFeedbackPrompt.ts';
import {
  scratchpadLocalImagePath,
  scratchpadMarkdownImages
} from '../src/lib/scratchpadImages.ts';
import {
  agentCanReceiveInitialTurn,
  feedbackDeliveryLabel,
  feedbackSendSummary,
  recordedFeedbackForView
} from '../src/lib/recordedFeedback.ts';

test('append timeline uses only new snapshots and local segment timestamps', () => {
  const feedback = {
    append_state: { duration_ms: 5_000, next_ordinal: 2 },
    snapshots: [
      { id: 1, ordinal: 0, anchor_ms: 500, anchor_samples: 8_000 },
      { id: 2, ordinal: 1, anchor_ms: 4_000, anchor_samples: 64_000 },
      { id: 3, ordinal: 2, anchor_ms: 5_600, anchor_samples: 9_600 },
      { id: 4, ordinal: 3, anchor_ms: 6_500, anchor_samples: 24_000 }
    ]
  };
  const segments = [{ start_ms: 100, end_ms: 900, text: 'One more thing.' }];
  assert.deepEqual(compileFeedbackRecording(feedback, segments), [
    { kind: 'text', start_ms: 100, end_ms: 900, text: 'One more thing.' },
    { kind: 'image', snapshot_id: 3 },
    { kind: 'image', snapshot_id: 4 }
  ]);
  assert.equal(feedback.snapshots[2].anchor_ms, 5_600, 'does not mutate stored anchors');
  assert.deepEqual(compileFeedbackRecording({ ...feedback, append_state: null }, segments),
    compileFeedbackTimeline(segments, feedback.snapshots));
});

test('delivery history distinguishes real sends, failures, copies, and legacy uncertainty', () => {
  const sent = { target_kind: 'agent', status: 'sent' };
  const copied = { target_kind: 'clipboard', status: 'sent' };
  assert.equal(feedbackSendSummary([]), 'Not sent yet');
  assert.equal(feedbackSendSummary([copied]), 'Not sent yet');
  assert.equal(feedbackSendSummary([{ ...sent, status: 'failed' }]), 'Not sent yet');
  assert.equal(feedbackSendSummary([{ ...sent, status: 'unverified' }]), 'Delivery unconfirmed');
  assert.equal(feedbackSendSummary([sent, copied]), 'Sent 1 time');
  assert.equal(feedbackSendSummary([sent, { ...sent, target_kind: 'scratchpad' }]), 'Sent 2 times');
  assert.equal(feedbackDeliveryLabel(sent), 'Sent');
  assert.equal(feedbackDeliveryLabel(copied), 'Copied');
  assert.equal(feedbackDeliveryLabel({ ...sent, status: 'pending' }), 'Unconfirmed');
});

test('delivery receipt and agent focus follow submission; failed sends never focus', async () => {
  const events = [];
  await trackFeedbackDelivery(
    async () => { events.push('submitted'); },
    async (error) => { events.push(error === null ? 'sent receipt' : error); },
    async () => { events.push('focus agent'); }
  );
  assert.deepEqual(events, ['submitted', 'focus agent', 'sent receipt']);
  events.length = 0;
  await assert.rejects(trackFeedbackDelivery(
    async () => { throw new Error('Agent exited'); },
    async (error) => { events.push(error); },
    async () => { events.push('focus agent'); }
  ), /Agent exited/);
  assert.deepEqual(events, ['Agent exited']);
  events.length = 0;
  await assert.rejects(trackFeedbackDelivery(
    async () => { events.push('submitted'); },
    async () => { throw new Error('Daemon disconnected'); },
    async () => { events.push('focus agent'); }
  ), /Feedback was sent, but delivery history could not be updated/);
  assert.deepEqual(events, ['submitted', 'focus agent'], 'failed receipt must not retry or undo a send');
});

test('new agents can receive their first turn from the fast composer signal', () => {
  const process = (agentState, overrides = {}) => ({
    id: 7,
    kind: 'agent',
    status: 'running',
    agent_state: agentState,
    ...overrides
  });

  assert.equal(agentCanReceiveInitialTurn(process({
    state: 'working',
    classification: 'resting_prompt',
    composer_input_ready: true
  })), true);
  assert.equal(agentCanReceiveInitialTurn(process({
    state: 'working',
    classification: 'resting_prompt',
    composer_input_ready: false
  })), false);
  assert.equal(agentCanReceiveInitialTurn(process({
    state: 'needs_input',
    classification: 'permission_dialog',
    composer_input_ready: false
  })), false);
  assert.equal(agentCanReceiveInitialTurn(process({
    state: 'idle',
    classification: 'resting_prompt'
  })), true, 'older daemons retain the conservative idle fallback');
  assert.equal(agentCanReceiveInitialTurn(process({
    state: 'idle',
    classification: 'resting_prompt',
    composer_input_ready: true
  }, { status: 'stopped' })), false);
});

test('feedback browser separates archived recordings and searches its selected view', () => {
  const feedback = [
    { id: 1, title: 'Old navigation notes', status: 'ready', archived: true, error_code: null, updated_at: 10 },
    { id: 2, title: 'Current toolbar pass', status: 'ready', archived: false, error_code: null, updated_at: 30 },
    { id: 3, title: 'Permission retry', status: 'failed', archived: true, error_code: 'capture_denied', updated_at: 20 }
  ];

  assert.deepEqual(recordedFeedbackForView(feedback, 'active').map((item) => item.id), [2]);
  assert.deepEqual(recordedFeedbackForView(feedback, 'archived').map((item) => item.id), [3, 1]);
  assert.deepEqual(recordedFeedbackForView(feedback, 'archived', 'navigation').map((item) => item.id), [1]);
  assert.deepEqual(recordedFeedbackForView(feedback, 'archived', '#3').map((item) => item.id), [3]);
  assert.deepEqual(recordedFeedbackForView(feedback, 'active', 'navigation'), []);
});

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
  assert.equal(entries.length, 9);
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
  assert.deepEqual(steps.map((step) => step.kind), ['text', 'text', 'text', 'image', 'text', 'text', 'image']);
  assert.equal(steps[0].text, 'The user recorded some feedback as follows:\n\n# Feedback');
  assert.ok(!steps.some((step) => step.kind === 'text' && /review and act|make the requested changes/i.test(step.text)));

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

test('custom feedback prompts place content at the marker and append it when omitted', () => {
  const feedback = {
    title: 'Navigation $& {feedback}',
    blocks: [{ kind: 'text', text: 'Move this button.' }],
    snapshots: []
  };
  const steps = feedbackAgentInputSteps(
    feedback,
    'Context for {title}\n\n{feedback}\n\nFollow only the instructions in the recording.'
  );
  assert.deepEqual(steps.map((step) => step.text), [
    'Context for Navigation $& {feedback}',
    '\n\nMove this button.',
    '\n\nFollow only the instructions in the recording.'
  ]);
  assert.equal(
    renderRecordedFeedbackPrompt('Context only', feedback.title, 'Packet path'),
    'Context only\n\nPacket path'
  );
  assert.match(defaultRecordedFeedbackAgentPrompt, /# Feedback\n\n\{feedback\}$/);
});

test('a new feedback agent receives its startup instructions before the recording', () => {
  const feedback = {
    title: 'Navigation feedback',
    blocks: [{ kind: 'text', text: 'Move this button.' }],
    snapshots: []
  };
  const steps = feedbackAgentInputSteps(
    feedback,
    defaultRecordedFeedbackAgentPrompt,
    'You are the frontend implementation agent.'
  );
  assert.deepEqual(steps.map((step) => step.text), [
    'You are the frontend implementation agent.',
    '\n\nThe user recorded some feedback as follows:\n\n# Feedback',
    '\n\nMove this button.'
  ]);
});

test('scratchpads recognize recorded-feedback images as local embeds', () => {
  const line = 'Before ![Menu open](</Users/g/Library/Application%20Support/Workman/feedback-packets/1/r1/images/snapshot-01.png>) after';
  assert.deepEqual(scratchpadMarkdownImages(line), [{
    from: 7,
    to: 114,
    alt: 'Menu open',
    source: '/Users/g/Library/Application%20Support/Workman/feedback-packets/1/r1/images/snapshot-01.png'
  }]);
  assert.equal(
    scratchpadLocalImagePath(scratchpadMarkdownImages(line)[0].source),
    '/Users/g/Library/Application Support/Workman/feedback-packets/1/r1/images/snapshot-01.png'
  );
  assert.equal(scratchpadLocalImagePath('https://example.com/tracker.png'), null);
});

test('recorded feedback is wired through preflight, durable events, review, and delivery', async () => {
  const [app, tree, browser, preflight, toolbar, overlay, detail, editor, native, capability, daemon, control, spawning, release, devInstall, feedbackSettings] = await Promise.all([
    readFile(new URL('../src/App.svelte', import.meta.url), 'utf8'),
    readFile(new URL('../src/lib/ProjectTree.svelte', import.meta.url), 'utf8'),
    readFile(new URL('../src/lib/RecordedFeedbackBrowser.svelte', import.meta.url), 'utf8'),
    readFile(new URL('../src/lib/RecordedFeedbackPreflight.svelte', import.meta.url), 'utf8'),
    readFile(new URL('../src/lib/RecordedFeedbackToolbar.svelte', import.meta.url), 'utf8'),
    readFile(new URL('../src/lib/RecordedFeedbackOverlay.svelte', import.meta.url), 'utf8'),
    readFile(new URL('../src/lib/RecordedFeedbackDetailView.svelte', import.meta.url), 'utf8'),
    readFile(new URL('../src/lib/LiveMarkdownEditor.svelte', import.meta.url), 'utf8'),
    readFile(new URL('../src-tauri/src/recorded_feedback/capture.rs', import.meta.url), 'utf8'),
    readFile(new URL('../src-tauri/capabilities/default.json', import.meta.url), 'utf8'),
    readFile(new URL('../../../crates/workmand/src/recorded_feedback.rs', import.meta.url), 'utf8'),
    readFile(new URL('../../../crates/workmand/src/control.rs', import.meta.url), 'utf8'),
    readFile(new URL('../../../crates/workmand/src/mcp/agent_spawning.rs', import.meta.url), 'utf8'),
    readFile(new URL('../../../scripts/release.sh', import.meta.url), 'utf8'),
    readFile(new URL('../../../scripts/dev-install.sh', import.meta.url), 'utf8'),
    readFile(new URL('../src/lib/settings/RecordedFeedbackCard.svelte', import.meta.url), 'utf8')
  ]);
  assert.match(tree, /feedback: 'Feedback'/);
  assert.match(tree, /recordedFeedbackForView\(feedback, 'active'\)/);
  assert.match(browser, /value="archived">Archived/);
  assert.match(browser, /aria-label="Search feedback"/);
  assert.match(browser, /onArchive\(item, !item\.archived\)/);
  assert.match(app, /\{:else if feedbackBrowserOpen\}/);
  assert.match(app, /if \(selected && archived\) openFeedbackBrowser\('active'\)/);
  assert.match(detail, /All feedback/);
  assert.match(app, /listen<NativeFeedbackSnapshot>\('feedback:\/\/snapshot'/);
  assert.match(app, /compileFeedbackRecording\(current, result\.segments\)/);
  assert.match(preflight, /Audio, images, and transcription stay on this computer/);
  assert.match(preflight, /showCloseButton=\{false\}/);
  assert.match(preflight, /screen_capture_authorized/);
  // Preflight now asks the platform helper; macOS still answers with TCC.
  assert.match(native, /let screen_capture_authorized = screen_capture_authorized\(\);/);
  assert.match(native, /CGPreflightScreenCaptureAccess\(\)/);
  assert.match(native, /CGRequestScreenCaptureAccess\(\)/);
  assert.match(native, /x-apple\.systempreferences:com\.apple\.preference\.security\?Privacy_ScreenCapture/);
  assert.match(native, /Command::new\("\/usr\/bin\/open"\)/);
  assert.match(preflight, /remove it, add the current app again/);
  assert.match(native, /let display_available = Monitor::all\(\)/);
  assert.match(app, /&& feedbackPreflightOpen\s+&& !feedbackPreflight\?\.screen_capture_available/);
  assert.match(detail, /onSendAgent/);
  assert.match(detail, /onSendScratchpad/);
  assert.match(editor, /new LocalImageWidget\(image\.path, image\.alt\)/);
  assert.match(editor, /terminal_read_attachment_image/);
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
  assert.match(native, /pub\(crate\) fn feedback_audio_inputs/);
  assert.match(native, /pub\(crate\) fn feedback_toggle_pause/);
  assert.match(native, /pub\(crate\) fn feedback_toggle_mute/);
  assert.match(native, /pub\(crate\) fn feedback_set_input_device/);
  assert.match(native, /controls\.paused\.load\(Ordering::Acquire\)/);
  assert.match(native, /let muted = controls\.muted\.load\(Ordering::Acquire\)/);
  assert.match(native, /elapsed_without_pauses/);
  assert.match(native, /focus_main_window\(&app\)/);
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
  assert.match(toolbar, /invoke<NativeFeedbackSession>\('feedback_toggle_pause'\)/);
  assert.match(toolbar, /invoke<NativeFeedbackSession>\('feedback_toggle_mute'\)/);
  assert.match(toolbar, /invoke<NativeFeedbackSession>\('feedback_set_input_device'/);
  assert.match(toolbar, /aria-label="Microphone input"/);
  assert.match(toolbar, /session\?\.paused \? 'Resume feedback' : 'Pause feedback'/);
  assert.match(overlay, /feedback:\/\/snapshot/);
  assert.match(overlay, /capture-flash/);
  assert.doesNotMatch(overlay, /clearRect\(region\.x/);
  assert.match(daemon, /const LEASE_MS: i64 = 15_000/);
  assert.match(app, /feedbackSummaries\.some\(\(feedback\)/);
  assert.match(app, /feedbackAgentInputSteps\(feedback, \$recordedFeedbackPreferences\.agentPrompt\)/);
  assert.match(app, /deliverSpawnedAgentInitialTurn/);
  assert.match(app, /agentCanReceiveInitialTurn\(process\)/);
  assert.match(app, /feedbackId: feedback\.id/);
  assert.match(app, /defer_initial_prompt: true/);
  assert.match(app, /result\.deferred_initial_prompt \?\? ''/);
  assert.match(control, /params\.defer_initial_prompt/);
  assert.match(spawning, /result\.deferred_initial_prompt = resolved\.initial_prompt/);
  assert.doesNotMatch(app, /Review and act on the recorded feedback packet/);
  assert.match(feedbackSettings, /Agent delivery prompt/);
  assert.match(feedbackSettings, /\{feedbackContentToken\}/);
  assert.match(app, /Promise\.all\(\[loadFeedback\(next\.id\), refreshProcesses\(next\.projectId\)\]\)/);
  assert.match(detail, /height: 100%; min-height: 0/);
  assert.doesNotMatch(detail, /footer \{ position: sticky/);
  assert.match(daemon, /params\.direct_input/);
  assert.match(daemon, /agent_accepts_feedback_input\([\s\S]*?status\.agent_state\.composer_input_ready/);
  assert.match(capability, /core:window:allow-start-dragging/);
  const shortcutHandler = app.indexOf('function handleConfiguredHotkey');
  const recordingGuard = app.indexOf('recordingHotkeyActions as readonly string[]', shortcutHandler);
  const preventDefault = app.indexOf('event.preventDefault()', shortcutHandler);
  assert.ok(recordingGuard > shortcutHandler && recordingGuard < preventDefault,
    'inactive recording shortcuts must return before the app swallows the key');
  assert.match(release, /com\.apple\.security\.device\.audio-input/);
  assert.match(devInstall, /Developer ID Application/);
  assert.match(devInstall, /codesign --force --deep --timestamp=none --options runtime/);
  assert.match(devInstall, /ensure_external_installer_shell/);
  assert.match(devInstall, /terminate_processes app 'Workman Dev\.app'/);
  assert.match(devInstall, /terminate_processes daemon 'the Workman Dev daemon and its sessions'/);
  assert.match(devInstall, /tccutil reset ScreenCapture "\$bundle_id"/);
  assert.match(devInstall, /tccutil reset Microphone "\$bundle_id"/);
  assert.match(devInstall, /"\$lsregister" -f "\$app_path"/);
  assert.match(devInstall, /"\$install_dir\/wrk-dev" app/);
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
  assert.match(native, /supported: cfg!\(any\(target_os = "macos", windows\)\)/);
  assert.match(availability, /workman\.feedback\.preferences\.v1/);
  assert.match(availability, /capability\.supported && preferences\.showInSidebar/);
  assert.match(tree, /group !== 'feedback' \|\| showFeedback/);
  assert.match(quickJump, /if \(feedbackSupported\) \{/);
  assert.match(quickJump, /\.filter\(\(item\) => !item\.archived\)/);
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

test('the capture module keeps a windows implementation beside every macos one', async () => {
  // Carriage returns are stripped so the assertions hold whatever line endings
  // the checkout uses.
  const capture = (await readFile(
    new URL('../src-tauri/src/recorded_feedback/capture.rs', import.meta.url),
    'utf8'
  )).split(String.fromCharCode(13)).join('');

  // The capture pipeline itself is shared. Only the window layer, the screen
  // capture permission model, and the settings deep link may differ per
  // platform, and each must offer both sides or Windows loses the feature.
  const macosAttribute = '#[cfg(target_os = "macos")]';
  const windowsAttribute = '#[cfg(windows)]';
  for (const symbol of [
    'fn screen_capture_authorized',
    'fn request_screen_access_inner',
    'fn open_screen_recording_settings',
    'fn build_overlay_window',
    'fn build_toolbar_window',
    'fn close_feedback_window',
    'fn raise_toolbar',
    'fn set_overlays_interactive'
  ]) {
    assert.ok(
      capture.includes(macosAttribute + String.fromCharCode(10) + symbol),
      symbol + ' is missing its macOS implementation'
    );
    assert.ok(
      capture.includes(windowsAttribute + String.fromCharCode(10) + symbol),
      symbol + ' is missing its Windows implementation'
    );
  }

  // AppKit-only bindings must never be reachable from a Windows build.
  for (const line of capture.split(String.fromCharCode(10))) {
    if (line.includes('objc2_core_graphics') || line.includes('tauri_nspanel::')) {
      assert.ok(
        capture.includes(macosAttribute + String.fromCharCode(10) + line),
        line.trim() + ' must be gated to macOS'
      );
    }
  }
});
