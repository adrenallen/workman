import assert from 'node:assert/strict';
import test from 'node:test';
import { createCreationDraft } from '../src/lib/creationDrafts.ts';
import { parseExtraArgs } from '../src/lib/extraArgs.ts';
import { agentPromptHistoryKey, boundAgentPromptHistory, loadAgentPromptHistory, maxAgentPromptHistory, saveAgentPromptHistory, snapshotAgentPrompt } from '../src/lib/agentPromptHistory.ts';

const tool = { id: 2, name: 'Agent', command: 'claude --model default --effort low', tool_type: 'claude' };
const template = { id: 3, name: 'Review', agent_tool_id: 2, prompt: 'Review carefully.', extra_args: ['--model', 'template-model', '--effort', 'medium', '--allowedTools', 'Read'] };
const draft = { ...createCreationDraft('agent', 7, -1), templateId: 3, prompt: 'Check the update.', attachments: ['/tmp/draft-image.png'], feedbackId: 9 };
const input = { project_id: 7, prompt: draft.prompt, model: 'override', extra_args: ['--effort', 'high'] };
function snapshot(id = 'entry') { return snapshotAgentPrompt(draft, tool, template, input, id, 1000); }
function memoryStorage() {
  const values = new Map();
  return { getItem: (key) => values.get(key) ?? null, setItem: (key, value) => values.set(key, value) };
}

test('recovery freezes template instructions, launch overrides, feedback, and attachments before a CLI starts', () => {
  const saved = snapshot();
  assert.equal(saved.processId, null);
  assert.equal(saved.draft.prompt, 'Review carefully.\n\nCheck the update.');
  assert.equal(saved.draft.templateId, null, 'recovery does not apply template instructions twice');
  assert.equal(saved.draft.agentToolId, 2);
  assert.equal(saved.draft.model, 'override');
  assert.equal(saved.draft.effort, 'high');
  assert.deepEqual(parseExtraArgs(saved.draft.extraArgs), ['--allowedTools', 'Read']);
  assert.equal(saved.draft.feedbackId, 9);
  saved.draft.attachments.push('/tmp/another.png');
  assert.deepEqual(draft.attachments, ['/tmp/draft-image.png']);
  assert.equal(saved.draft.prompt, snapshotAgentPrompt(draft, tool, { ...template }, input, 'copy').draft.prompt);
});

test('switching a template to another tool keeps the prompt and skips incompatible template args', () => {
  const saved = snapshotAgentPrompt(draft, { ...tool, id: 4, tool_type: 'codex', command: 'codex' }, template, { ...input, model: undefined, extra_args: [] }, 'swapped');
  assert.equal(saved.draft.prompt, 'Review carefully.\n\nCheck the update.');
  assert.equal(saved.draft.extraArgs, '');
  assert.equal(saved.draft.model, '');
  assert.equal(saved.draft.effort, '');
});

test('template-only and standalone prompts remain recoverable', () => {
  assert.equal(snapshotAgentPrompt(draft, tool, template, { ...input, prompt: undefined }, 'template').draft.prompt, template.prompt);
  const saved = snapshotAgentPrompt(draft, tool, null, { ...input, model: undefined, extra_args: [] }, 'standalone');
  assert.equal(saved.draft.prompt, draft.prompt);
  assert.equal(saved.draft.model, 'default');
  assert.equal(saved.draft.effort, 'low');
});

test('history survives reload before or after launch and stays scoped to the profile', () => {
  const storage = memoryStorage();
  const saved = snapshot();
  assert.equal(saveAgentPromptHistory(1, [saved], storage), true);
  assert.deepEqual(loadAgentPromptHistory(1, storage), [saved]);
  assert.deepEqual(loadAgentPromptHistory(2, storage), []);
  saved.processId = 42;
  saveAgentPromptHistory(1, [saved], storage);
  assert.equal(loadAgentPromptHistory(1, storage)[0].processId, 42);
  saveAgentPromptHistory(1, [], storage);
  assert.deepEqual(loadAgentPromptHistory(1, storage), []);
});

test('history is bounded, ignores corrupt entries, and reports storage failure', () => {
  assert.equal(boundAgentPromptHistory(Array.from({ length: 100 }, (_, i) => snapshot(`${i}`))).length, maxAgentPromptHistory);
  const storage = memoryStorage();
  storage.setItem(agentPromptHistoryKey(1), JSON.stringify([null, snapshot(), snapshot(), { ...snapshot('bad'), createdAt: 1e20 }, { ...snapshot('broken'), draft: {} }]));
  assert.deepEqual(loadAgentPromptHistory(1, storage), [snapshot()]);
  storage.setItem(agentPromptHistoryKey(1), '{');
  assert.deepEqual(loadAgentPromptHistory(1, storage), []);
  assert.equal(saveAgentPromptHistory(1, [snapshot()], { setItem() { throw new Error('quota'); } }), false);
});
