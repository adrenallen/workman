import assert from 'node:assert/strict';
import test from 'node:test';

import {
  POPOVER_HOVER_GRACE_MS,
  POPOVER_HOVER_INTENT_MS,
  PopoverHoverIntent
} from '../src/lib/popoverHoverIntent.ts';

function fakeScheduler() {
  let nextId = 0;
  const scheduled = new Map();
  return {
    scheduler: {
      set(callback, delayMs) {
        const id = ++nextId;
        scheduled.set(id, { callback, delayMs });
        return id;
      },
      clear(id) {
        scheduled.delete(id);
      }
    },
    scheduled,
    run(delayMs) {
      const entries = [...scheduled.entries()].filter(([, task]) => task.delayMs === delayMs);
      for (const [id, task] of entries) {
        scheduled.delete(id);
        task.callback();
      }
    }
  };
}

test('hover intent waits 200ms and cancels when the pointer leaves early', () => {
  const fake = fakeScheduler();
  const intent = new PopoverHoverIntent(fake.scheduler);
  let opens = 0;

  intent.enterTrigger(() => opens += 1);
  assert.equal([...fake.scheduled.values()][0].delayMs, POPOVER_HOVER_INTENT_MS);
  intent.leaveTrigger(() => {});
  fake.run(POPOVER_HOVER_INTENT_MS);
  assert.equal(opens, 0);
});

test('hover grace keeps the popover open across the trigger-content gap', () => {
  const fake = fakeScheduler();
  const intent = new PopoverHoverIntent(fake.scheduler);
  let closes = 0;

  intent.leaveTrigger(() => closes += 1);
  assert.equal([...fake.scheduled.values()][0].delayMs, POPOVER_HOVER_GRACE_MS);
  intent.enterContent();
  fake.run(POPOVER_HOVER_GRACE_MS);
  assert.equal(closes, 0);

  intent.leaveContent(() => closes += 1);
  fake.run(POPOVER_HOVER_GRACE_MS);
  assert.equal(closes, 1);
});

test('cancel clears both pending intent and grace callbacks', () => {
  const fake = fakeScheduler();
  const intent = new PopoverHoverIntent(fake.scheduler);

  intent.enterTrigger(() => assert.fail('open callback should be cancelled'));
  intent.leaveContent(() => assert.fail('close callback should be cancelled'));
  intent.cancel();
  assert.equal(fake.scheduled.size, 0);
});
