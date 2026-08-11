import assert from 'node:assert/strict';
import test from 'node:test';

import { updateActionAvailable, updateActionCopy } from '../src/lib/updateRecovery.ts';

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
  assert.match(copy.dialogDescription, /durable versioned layout/);
  assert.match(copy.dialogDescription, /repair the launchers in ~\/.local\/bin/);
  assert.match(copy.dialogDescription, /update the desktop app/);
});

test('the current desktop release can repair the CLI without a newer update', () => {
  const update = status({ available: false, recovery: true });
  const copy = updateActionCopy(update);

  assert.equal(updateActionAvailable(update), true);
  assert.equal(copy.buttonLabel, 'Repair command-line tools');
  assert.equal(copy.confirmLabel, 'Repair CLI');
  assert.match(copy.dialogDescription, /download and verify 0\.1\.6/);
});

test('a healthy current install shows no recovery action or recovery copy', () => {
  const update = status({ available: false, recovery: false });
  const copy = updateActionCopy(update);

  assert.equal(updateActionAvailable(update), false);
  assert.equal(copy.buttonLabel, 'Update now');
  assert.doesNotMatch(copy.dialogTitle, /repair/i);
  assert.doesNotMatch(copy.dialogDescription, /missing|repair/i);
});
