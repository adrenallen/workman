import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

import {
  choiceValue,
  chooseInitialAgentChoice,
  lastAgentChoiceStorageKey,
  parseChoiceValue
} from '../src/lib/agentTemplates.ts';
import { formatExtraArgs, parseExtraArgs } from '../src/lib/extraArgs.ts';

const tools = [
  { id: 7, name: 'Codex', enabled: true },
  { id: 8, name: 'Claude', enabled: false },
  { id: 9, name: 'Grok', enabled: true }
];
const templates = [
  { id: 12, name: 'Implement', agent_tool_id: 7 },
  { id: 13, name: 'Disabled tool template', agent_tool_id: 8 }
];

test('extra argument formatting round-trips whitespace and quoted characters', () => {
  const args = [
    '--model',
    'fast model',
    'line one\nline two',
    'column\tvalue',
    'literal\\nsequence',
    'say "hello"',
    "it's ready",
    ''
  ];
  assert.deepEqual(parseExtraArgs(formatExtraArgs(args)), args);
});

test('last agent choice is validated and otherwise falls back to the first enabled tool', () => {
  assert.deepEqual(chooseInitialAgentChoice(templates, tools, 'template:12'), {
    kind: 'template', id: 12, agentToolId: 7
  });
  assert.deepEqual(chooseInitialAgentChoice(templates, tools, 'template:12:tool:7'), {
    kind: 'template', id: 12, agentToolId: 7
  });
  assert.deepEqual(chooseInitialAgentChoice(templates, tools, 'template:12:tool:9'), {
    kind: 'template', id: 12, agentToolId: 9
  });
  assert.deepEqual(chooseInitialAgentChoice(templates, tools, 'template:12:tool:8'), {
    kind: 'template', id: 12, agentToolId: 7
  });
  assert.deepEqual(chooseInitialAgentChoice(templates, tools, 'tool:7'), {
    kind: 'tool', id: 7
  });
  assert.deepEqual(chooseInitialAgentChoice(templates, tools, 'template:13'), {
    kind: 'tool', id: 7
  });
  assert.deepEqual(chooseInitialAgentChoice(templates, tools, null), {
    kind: 'tool', id: 7
  });
  assert.equal(choiceValue({ kind: 'template', id: 12 }), 'template:12');
  assert.equal(
    choiceValue({ kind: 'template', id: 12, agentToolId: 7 }),
    'template:12:tool:7'
  );
  assert.deepEqual(parseChoiceValue('tool:7'), { kind: 'tool', id: 7 });
  assert.deepEqual(parseChoiceValue('template:12:tool:7'), {
    kind: 'template', id: 12, agentToolId: 7
  });
  assert.equal(parseChoiceValue('profile:7'), null);
  assert.equal(lastAgentChoiceStorageKey, 'workman.new-agent.last-choice.v1');
});

test('new-agent draft keeps template and agent roster choices independent and persistent', async () => {
  const [source, card, daemon] = await Promise.all([
    readFile(new URL('../src/lib/NewAgentDraftPanel.svelte', import.meta.url), 'utf8'),
    readFile(new URL('../src/lib/settings/AgentTemplatesCard.svelte', import.meta.url), 'utf8'),
    readFile(new URL('../src/lib/daemon.ts', import.meta.url), 'utf8')
  ]);
  assert.match(source, /<strong>Templates<\/strong>/);
  assert.match(source, /<strong>Models &amp; tools<\/strong>/);
  assert.match(source, /name={`draft-agent-launch-\$\{draft\.id\}`}/);
  assert.match(source, /type="radio"/);
  assert.doesNotMatch(source, /<Select\.(?:Root|Trigger|Content|Item)\b/);
  assert.match(source, /agent_template_id: selectedTemplate\.id, agent_tool_id: selectedTool\.id/);
  assert.match(source, /prompt: draft\.prompt\.trim\(\) \|\| undefined/);
  assert.match(source, /attachments: draft\.attachments\.length > 0/);
  assert.match(source, /onDragDropEvent\(\(event\) => handleNativePromptDrop\(event\.payload\)\)/);
  assert.match(source, /function selectTemplate\(template: AgentTemplate\)[\s\S]*templateId: template\.id, agentToolId: template\.agent_tool_id/);
  assert.match(source, /function selectStandaloneAgent\(tool: AgentTool\)[\s\S]*templateId: null, agentToolId: tool\.id/);
  assert.match(source, /function selectTemplateAgent\(tool: AgentTool\)[\s\S]*onChange\(\{ agentToolId: tool\.id \}\)/);
  assert.match(source, /Template launch args are skipped when using/);
  assert.match(source, /Prepended to your optional prompt/);
  assert.match(source, /primaryModifier\(event\)/);
  assert.match(source, /\{#each templates as template/);
  assert.match(source, /disabled=\{!tool\.enabled\}/);
  assert.match(source, /agent disabled/);
  assert.match(source, /No enabled agents\. Add or enable one in Settings\./);
  assert.doesNotMatch(source, /No enabled agent tools/);
  assert.match(source, /class="prompt-textarea min-h-\[14rem\] resize-y text-sm leading-6"/);
  assert.doesNotMatch(source, /rows=\{11\}/);
  assert.match(source, /bind:ref=\{promptTextarea\}/);
  assert.match(source, /if \(!focusOnMount\) return;[\s\S]*promptTextarea\?\.focus\(\)/);
  assert.match(source, /<span>Prompt <small>optional<\/small><\/span>/);
  assert.match(source, /showFooterCreate=\{false\}/);
  assert.match(source, /&& !choice\.missingTemplate[\s\S]*&& !choice\.missingTool/);
  assert.match(card, /find\(\(candidate\) => candidate\.enabled\)/);
  assert.doesNotMatch(card, /candidate\.enabled\) \?\? toolSnapshot\.tools\[0\]/);
  assert.match(card, /Agent disabled/);
  assert.match(card, /Pair a default agent with launch arguments and a reusable prompt\./);
  assert.match(card, />Default agent/);
  assert.match(card, /Select a default agent/);
  assert.match(card, /Missing default agent/);
  assert.match(card, /Add or enable an agent before creating an agent template/);
  assert.doesNotMatch(card, /agent tools?/i);
  assert.match(daemon, /listAgentTemplates\(\)[\s\S]*requestOptional\('agent_templates\.list', \{\}, \[\]\)/);
});

test('desktop spawn entry surfaces route through the inline draft panel', async () => {
  const app = await readFile(new URL('../src/App.svelte', import.meta.url), 'utf8');
  const draftPanel = await readFile(
    new URL('../src/lib/NewAgentDraftPanel.svelte', import.meta.url),
    'utf8'
  );
  assert.match(app, /<NewAgentDraftPanel/);
  assert.match(app, /await openAgentDraft\(tool\.id\)/);
  assert.match(app, /onCreate=\{\(submission\) => createAgentFromDraft\(draft, submission\)\}/);
  assert.match(draftPanel, /Prepended to your optional prompt/);
  assert.match(draftPanel, /if \(!focusOnMount\) return;[\s\S]*promptTextarea\?\.focus\(\)/);
  assert.match(draftPanel, /Template #\{draft\.templateId\} is no longer available/);
  assert.match(draftPanel, /showFooterCreate=\{false\}/);
  assert.doesNotMatch(app, /NewAgentDialog|AgentsPanel/);
});
