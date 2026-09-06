import assert from 'node:assert/strict';
import test from 'node:test';

import { updateActionAvailable, updateActionCopy, updateInstallBlocked } from '../src/lib/updateRecovery.ts';

function status({ available, recovery }) {
  return {
    cli_recovery_required: recovery,
    check: {
      available,
      current: '0.1.6',
      latest: available ? '0.2.0' : '0.1.6'
    }
  };
}

test('a missing CLI becomes a plain one-click recovery offer that continues the update', () => {
  const update = status({ available: true, recovery: true });
  const copy = updateActionCopy(update);

  assert.equal(updateActionAvailable(update), true);
  assert.equal(copy.buttonLabel, 'Repair CLI and update');
  assert.equal(copy.confirmLabel, 'Repair and update');
  assert.match(copy.dialogDescription, /wrk and workmand launchers are missing/);
  assert.match(copy.dialogDescription, /repair the launchers in ~\/.local\/bin/);
  assert.match(copy.dialogDescription, /update the desktop app/);
  assert.match(copy.dialogDescription, /restart the app and daemon automatically/);
});

test('the current desktop release can repair the CLI without a newer update', () => {
  const update = status({ available: false, recovery: true });
  const copy = updateActionCopy(update);

  assert.equal(updateActionAvailable(update), true);
  assert.equal(copy.buttonLabel, 'Repair command-line tools');
  assert.equal(copy.confirmLabel, 'Repair CLI');
  assert.match(copy.dialogDescription, /download, verify, and install 0\.1\.6/);
  assert.match(copy.dialogDescription, /restart the app and daemon automatically/);
});

test('a healthy current install shows no recovery action or recovery copy', () => {
  const update = status({ available: false, recovery: false });
  const copy = updateActionCopy(update);

  assert.equal(updateActionAvailable(update), false);
  assert.equal(copy.buttonLabel, 'Update now');
  assert.doesNotMatch(copy.dialogTitle, /repair/i);
  assert.doesNotMatch(copy.dialogDescription, /missing|repair/i);
  assert.match(copy.dialogDescription, /restart the app and daemon automatically/);
});

test('a release without a package for this platform is download-only, not an installable update', () => {
  const blocked = {
    ...status({ available: true, recovery: false }),
    check: {
      available: true,
      current: '0.1.6',
      latest: '0.2.0',
      install_blocked: 'Workman 0.2.0 has no Windows x86_64 package yet. Download it from https://example.com/v0.2.0 once one is published.'
    }
  };
  assert.equal(updateInstallBlocked(blocked), blocked.check.install_blocked);
  assert.equal(updateActionAvailable(blocked), false);
  const copy = updateActionCopy(blocked);
  assert.equal(copy.bannerTitle, 'Workman 0.2.0 is available to download');
  assert.equal(copy.bannerDescription, blocked.check.install_blocked);

  // A CLI repair would install the same missing package, so it is not offered either.
  const repairable = { ...blocked, cli_recovery_required: true };
  assert.equal(updateActionAvailable(repairable), false);
  assert.equal(updateActionCopy(repairable).bannerTitle, 'Workman 0.2.0 is available to download');
  assert.match(updateActionCopy(repairable).bannerDescription, /launchers are also missing/);

  // The note only matters while the release is actually newer.
  const current = { ...blocked, check: { ...blocked.check, available: false, latest: '0.1.6' } };
  assert.equal(updateInstallBlocked(current), null);
  assert.equal(updateActionAvailable(current), false);
});
