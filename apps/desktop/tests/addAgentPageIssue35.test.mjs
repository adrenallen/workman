import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

const panelUrl = new URL('../src/lib/NewAgentDraftPanel.svelte', import.meta.url);
const scaffoldUrl = new URL('../src/lib/CreationDraftScaffold.svelte', import.meta.url);

function sourceBetween(source, start, end, description) {
  const startIndex = source.indexOf(start);
  assert.ok(startIndex >= 0, `${description} start is present`);
  const endIndex = source.indexOf(end, startIndex + start.length);
  assert.ok(endIndex > startIndex, `${description} end is present after its start`);
  return source.slice(startIndex, endIndex);
}

test('add-agent launch choices expose templates and standalone agents directly', async () => {
  const source = await readFile(panelUrl, 'utf8');
  const templateLoopMatch = source.match(/\{#each templateChoices as templateChoice/);
  const templateLoop = templateLoopMatch?.index ?? -1;
  const standaloneLoop = source.indexOf('{#each enabledTools as tool', templateLoop);
  const overrideGate = source.indexOf('{#if selectedTemplate}', standaloneLoop);

  assert.ok(templateLoop >= 0, 'available templates are rendered');
  assert.ok(standaloneLoop > templateLoop, 'standalone agents follow templates in the launch roster');
  assert.ok(overrideGate > standaloneLoop, 'the primary launch roster precedes template-only controls');

  const templateChoices = source.slice(templateLoop, standaloneLoop);
  const standaloneChoices = source.slice(standaloneLoop, overrideGate);
  for (const [label, choices] of [
    ['template', templateChoices],
    ['standalone agent', standaloneChoices]
  ]) {
    assert.match(choices, /<input\b[\s\S]*?\btype="radio"/, `${label} choices use direct semantic controls`);
    assert.doesNotMatch(choices, /<Select\.(?:Root|Trigger|Content|Item)\b/, `${label} choices are not hidden in a select menu`);
  }
  assert.match(templateChoices, /\{template\.name\}/);
  assert.match(templateChoices, /checked=\{selectedTemplate\?\.id === template\.id\}/);
  assert.match(templateChoices, /onclick=\{\(\) => selectTemplate\(template\)\}/);
  assert.match(standaloneChoices, /\{tool\.name\}/);
  assert.match(standaloneChoices, /checked=\{isStandaloneAgentSelected\(choice, tool\.id\)\}/);
  assert.match(standaloneChoices, /onclick=\{\(\) => selectStandaloneAgent\(tool\)\}/);

  const standaloneHandler = sourceBetween(
    source,
    'function selectStandaloneAgent',
    'function selectTemplateAgent',
    'standalone agent selection handler'
  );
  assert.match(standaloneHandler, /onChange\(\{[\s\S]*templateId: null,[\s\S]*agentToolId: tool\.id/);
  assert.match(standaloneHandler, /rememberChoice\(\{ kind: 'tool', id: tool\.id \}\)/);
});

test('a template keeps its default agent and reveals optional override choices', async () => {
  const source = await readFile(panelUrl, 'utf8');
  const selectTemplate = sourceBetween(
    source,
    'function selectTemplate',
    'function submit()',
    'template selection handler'
  );
  assert.match(selectTemplate, /agentTemplateSelectionChange\(selectedTemplate, template\)/);
  assert.match(selectTemplate, /if \(!selection\) return/);
  assert.match(
    selectTemplate,
    /onChange\(\{[\s\S]*templateId: selection\.id,[\s\S]*agentToolId: selection\.agentToolId/
  );

  const standaloneLoop = source.indexOf('{#each enabledTools as tool');
  const overrideGate = source.indexOf('{#if selectedTemplate}', standaloneLoop);
  const overrideLoop = source.indexOf('{#each enabledTools as tool', overrideGate);
  assert.ok(overrideGate > standaloneLoop, 'override controls are gated by a selected template');
  assert.ok(overrideLoop > overrideGate, 'enabled agents are offered as template overrides');

  const instructionSurface = source.indexOf('bind:this={promptField}', overrideLoop);
  assert.ok(instructionSurface > overrideLoop, 'override controls precede the instruction surface');
  const overrideChoices = source.slice(overrideGate, instructionSurface);
  assert.match(overrideChoices, /override/i);
  assert.match(overrideChoices, /bind:open={templateAgentOpen}/);
  assert.match(source, /let templateAgentOpen = \$state\(false\)/);
  assert.match(source, /let templateInstructionsOpen = \$state\(false\)/);
  assert.match(overrideChoices, /<input\b[\s\S]*?\btype="radio"/);
  assert.match(overrideChoices, /\{tool\.name\}/);
  assert.match(overrideChoices, /checked=\{selectedTool\?\.id === tool\.id\}/);
  assert.match(overrideChoices, /onclick=\{\(\) => selectTemplateAgent\(tool\)\}/);

  const overrideHandler = sourceBetween(
    source,
    'function selectTemplateAgent',
    'function submit()',
    'template override selection handler'
  );
  assert.match(overrideHandler, /if \(!selectedTemplate\) return/);
  assert.match(overrideHandler, /onChange\(\{[\s\S]*agentToolId: tool\.id/);
  assert.match(
    overrideHandler,
    /rememberChoice\(\{ kind: 'template', id: selectedTemplate\.id, agentToolId: tool\.id \}\)/
  );
});

test('the additional-instructions surface owns Create and remains optional', async () => {
  const [source, scaffold] = await Promise.all([
    readFile(panelUrl, 'utf8'),
    readFile(scaffoldUrl, 'utf8')
  ]);
  const instructionSurface = sourceBetween(
    source,
    'bind:this={promptField}',
    '<Collapsible.Root bind:open={advancedOpen}',
    'additional instructions surface'
  );
  assert.match(instructionSurface, /selectedTemplate \? 'Additional instructions' : 'Instructions'/);
  assert.match(instructionSurface, /Sent as a separate message after \$\{selectedTemplate\.name\} finishes its setup\./);
  assert.match(instructionSurface, /<Textarea\b/);
  const createButton = instructionSurface.match(/<Button\b([\s\S]*?)>[\s\S]*?Creat(?:e|ing)/);
  assert.ok(createButton, 'Create is rendered beside the prompt controls');
  assert.match(createButton[0], /(?:type="submit"|onclick=\{submit\})/);
  assert.doesNotMatch(createButton[1], /draft\.prompt|prompt\.trim/);

  const scaffoldInvocation = sourceBetween(
    source,
    '<CreationDraftScaffold',
    '>',
    'creation draft scaffold invocation'
  );
  const disabledPrimaryProp = scaffoldInvocation.match(/\b([\w-]*(?:primary|create)[\w-]*)=\{false\}/i);
  assert.ok(disabledPrimaryProp, 'the detached scaffold Create action is explicitly disabled');
  assert.match(
    scaffold,
    new RegExp(`\\{#if ${disabledPrimaryProp[1]}\\}[\\s\\S]*?<Button type="submit"`),
    'the scaffold only renders its primary submit action when enabled'
  );

  const canCreate = sourceBetween(source, 'const canCreate = $derived(', '$effect', 'create eligibility');
  assert.match(canCreate, /selectedTool !== null/);
  assert.doesNotMatch(canCreate, /draft\.prompt|prompt\.trim/);

  const submit = sourceBetween(source, 'function submit()', 'function handleKeydown', 'submit handler');
  assert.doesNotMatch(submit, /if\s*\([^)]*(?:draft\.)?prompt/);
  assert.match(submit, /prompt: draft\.prompt\.trim\(\) \|\| undefined/);
  assert.match(submit, /void onCreate\(/);
});

test('other creation drafts retain their scaffold footer action', async () => {
  const [command, todo] = await Promise.all([
    readFile(new URL('../src/lib/NewCommandDraftPanel.svelte', import.meta.url), 'utf8'),
    readFile(new URL('../src/lib/NewTodoDraftPanel.svelte', import.meta.url), 'utf8')
  ]);

  for (const [label, source] of [['command', command], ['todo', todo]]) {
    assert.match(source, /<CreationDraftScaffold\b/, `${label} uses the shared creation scaffold`);
    assert.doesNotMatch(source, /showFooterCreate=\{false\}/, `${label} keeps the default footer Create`);
  }
});
