import assert from 'node:assert/strict';
import test from 'node:test';

import {
  agentTemplateSelectionChange,
  agentTemplateRosterChoices,
  isStandaloneAgentSelected,
  resolveAgentDraftChoice
} from '../src/lib/agentDraftChoices.ts';

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
  assert.equal(isStandaloneAgentSelected(choice, 2), false);
});

test('standalone selection stays actionable while recovering a stale template', () => {
  const standalone = { ...draft, templateId: null };
  const selected = resolveAgentDraftChoice(standalone, tools, templates, true, 'tool:1');
  const stale = resolveAgentDraftChoice(draft, tools, [], true, 'tool:1');

  assert.equal(isStandaloneAgentSelected(selected, 2), true);
  assert.equal(isStandaloneAgentSelected(selected, 1), false);
  assert.equal(isStandaloneAgentSelected(stale, 2), false);
});

test('template roster omits dangling tools without hiding disabled choices', () => {
  const choices = agentTemplateRosterChoices(
    [
      ...templates,
      { id: 6, name: 'Disabled template', agent_tool_id: 3 },
      { id: 7, name: 'Dangling template', agent_tool_id: 99 }
    ],
    [...tools, { id: 3, name: 'Disabled agent', enabled: false }]
  );

  assert.deepEqual(choices.map(({ template, tool }) => [template.id, tool.id]), [
    [5, 2],
    [6, 3]
  ]);
});

test('reselecting the active template preserves its tool override without touching the draft', () => {
  const activeTemplate = templates[0];
  const otherTemplate = { ...activeTemplate, id: 6, agent_tool_id: 1 };

  assert.equal(agentTemplateSelectionChange(activeTemplate, activeTemplate), null);
  assert.deepEqual(agentTemplateSelectionChange(activeTemplate, otherTemplate), {
    kind: 'template',
    id: 6,
    agentToolId: 1
  });
});
