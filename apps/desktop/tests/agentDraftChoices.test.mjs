import assert from 'node:assert/strict';
import test from 'node:test';

import { resolveAgentDraftChoice } from '../src/lib/agentDraftChoices.ts';

const draft = {
  id: -1,
  projectId: 217,
  kind: 'agent',
  createdAt: 1,
  touched: true,
  agentToolId: 2,
  templateId: 5,
  name: 'Persisted agent',
  prompt: 'Keep this choice',
  extraArgs: ''
};
const tools = [
  { id: 1, name: 'Codex', enabled: true },
  { id: 2, name: 'Claude Code', enabled: true }
];
const templates = [
  { id: 5, name: 'Review template', agent_tool_id: 2 }
];

test('restore with empty metadata keeps the persisted template and agent pending', () => {
  const restored = structuredClone(draft);
  const choice = resolveAgentDraftChoice(restored, [], [], false, 'tool:1');

  assert.equal(choice.selectedTool, null);
  assert.equal(choice.selectedTemplate, null);
  assert.equal(choice.missingTool, false);
  assert.equal(choice.missingTemplate, false);
  assert.equal(choice.initialChoice, null);
  assert.equal(restored.agentToolId, 2);
  assert.equal(restored.templateId, 5);
});

test('metadata arriving later resolves the persisted template and agent', () => {
  const choice = resolveAgentDraftChoice(draft, tools, templates, true, 'tool:1');

  assert.equal(choice.selectedTool?.id, 2);
  assert.equal(choice.selectedTemplate?.id, 5);
  assert.equal(choice.missingTool, false);
  assert.equal(choice.missingTemplate, false);
  assert.equal(choice.initialChoice, null);
});

test('confirmed-missing template is reported without a silent replacement', () => {
  const restored = structuredClone(draft);
  const choice = resolveAgentDraftChoice(restored, tools, [], true, 'tool:1');

  assert.equal(choice.selectedTool?.id, 2);
  assert.equal(choice.selectedTemplate, null);
  assert.equal(choice.missingTemplate, true);
  assert.equal(choice.initialChoice, null);
  assert.equal(restored.templateId, 5);
  assert.equal(restored.agentToolId, 2);
});
