<script lang="ts">
  import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
  import SlidersHorizontalIcon from '@lucide/svelte/icons/sliders-horizontal';
  import { onMount } from 'svelte';

  import AgentBrandMark from './AgentBrandMark.svelte';
  import CreationDraftScaffold from './CreationDraftScaffold.svelte';
  import { parseExtraArgs, type AgentTool, type SpawnAgentInput } from './agentTools';
  import {
    choiceValue,
    chooseInitialAgentChoice,
    lastAgentChoiceStorageKey,
    type AgentChoice,
    type AgentTemplate
  } from './agentTemplates';
  import type { AgentCreationDraft } from './creationDrafts';
  import { Button } from './components/ui/button';
  import * as Collapsible from './components/ui/collapsible';
  import { Input } from './components/ui/input';
  import * as Select from './components/ui/select';
  import { Textarea } from './components/ui/textarea';

  interface AgentDraftSubmission {
    input: SpawnAgentInput;
    tool: AgentTool;
    template: AgentTemplate | null;
  }

  interface Props {
    draft: AgentCreationDraft;
    projectName: string;
    tools: AgentTool[];
    templates: AgentTemplate[];
    loading?: boolean;
    busy?: boolean;
    onChange: (patch: Partial<AgentCreationDraft>) => void;
    onInitialize: (patch: Partial<AgentCreationDraft>) => void;
    onCreate: (submission: AgentDraftSubmission) => void | Promise<void>;
    onDiscard: () => void;
    onOpenSettings?: () => void;
    onError?: (message: string) => void;
  }

  let {
    draft,
    projectName,
    tools,
    templates,
    loading = false,
    busy = false,
    onChange,
    onInitialize,
    onCreate,
    onDiscard,
    onOpenSettings,
    onError = () => undefined
  }: Props = $props();

  let advancedOpen = $state(false);
  let previewOpen = $state(true);
  let promptTextarea = $state<HTMLTextAreaElement | null>(null);

  const enabledTools = $derived(tools.filter((tool) => tool.enabled));
  const availableTemplates = $derived(
    templates.filter((template) =>
      enabledTools.some((tool) => tool.id === template.agent_tool_id)
    )
  );
  const selectedTemplate = $derived(
    availableTemplates.find((template) => template.id === draft.templateId) ?? null
  );
  const selectedTool = $derived(
    enabledTools.find((tool) => tool.id === draft.agentToolId) ?? null
  );
  const templateDefaultTool = $derived(
    selectedTemplate
      ? tools.find((tool) => tool.id === selectedTemplate.agent_tool_id) ?? null
      : null
  );
  const agentOverridden = $derived(
    selectedTemplate !== null
      && selectedTool !== null
      && selectedTool.id !== selectedTemplate.agent_tool_id
  );
  const templateChoice = $derived(
    selectedTemplate ? `template:${selectedTemplate.id}` : 'none'
  );
  const agentChoice = $derived(selectedTool ? `tool:${selectedTool.id}` : '');

  $effect(() => {
    if (selectedTool && (draft.templateId === null || selectedTemplate)) return;
    const preferred = draft.templateId !== null && draft.agentToolId !== null
      ? `template:${draft.templateId}:tool:${draft.agentToolId}`
      : draft.agentToolId !== null
        ? `tool:${draft.agentToolId}`
        : readLastChoice();
    const initial = chooseInitialAgentChoice(availableTemplates, enabledTools, preferred);
    if (!initial) {
      if (draft.agentToolId !== null || draft.templateId !== null) {
        onInitialize({ agentToolId: null, templateId: null });
      }
      return;
    }
    const templateId = initial.kind === 'template' ? initial.id : null;
    const agentToolId = initial.kind === 'template'
      ? initial.agentToolId ?? availableTemplates.find((template) => template.id === initial.id)?.agent_tool_id ?? null
      : initial.id;
    if (draft.templateId !== templateId || draft.agentToolId !== agentToolId) {
      onInitialize({ templateId, agentToolId });
    }
  });

  onMount(() => {
    requestAnimationFrame(() => promptTextarea?.focus());
  });

  function readLastChoice(): string | null {
    try {
      return localStorage.getItem(lastAgentChoiceStorageKey);
    } catch {
      return null;
    }
  }

  function rememberChoice(choice: AgentChoice): void {
    try {
      localStorage.setItem(lastAgentChoiceStorageKey, choiceValue(choice));
    } catch {
      // Draft editing remains available if webview storage is unavailable.
    }
  }

  function selectTemplate(value: string | undefined): void {
    const template = availableTemplates.find((candidate) => value === `template:${candidate.id}`);
    if (!template) {
      onChange({ templateId: null });
      if (selectedTool) rememberChoice({ kind: 'tool', id: selectedTool.id });
      return;
    }
    onChange({ templateId: template.id, agentToolId: template.agent_tool_id });
    rememberChoice({ kind: 'template', id: template.id, agentToolId: template.agent_tool_id });
  }

  function selectAgent(value: string | undefined): void {
    const tool = enabledTools.find((candidate) => value === `tool:${candidate.id}`);
    if (!tool) return;
    onChange({ agentToolId: tool.id });
    rememberChoice(selectedTemplate
      ? { kind: 'template', id: selectedTemplate.id, agentToolId: tool.id }
      : { kind: 'tool', id: tool.id });
  }

  function submit(): void {
    if (!selectedTool || busy) return;
    let extraArgs: string[];
    try {
      extraArgs = parseExtraArgs(draft.extraArgs);
    } catch (cause) {
      onError(cause instanceof Error ? cause.message : String(cause));
      return;
    }
    rememberChoice(selectedTemplate
      ? { kind: 'template', id: selectedTemplate.id, agentToolId: selectedTool.id }
      : { kind: 'tool', id: selectedTool.id });
    void onCreate({
      input: {
        project_id: draft.projectId,
        ...(selectedTemplate
          ? { agent_template_id: selectedTemplate.id, agent_tool_id: selectedTool.id }
          : { agent_tool_id: selectedTool.id }),
        name: draft.name.trim() || undefined,
        extra_args: extraArgs,
        prompt: draft.prompt.trim() || undefined
      },
      tool: selectedTool,
      template: selectedTemplate
    });
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (event.key !== 'Enter' || !event.metaKey) return;
    event.preventDefault();
    submit();
  }
