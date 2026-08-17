import assert from 'node:assert/strict';
import test from 'node:test';

import {
  canPresentUpdateProgress,
  manualUpdateFlow,
  updateBannerState,
  updateCompletionAction
} from '../src/lib/updateFlow.ts';

test('only the locally active install can accept progress', () => {
  const running = {
    kind: 'running',
    progress: {
      stage: 'checking',
      message: 'Checking',
      bytes_done: null,
      bytes_total: null,
      percent: null,
      failed: false
    }
  };
  assert.equal(canPresentUpdateProgress(running, true), true);
  assert.equal(canPresentUpdateProgress(running, false), false);
  assert.equal(canPresentUpdateProgress({ kind: 'idle' }, true), false);
  assert.equal(
    canPresentUpdateProgress({ kind: 'failed', stage: 'downloading', message: 'timeout' }, true),
    false
  );
});

function report({ app, current = '0.1.9', latest = '0.2.0', bundle = app ? '/Applications/Workman.app' : null }) {
  return {
    current,
    latest,
    install_dir: '/tmp/workman',
    updated_files: [],
    desktop_instruction: null,
    installed_app_bundle: bundle,
    quarantine_cleared: false,
    restart_plan: { daemon: true, app }
  };
}

test('completion action relaunches only when the refreshed app can relaunch', () => {
  assert.equal(
    updateCompletionAction(report({ app: true }), {
      nativeRelaunchAvailable: true,
      appVersion: '0.1.9',
      appBundle: '/Applications/Workman.app'
    }),
    'relaunch'
  );
  assert.equal(
    updateCompletionAction(report({ app: true }), {
      nativeRelaunchAvailable: false,
      appVersion: '0.1.9',
      appBundle: '/Applications/Workman.app'
    }),
    'manual-restart'
  );
});

test('same-version CLI repair restarts only the replaced daemon', () => {
  assert.equal(
    updateCompletionAction(report({ app: false, current: '0.1.9', latest: '0.1.9' }), {
      nativeRelaunchAvailable: true,
      appVersion: '0.1.9',
      appBundle: '/Applications/Workman.app'
    }),
    'restart-daemon-only'
  );
});

test('binary-only version update requires a truthful manual restart', () => {
  assert.equal(
    updateCompletionAction(report({ app: false }), {
      nativeRelaunchAvailable: true,
      appVersion: '0.1.9',
      appBundle: '/Applications/Workman.app'
    }),
    'manual-restart'
  );
});

test('relaunch requires the exact application bundle replaced by the report', () => {
  assert.equal(
    updateCompletionAction(report({ app: true }), {
      nativeRelaunchAvailable: true,
      appVersion: '0.1.9',
      appBundle: '/Users/test/Downloads/Workman.app'
    }),
    'manual-restart'
  );
});

test('legacy reports without restart_plan degrade to a manual completion', () => {
  const legacy = report({ app: false });
  delete legacy.restart_plan;
  assert.equal(
    updateCompletionAction(legacy, {
      nativeRelaunchAvailable: true,
      appVersion: '0.1.9',
      appBundle: '/Applications/Workman.app'
    }),
    'manual-restart'
  );
  const flow = manualUpdateFlow(legacy);
  assert.equal(flow.kind, 'needs-restart');
  assert.equal(flow.title, 'Installed Workman 0.2.0');
  assert.equal(flow.restartAction, null);
});

test('binary-only completion names the replaced tools and says the app was unchanged', () => {
  const binary = report({ app: false });
  binary.updated_files = ['/tmp/versions/0.2.0/bin/wrk', '/tmp/versions/0.2.0/bin/workmand'];
  const flow = manualUpdateFlow(binary);
  assert.match(flow.title, /command-line tools and daemon/);
  assert.match(flow.instruction, /wrk, workmand/);
  assert.match(flow.instruction, /desktop app bundle was not replaced/);
  assert.equal(flow.restartAction, null);
});

test('banner derives byte progress and failure retry from daemon state', () => {
  const downloading = updateBannerState(null, {
    kind: 'running',
    progress: {
      stage: 'downloading',
      message: 'Downloading archive',
      bytes_done: 524_288,
      bytes_total: 1_048_576,
      percent: 50,
      failed: false
    }
  });
  assert.equal(downloading.mode, 'running');
  assert.equal(downloading.title, 'Downloading update…');
  assert.equal(downloading.description, '512.0 KB of 1.0 MB · 50%');
  assert.equal(downloading.percent, 50);

  const failed = updateBannerState(null, {
    kind: 'failed',
    stage: 'verifying',
    message: 'SHA256 mismatch'
  });
  assert.equal(failed.title, 'Verifying update failed');
  assert.equal(failed.retry, true);
  assert.equal(failed.description, 'SHA256 mismatch');
});

test('banner distinguishes automatic restart from manual completion', () => {
  const restarting = updateBannerState(null, { kind: 'restarting', version: '0.2.0', target: 'app' });
  assert.equal(restarting.title, 'Installed Workman 0.2.0 — restarting…');
  assert.equal(restarting.restart, false);

  const manual = updateBannerState(null, {
    kind: 'needs-restart',
    version: '0.2.0',
    title: 'Installed Workman 0.2.0. Restart Workman to finish',
    instruction: 'Open it again.',
    restartAction: 'app'
  });
  assert.equal(manual.title, 'Installed Workman 0.2.0. Restart Workman to finish');
  assert.equal(manual.restart, true);
  assert.equal(manual.dismiss, true);
});
