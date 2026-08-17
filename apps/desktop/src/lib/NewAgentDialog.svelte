<script lang="ts">
  import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
  import SlidersHorizontalIcon from '@lucide/svelte/icons/sliders-horizontal';
  import XIcon from '@lucide/svelte/icons/x';

  import AgentBrandMark from './AgentBrandMark.svelte';
  import { parseExtraArgs, type AgentTool, type SpawnAgentInput } from './agentTools';
  import {
    choiceValue,
    chooseInitialAgentChoice,
    lastAgentChoiceStorageKey,
    type AgentChoice,
    type AgentTemplate
  } from './agentTemplates';
  import IconButton from './components/ds/IconButton.svelte';
  import { Button } from './components/ui/button';
  import * as Collapsible from './components/ui/collapsible';
  import * as Dialog from './components/ui/dialog';
  import { Input } from './components/ui/input';
  import * as Select from './components/ui/select';
  import { Textarea } from './components/ui/textarea';
  export interface NewAgentSubmission {
    input: SpawnAgentInput;
    tool: AgentTool;
    template: AgentTemplate | null;
  }

  interface Props {
    projectId: number;
    tools: AgentTool[];
    templates: AgentTemplate[];
    loading?: boolean;
    busy?: boolean;
    initialChoice?: AgentChoice | null;
    onSpawn: (submission: NewAgentSubmission) => void | Promise<void>;
    onClose: () => void;
    onOpenSettings?: () => void;
    onError?: (message: string) => void;
  }

  let {
    projectId,
    tools,
    templates,
    loading = false,
    busy = false,
    initialChoice = null,
    onSpawn,
    onClose,
    onOpenSettings,
    onError = () => undefined
  }: Props = $props();

  let templateChoice = $state('none');
  let agentChoice = $state('');
  let prompt = $state('');
  let launchName = $state('');
  let launchArgs = $state('');
  let advancedOpen = $state(false);
  let previewOpen = $state(true);
  let promptTextarea = $state<HTMLTextAreaElement | null>(null);
  let initialized = false;

  const enabledTools = $derived(tools.filter((tool) => tool.enabled));
  const availableTemplates = $derived(
    templates.filter((template) =>
      enabledTools.some((tool) => tool.id === template.agent_tool_id)
    )
  );
  const selectedTemplate = $derived(
    availableTemplates.find((template) => templateChoice === `template:${template.id}`) ?? null
  );
  const selectedTool = $derived(
    enabledTools.find((tool) => agentChoice === `tool:${tool.id}`) ?? null
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

  $effect(() => {
    const valid = selectedTool !== null
      && (templateChoice === 'none' || selectedTemplate !== null);
    if (initialized && valid) return;
    const initial = chooseInitialAgentChoice(
      availableTemplates,
      enabledTools,
      initialChoice ? choiceValue(initialChoice) : readLastChoice()
    );
    applyChoice(initial);
    initialized = true;
  });

  function readLastChoice(): string | null {
    try {
      return localStorage.getItem(lastAgentChoiceStorageKey);
    } catch {
      return null;
    }
  }

  function rememberChoice(selected = currentChoice()): void {
    if (!selected) return;
    try {
      localStorage.setItem(lastAgentChoiceStorageKey, choiceValue(selected));
    } catch {
      // Spawning remains available if webview storage is unavailable.
    }
  }

  function currentChoice(): AgentChoice | null {
    if (!selectedTool) return null;
    return selectedTemplate
      ? { kind: 'template', id: selectedTemplate.id, agentToolId: selectedTool.id }
      : { kind: 'tool', id: selectedTool.id };
  }

  function applyChoice(selected: AgentChoice | null): void {
    if (!selected) {
      templateChoice = 'none';
      agentChoice = '';
      return;
    }
    if (selected.kind === 'template') {
      const template = availableTemplates.find((candidate) => candidate.id === selected.id);
      if (template) {
        templateChoice = `template:${template.id}`;
        agentChoice = `tool:${selected.agentToolId ?? template.agent_tool_id}`;
        return;
      }
    }
    templateChoice = 'none';
    agentChoice = `tool:${selected.id}`;
  }

  function selectTemplate(value: string | undefined): void {
    const template = availableTemplates.find((candidate) => value === `template:${candidate.id}`);
    if (!template) {
      templateChoice = 'none';
      rememberChoice(selectedTool ? { kind: 'tool', id: selectedTool.id } : null);
      return;
    }
    templateChoice = `template:${template.id}`;
    agentChoice = `tool:${template.agent_tool_id}`;
    rememberChoice({ kind: 'template', id: template.id, agentToolId: template.agent_tool_id });
  }

  function selectAgent(value: string | undefined): void {
    const tool = enabledTools.find((candidate) => value === `tool:${candidate.id}`);
    if (!tool) return;
    agentChoice = `tool:${tool.id}`;
    rememberChoice(selectedTemplate
      ? { kind: 'template', id: selectedTemplate.id, agentToolId: tool.id }
      : { kind: 'tool', id: tool.id });
  }

  async function submit(): Promise<void> {
    if (!selectedTool || busy) return;
    let extraArgs: string[];
    try {
      extraArgs = parseExtraArgs(launchArgs);
    } catch (cause) {
      onError(cause instanceof Error ? cause.message : String(cause));
      return;
    }
    rememberChoice();
    await onSpawn({
      input: {
        project_id: projectId,
        ...(selectedTemplate
          ? { agent_template_id: selectedTemplate.id, agent_tool_id: selectedTool.id }
          : { agent_tool_id: selectedTool.id }),
        name: launchName.trim() || undefined,
        extra_args: extraArgs,
        prompt: prompt.trim() || undefined
      },
      tool: selectedTool,
      template: selectedTemplate
    });
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (event.key !== 'Enter' || !event.metaKey) return;
    event.preventDefault();
    void submit();
  }

  function focusPromptOnOpen(event: Event): void {
    if (enabledTools.length === 0) return;
    event.preventDefault();
    requestAnimationFrame(() => promptTextarea?.focus());
  }
</script>

<Dialog.Root open onOpenChange={(open) => { if (!open && !busy) onClose(); }}>
  <Dialog.Content
    class="w-[min(880px,calc(100vw-24px))] !max-w-none gap-0 overflow-hidden rounded-md border border-border bg-popover p-0 shadow-2xl"
    showCloseButton={false}
    aria-label="New agent"
    onOpenAutoFocus={focusPromptOnOpen}
  >
    <form onsubmit={(event) => { event.preventDefault(); void submit(); }}>
      <header class="flex items-start justify-between gap-3 border-b border-border px-4 py-3">
        <div>
          <span class="text-xs font-medium text-muted-foreground">New agent</span>
          <h2 class="mt-0.5 text-base font-semibold">Choose a template and agent</h2>
        </div>
        <IconButton label="Close new agent" disabled={busy} onclick={onClose}>
          {#snippet icon()}<XIcon size={14} />{/snippet}
        </IconButton>
      </header>

      <section class="grid max-h-[calc(100dvh-10rem)] gap-3 overflow-y-auto p-4">
        <div class="grid gap-3 sm:grid-cols-2">
          <label class="grid content-start gap-1.5 text-sm font-medium" for="new-agent-template">
            <span>Template <span class="font-normal text-muted-foreground">(optional)</span></span>
            <Select.Root type="single" value={templateChoice} disabled={loading || busy} onValueChange={selectTemplate}>
              <Select.Trigger id="new-agent-template" class="w-full text-left">
                {selectedTemplate?.name ?? 'None'}
              </Select.Trigger>
              <Select.Content>
                <Select.Item value="none" label="None">None</Select.Item>
                {#each templates as template (template.id)}
                  {@const tool = tools.find((candidate) => candidate.id === template.agent_tool_id)}
                  {#if tool}
                    <Select.Item
                      value={`template:${template.id}`}
                      label={template.name}
                      disabled={!tool.enabled}
                    >
                      <AgentBrandMark {tool} size={16} />
                      <span>{template.name}</span>
                      <span class="text-xs text-muted-foreground">
                        {tool.name}{#if !tool.enabled} · agent disabled{/if}
                      </span>
                    </Select.Item>
                  {/if}
                {/each}
              </Select.Content>
            </Select.Root>
          </label>

          <label class="grid content-start gap-1.5 text-sm font-medium" for="new-agent-tool">
            Agent
            <Select.Root type="single" value={agentChoice} disabled={loading || busy} onValueChange={selectAgent}>
              <Select.Trigger id="new-agent-tool" class="w-full text-left">
                {#if selectedTool}
                  <span class="flex min-w-0 items-center gap-1.5">
                    <AgentBrandMark tool={selectedTool} size={16} />
                    <span class="truncate">{selectedTool.name}</span>
                  </span>
                {:else}
                  Select an agent
                {/if}
              </Select.Trigger>
              <Select.Content>
                {#each enabledTools as tool (tool.id)}
                  <Select.Item value={`tool:${tool.id}`} label={tool.name}>
                    <AgentBrandMark {tool} size={16} />
                    <span>{tool.name}</span>
                  </Select.Item>
                {/each}
              </Select.Content>
            </Select.Root>
            {#if agentOverridden && templateDefaultTool}
              <span class="whitespace-normal text-xs font-normal leading-4 text-muted-foreground">
                Template default: {templateDefaultTool.name}. Template launch args are skipped for other agents.
              </span>
            {/if}
          </label>
        </div>

        {#if selectedTemplate}
          <Collapsible.Root bind:open={previewOpen} class="overflow-hidden rounded-md border border-border bg-muted/20">
            <Collapsible.Trigger class="flex min-h-9 w-full items-center gap-2 px-3 text-left focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring">
              <span class="min-w-0 flex-1">
                <strong class="block text-sm font-medium">Template prompt</strong>
                <span class="block text-xs text-muted-foreground">Template prompt is prepended</span>
              </span>
              <ChevronDownIcon class={`text-muted-foreground motion-safe:transition-transform ${previewOpen ? 'rotate-180' : ''}`} size={14} />
            </Collapsible.Trigger>
            <Collapsible.Content>
              <div class="max-h-36 overflow-y-auto whitespace-pre-wrap border-t border-border px-3 py-2 font-mono text-xs leading-5 text-muted-foreground" aria-label="Template prompt preview">{selectedTemplate.prompt || 'No template prompt'}</div>
            </Collapsible.Content>
          </Collapsible.Root>
        {/if}

        <label class="grid gap-1.5 text-sm font-medium" for="new-agent-prompt">
          <span>Prompt <span class="font-normal text-muted-foreground">(optional)</span></span>
          <Textarea
            id="new-agent-prompt"
            class="min-h-[17rem] resize-y text-sm leading-6"
            bind:ref={promptTextarea}
            bind:value={prompt}
            placeholder="What should this agent work on?"
            disabled={busy}
            onkeydown={handleKeydown}
          />
          <span class="text-xs font-normal text-muted-foreground">Cmd+Enter spawns. Shift+Enter adds a line.</span>
        </label>

        <Collapsible.Root bind:open={advancedOpen} class="overflow-hidden rounded-md border border-border">
          <Collapsible.Trigger class="flex min-h-9 w-full items-center gap-2 px-3 text-left focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring">
            <SlidersHorizontalIcon class="text-muted-foreground" size={14} />
            <span class="min-w-0 flex-1 text-sm font-medium">Advanced</span>
            <ChevronDownIcon class={`text-muted-foreground motion-safe:transition-transform ${advancedOpen ? 'rotate-180' : ''}`} size={14} />
          </Collapsible.Trigger>
          <Collapsible.Content>
            <div class="grid gap-3 border-t border-border p-3 sm:grid-cols-2">
              <label class="grid gap-1.5 text-sm font-medium" for="new-agent-name">
                <span>Name <span class="font-normal text-muted-foreground">(optional)</span></span>
                <Input id="new-agent-name" bind:value={launchName} placeholder={`${selectedTool?.name.toLowerCase() ?? 'agent'} worker`} disabled={busy} onkeydown={handleKeydown} />
              </label>
              <label class="grid gap-1.5 text-sm font-medium" for="new-agent-args">
                <span>Extra launch args <span class="font-normal text-muted-foreground">(optional)</span></span>
                <Input id="new-agent-args" bind:value={launchArgs} placeholder='--model "gpt-5"' disabled={busy} autocapitalize="off" autocorrect="off" spellcheck={false} onkeydown={handleKeydown} />
              </label>
            </div>
          </Collapsible.Content>
        </Collapsible.Root>

        {#if !loading && enabledTools.length === 0}
          <p class="rounded-md border border-border bg-muted/20 px-3 py-2 text-sm text-muted-foreground">No enabled agents. Add or enable one in Settings.</p>
        {/if}
      </section>

      <footer class="flex items-center justify-between gap-2 border-t border-border px-4 py-3">
        {#if onOpenSettings}
          <Button type="button" variant="outline" disabled={busy} onclick={onOpenSettings}>Open Settings</Button>
        {:else}<span></span>{/if}
        <div class="flex gap-2">
          <Button type="button" variant="ghost" disabled={busy} onclick={onClose}>Cancel</Button>
          <Button type="submit" disabled={busy || loading || !selectedTool}>{busy ? 'Spawning…' : 'Spawn'}</Button>
        </div>
      </footer>
    </form>
  </Dialog.Content>
</Dialog.Root>
