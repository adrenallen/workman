<script lang="ts">
  import NewAgentDraftPanel from '../../src/lib/NewAgentDraftPanel.svelte';
  import { createCreationDraft, type AgentCreationDraft } from '../../src/lib/creationDrafts';
  import type { AgentTool } from '../../src/lib/agentTools';
  import type { AgentTemplate } from '../../src/lib/agentTemplates';

  const tools: AgentTool[] = ['Codex', 'Claude', 'Kimi', 'DeepSeek V4 flash', 'OpenCode', 'Gemini'].map((name, i) => ({
    id: i + 1, name, command: name.toLowerCase(), tool_type: ['codex', 'claude', 'kimi', 'deepseek', 'opencode', 'gemini'][i],
    enabled: true, source: 'local', resume_args: null, continue_args: null, icon_data_url: null
  }));
  const templates: AgentTemplate[] = [
    { id: 1, name: 'Orchestrator Agent', agent_tool_id: 2, prompt: 'You are an orchestrator agent working in a Workman development environment. Plan the work, coordinate focused tasks, and review the result before reporting back.', extra_args: ['--model', 'opus'], profile_id: 1, sort_order: 0, created_at: 0, updated_at: 0 },
    { id: 2, name: 'General Agent', agent_tool_id: 1, prompt: 'Investigate the task, implement a focused solution, and verify the behavior.', extra_args: [], profile_id: 1, sort_order: 1, created_at: 0, updated_at: 0 }
  ];
  const initial = (): AgentCreationDraft => ({ ...createCreationDraft('agent', 1, 1001) as AgentCreationDraft, templateId: 1, agentToolId: 2 });
  let draft = $state(initial());
  let busy = $state(false);
  let loading = $state(false);
  let noTemplates = $state(false);
  let noTools = $state(false);
  let submission = $state('');
  let history = $state([{ id: 'saved-1', createdAt: Date.now(), processId: null, label: 'Review the release', draft: { ...initial(), templateId: null, agentToolId: 1, prompt: 'Review the release and summarize the changes.', model: 'gpt-6' } }]);
  let error = $state('');
  let focusOnMount = $state(true);
  function change(patch: Partial<AgentCreationDraft>) { draft = { ...draft, ...patch }; }
</script>

<div class="fixture-controls">
  <button onclick={() => { busy = !busy; }}>Toggle busy</button>
  <button onclick={() => { loading = !loading; }}>Toggle loading</button>
  <button onclick={() => { noTemplates = !noTemplates; }}>Toggle templates</button>
  <button onclick={() => { noTools = !noTools; }}>Toggle tools</button>
  <button onclick={() => { draft = initial(); }}>Reset draft</button>
  <button onclick={() => change({ prompt: draft.prompt + ' [Image #1]', attachments: ['/tmp/agent-fixture.png'] })}>Attach fixture</button>
</div>
<div class="fixture-pane">
  <NewAgentDraftPanel {draft} tools={noTools ? [] : tools} templates={noTemplates ? [] : templates} {busy} {loading} metadataLoaded={!loading}
    {focusOnMount} onInitialFocusHandled={() => { focusOnMount = false; }}
    projectName="Workman" onChange={change} onInitialize={change} onCreate={value => { submission = JSON.stringify(value.input); }}
    onDiscard={() => { submission = 'discarded'; }} onOpenSettings={() => { submission = 'settings'; }} onError={value => { error = value; }}
    promptHistory={history} onRestorePrompt={entry => { draft = { ...entry.draft }; }} onClearPromptHistory={() => { history = []; }} />
</div>
<output data-testid="submission" hidden>{submission}</output>
<output data-testid="draft" hidden>{JSON.stringify(draft)}</output>
<output data-testid="error" hidden>{error}</output>

<style>
  :global(html), :global(body), :global(#app) { height: 100%; margin: 0; }
  .fixture-controls { display: flex; gap: 16px; padding: 8px; height: 36px; font-size: 12px; }
  .fixture-pane { height: calc(100% - 36px); width: 100%; }
</style>
