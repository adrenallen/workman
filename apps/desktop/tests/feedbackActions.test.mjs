import assert from 'node:assert/strict';
import test from 'node:test';
import { get } from 'svelte/store';
import { agentCanReceiveFeedback, agentFeedbackAvailability } from '../src/lib/recordedFeedback.ts';
import { parseRecordedFeedbackPreferences, recordedFeedbackPreferences, recordedFeedbackPreferencesStorageKey, setRecordedFeedbackAutoArchive } from '../src/lib/recordedFeedbackAvailability.ts';

test('auto-archive defaults on and preserves old preferences and an explicit opt-out', () => {
  assert.equal(parseRecordedFeedbackPreferences(null).autoArchiveAfterSend, true);
  const migrated = parseRecordedFeedbackPreferences({ showInSidebar: false, agentPrompt: 'Keep my wrapper' });
  assert.deepEqual(migrated, { showInSidebar: false, agentPrompt: 'Keep my wrapper', autoArchiveAfterSend: true });
  assert.equal(parseRecordedFeedbackPreferences({ ...migrated, autoArchiveAfterSend: false }).autoArchiveAfterSend, false);
  assert.equal(parseRecordedFeedbackPreferences({ autoArchiveAfterSend: 'false' }).autoArchiveAfterSend, true);
});

test('the auto-archive switch persists without resetting other feedback preferences', () => {
  const saved = new Map();
  const previous = globalThis.localStorage;
  const preferences = get(recordedFeedbackPreferences);
  globalThis.localStorage = { setItem: (key, value) => saved.set(key, value) };
  try {
    recordedFeedbackPreferences.set({ ...preferences, showInSidebar: false, agentPrompt: 'Custom wrapper' });
    setRecordedFeedbackAutoArchive(false);
    assert.deepEqual(parseRecordedFeedbackPreferences(JSON.parse(saved.get(recordedFeedbackPreferencesStorageKey))), {
      showInSidebar: false, agentPrompt: 'Custom wrapper', autoArchiveAfterSend: false
    });
    setRecordedFeedbackAutoArchive(true);
    assert.equal(get(recordedFeedbackPreferences).autoArchiveAfterSend, true);
  } finally {
    recordedFeedbackPreferences.set(preferences);
    if (previous === undefined) delete globalThis.localStorage;
    else globalThis.localStorage = previous;
  }
});

test('agent targets expose lifecycle status and only accept feedback when ready', () => {
  for (const [status, state, label, ready] of [
    ['running', 'idle', 'Ready', true], ['running', 'needs_input', 'Needs input', true],
    ['running', 'waiting', 'Waiting', true], ['running', 'working', 'Working', false],
    ['starting', 'working', 'Starting', false], ['stopped', 'idle', 'Stopped', false],
    ['exited', 'idle', 'Exited', false], ['crashed', 'working', 'Crashed', false],
    ['running', 'exited', 'Exited', false]
  ]) {
    const agent = { kind: 'agent', status, agent_state: { state } };
    assert.equal(agentFeedbackAvailability(agent), label);
    assert.equal(agentCanReceiveFeedback(agent), ready);
  }
});
