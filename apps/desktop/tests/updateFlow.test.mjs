import assert from 'node:assert/strict';
import test from 'node:test';

import {
  updateBannerState,
  updateCompletionAction
} from '../src/lib/updateFlow.ts';

function report({ app, current = '0.1.9', latest = '0.2.0' }) {
  return {
    current,
    latest,
    install_dir: '/tmp/workman',
    updated_files: [],
    desktop_instruction: null,
    quarantine_cleared: false,
    restart_plan: { daemon: true, app }
  };
}

test('completion action relaunches only when the refreshed app can relaunch', () => {
  assert.equal(
    updateCompletionAction(report({ app: true }), {
      nativeRelaunchAvailable: true,
      appVersion: '0.1.9'
    }),
    'relaunch'
  );
  assert.equal(
    updateCompletionAction(report({ app: true }), {
      nativeRelaunchAvailable: false,
      appVersion: '0.1.9'
    }),
    'manual-restart'
  );
});

test('same-version CLI repair restarts only the replaced daemon', () => {
  assert.equal(
    updateCompletionAction(report({ app: false, current: '0.1.9', latest: '0.1.9' }), {
      nativeRelaunchAvailable: true,
      appVersion: '0.1.9'
    }),
    'restart-daemon-only'
  );
});

test('binary-only version update requires a truthful manual restart', () => {
  assert.equal(
    updateCompletionAction(report({ app: false }), {
      nativeRelaunchAvailable: true,
      appVersion: '0.1.9'
    }),
    'manual-restart'
  );
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
  const restarting = updateBannerState(null, { kind: 'restarting', version: '0.2.0' });
  assert.equal(restarting.title, 'Installed Workman 0.2.0 — restarting…');
  assert.equal(restarting.restart, false);

  const manual = updateBannerState(null, {
    kind: 'needs-restart',
    version: '0.2.0',
    instruction: null
  });
  assert.equal(manual.title, 'Installed Workman 0.2.0. Restart Workman to finish');
  assert.equal(manual.restart, true);
  assert.equal(manual.dismiss, true);
});