</script>

<CreationDraftScaffold
  {projectName}
  kindLabel="Agent"
  title={draft.name.trim() || 'New agent'}
  createLabel="Create agent"
  {busy}
  canCreate={!loading && selectedTool !== null}
  onCreate={submit}
  {onDiscard}
>
  {#snippet secondaryAction()}
    {#if onOpenSettings}
      <Button type="button" variant="outline" disabled={busy} onclick={onOpenSettings}>Open Settings</Button>
    {/if}
  {/snippet}
  <section class="agent-fields">
    <div class="choice-grid">
      <label for={`draft-agent-template-${draft.id}`}>
        <span>Template <small>optional</small></span>
        <Select.Root type="single" value={templateChoice} disabled={loading || busy} onValueChange={selectTemplate}>
          <Select.Trigger id={`draft-agent-template-${draft.id}`} class="w-full text-left">
            {selectedTemplate?.name ?? 'None'}
          </Select.Trigger>
          <Select.Content>
            <Select.Item value="none" label="None">None</Select.Item>
            {#each templates as template (template.id)}
              {@const tool = tools.find((candidate) => candidate.id === template.agent_tool_id)}
              {#if tool}
                <Select.Item value={`template:${template.id}`} label={template.name} disabled={!tool.enabled}>
                  <AgentBrandMark {tool} size={16} />
                  <span>{template.name}</span>
                  <span class="text-xs text-muted-foreground">{tool.name}{#if !tool.enabled} · agent disabled{/if}</span>
                </Select.Item>
              {/if}
            {/each}
          </Select.Content>
        </Select.Root>
      </label>

      <label for={`draft-agent-tool-${draft.id}`}>
        <span>Agent</span>
        <Select.Root type="single" value={agentChoice} disabled={loading || busy} onValueChange={selectAgent}>
          <Select.Trigger id={`draft-agent-tool-${draft.id}`} class="w-full text-left">
            {#if selectedTool}
              <span class="flex min-w-0 items-center gap-1.5"><AgentBrandMark tool={selectedTool} size={16} /><span class="truncate">{selectedTool.name}</span></span>
            {:else}Select an agent{/if}
          </Select.Trigger>
          <Select.Content>
            {#each enabledTools as tool (tool.id)}
              <Select.Item value={`tool:${tool.id}`} label={tool.name}><AgentBrandMark {tool} size={16} /><span>{tool.name}</span></Select.Item>
            {/each}
          </Select.Content>
        </Select.Root>
        {#if agentOverridden && templateDefaultTool}
          <small>Template default: {templateDefaultTool.name}. Template launch args are skipped for other agents.</small>
        {/if}
      </label>
    </div>

    {#if selectedTemplate}
      <Collapsible.Root bind:open={previewOpen} class="overflow-hidden rounded-md border border-border bg-muted/20">
        <Collapsible.Trigger class="flex min-h-9 w-full items-center gap-2 px-3 text-left focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring">
          <span class="min-w-0 flex-1"><strong class="block text-sm font-medium">Template prompt</strong><span class="block text-xs text-muted-foreground">Template prompt is prepended</span></span>
          <ChevronDownIcon class={`text-muted-foreground ${previewOpen ? 'rotate-180' : ''}`} size={14} />
        </Collapsible.Trigger>
        <Collapsible.Content><div class="template-preview" aria-label="Template prompt preview">{selectedTemplate.prompt || 'No template prompt'}</div></Collapsible.Content>
      </Collapsible.Root>
    {/if}

    <label for={`draft-agent-prompt-${draft.id}`}>
      <span>Prompt <small>optional</small></span>
      <Textarea
        id={`draft-agent-prompt-${draft.id}`}
        class="min-h-[17rem] resize-y text-sm leading-6"
        bind:ref={promptTextarea}
        value={draft.prompt}
        placeholder="What should this agent work on?"
        disabled={busy}
        oninput={(event) => onChange({ prompt: event.currentTarget.value })}
        onkeydown={handleKeydown}
      />
      <small>Cmd+Enter creates. Shift+Enter adds a line.</small>
    </label>

    <Collapsible.Root bind:open={advancedOpen} class="overflow-hidden rounded-md border border-border">
      <Collapsible.Trigger class="flex min-h-9 w-full items-center gap-2 px-3 text-left focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring">
        <SlidersHorizontalIcon class="text-muted-foreground" size={14} />
        <span class="min-w-0 flex-1 text-sm font-medium">Advanced</span>
        <ChevronDownIcon class={`text-muted-foreground ${advancedOpen ? 'rotate-180' : ''}`} size={14} />
      </Collapsible.Trigger>
      <Collapsible.Content>
        <div class="advanced-grid">
          <label for={`draft-agent-name-${draft.id}`}><span>Name <small>optional</small></span><Input id={`draft-agent-name-${draft.id}`} value={draft.name} placeholder={`${selectedTool?.name.toLowerCase() ?? 'agent'} worker`} disabled={busy} oninput={(event) => onChange({ name: event.currentTarget.value })} onkeydown={handleKeydown} /></label>
          <label for={`draft-agent-args-${draft.id}`}><span>Extra launch args <small>optional</small></span><Input id={`draft-agent-args-${draft.id}`} value={draft.extraArgs} placeholder='--model "gpt-5"' disabled={busy} autocapitalize="off" autocorrect="off" spellcheck={false} oninput={(event) => onChange({ extraArgs: event.currentTarget.value })} onkeydown={handleKeydown} /></label>
        </div>
      </Collapsible.Content>
    </Collapsible.Root>

    {#if !loading && enabledTools.length === 0}
      <p class="empty-note">No enabled agents. Add or enable one in Settings.</p>
    {/if}
  </section>
</CreationDraftScaffold>

<style>
  .agent-fields { display: grid; gap: 13px; }
  .choice-grid, .advanced-grid { display: grid; gap: 12px; grid-template-columns: repeat(2, minmax(0, 1fr)); }
  label { display: grid; align-content: start; gap: 6px; color: var(--foreground); font-size: var(--font-size-sm); font-weight: 560; }
  label > span { color: var(--text-soft); }
  label small { color: var(--muted-foreground); font-size: var(--font-size-xs); font-weight: 400; line-height: 1.4; }
  .template-preview { max-height: 144px; overflow-y: auto; border-top: 1px solid var(--border); padding: 8px 12px; color: var(--muted-foreground); font: var(--font-size-xs)/1.55 var(--terminal-font-family); white-space: pre-wrap; }
  .advanced-grid { border-top: 1px solid var(--border); padding: 12px; }
  .empty-note { margin: 0; border: 1px solid var(--border); border-radius: var(--radius); padding: 9px 11px; background: color-mix(in srgb, var(--muted) 20%, transparent); color: var(--muted-foreground); font-size: var(--font-size-sm); }
  @media (max-width: 620px) { .choice-grid, .advanced-grid { grid-template-columns: 1fr; } }
</style>
