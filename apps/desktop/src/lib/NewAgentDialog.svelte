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
    parseChoiceValue,
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

  let choice = $state('');
  let prompt = $state('');
  let launchName = $state('');
  let launchArgs = $state('');
  let advancedOpen = $state(false);
  let previewOpen = $state(true);
  let initialized = false;

  const enabledTools = $derived(tools.filter((tool) => tool.enabled));
  const availableTemplates = $derived(
    templates.filter((template) =>
      enabledTools.some((tool) => tool.id === template.agent_tool_id)
    )
  );
  const selectedChoice = $derived(parseChoiceValue(choice));
  const selectedTemplate = $derived(
    selectedChoice?.kind === 'template'
      ? availableTemplates.find((template) => template.id === selectedChoice.id) ?? null
      : null
  );
  const selectedTool = $derived(
    selectedTemplate
      ? enabledTools.find((tool) => tool.id === selectedTemplate.agent_tool_id) ?? null
      : selectedChoice?.kind === 'tool'
        ? enabledTools.find((tool) => tool.id === selectedChoice.id) ?? null
        : null
  );

  $effect(() => {
    const valid = selectedTool !== null;
    if (initialized && valid) return;
    const initial = chooseInitialAgentChoice(
      availableTemplates,
      enabledTools,
      initialChoice ? choiceValue(initialChoice) : readLastChoice()
    );
    choice = initial ? choiceValue(initial) : '';
    initialized = true;
  });

  function readLastChoice(): string | null {
    try {
      return localStorage.getItem(lastAgentChoiceStorageKey);
    } catch {
      return null;
    }
  }

  function rememberChoice(value = choice): void {
    try {
      localStorage.setItem(lastAgentChoiceStorageKey, value);
    } catch {
      // Spawning remains available if webview storage is unavailable.
    }
  }

  function selectChoice(value: string | undefined): void {
    choice = value ?? '';
    if (choice) rememberChoice(choice);
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
          ? { agent_template_id: selectedTemplate.id }
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
</script>

<Dialog.Root open onOpenChange={(open) => { if (!open && !busy) onClose(); }}>
  <Dialog.Content
    class="max-w-[560px] gap-0 overflow-hidden rounded-md border border-border bg-popover p-0 shadow-2xl"
    showCloseButton={false}
    aria-label="New agent"
  >
    <form onsubmit={(event) => { event.preventDefault(); void submit(); }}>
      <header class="flex items-start justify-between gap-3 border-b border-border px-4 py-3">
        <div>
          <span class="text-xs font-medium text-muted-foreground">New agent</span>
          <h2 class="mt-0.5 text-base font-semibold">Choose a template or agent</h2>
        </div>
        <IconButton label="Close new agent" disabled={busy} onclick={onClose}>
          {#snippet icon()}<XIcon size={14} />{/snippet}
        </IconButton>
      </header>

      <section class="grid max-h-[calc(100dvh-10rem)] gap-3 overflow-y-auto p-4">
        <label class="grid gap-1.5 text-sm font-medium" for="new-agent-choice">
          Launch with
          <Select.Root type="single" value={choice} disabled={loading || busy} onValueChange={selectChoice}>
            <Select.Trigger id="new-agent-choice" class="w-full">
              {#if selectedTool}
                <AgentBrandMark tool={selectedTool} size={16} />
                <span class="truncate">{selectedTemplate?.name ?? selectedTool.name}</span>
                <span class="ml-auto text-xs text-muted-foreground">{selectedTemplate ? 'Template' : 'Agent'}</span>
              {:else}
                Select an agent
              {/if}
            </Select.Trigger>
            <Select.Content>
              {#if availableTemplates.length > 0}
                <Select.Group>
                  <Select.GroupHeading>Templates</Select.GroupHeading>
                  {#each availableTemplates as template (template.id)}
                    {@const tool = enabledTools.find((candidate) => candidate.id === template.agent_tool_id)}
                    {#if tool}
                      <Select.Item value={`template:${template.id}`} label={template.name}>
                        <AgentBrandMark {tool} size={16} />
                        <span>{template.name}</span>
                        <span class="text-xs text-muted-foreground">{tool.name}</span>
                      </Select.Item>
                    {/if}
                  {/each}
                </Select.Group>
                <Select.Separator />
              {/if}
              <Select.Group>
                <Select.GroupHeading>Agents</Select.GroupHeading>
                {#each enabledTools as tool (tool.id)}
                  <Select.Item value={`tool:${tool.id}`} label={tool.name}>
                    <AgentBrandMark {tool} size={16} />
                    <span>{tool.name}</span>
                  </Select.Item>
                {/each}
              </Select.Group>
            </Select.Content>
          </Select.Root>
        </label>

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
          Prompt <span class="font-normal text-muted-foreground">(optional)</span>
          <Textarea id="new-agent-prompt" bind:value={prompt} rows={5} placeholder="What should this agent work on?" disabled={busy} onkeydown={handleKeydown} />
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
                Name <span class="font-normal text-muted-foreground">(optional)</span>
                <Input id="new-agent-name" bind:value={launchName} placeholder={`${selectedTool?.name.toLowerCase() ?? 'agent'} worker`} disabled={busy} onkeydown={handleKeydown} />
              </label>
              <label class="grid gap-1.5 text-sm font-medium" for="new-agent-args">
                Extra launch args <span class="font-normal text-muted-foreground">(optional)</span>
                <Input id="new-agent-args" bind:value={launchArgs} placeholder='--model "gpt-5"' disabled={busy} autocapitalize="off" autocorrect="off" spellcheck={false} onkeydown={handleKeydown} />
              </label>
            </div>
          </Collapsible.Content>
        </Collapsible.Root>

        {#if !loading && enabledTools.length === 0}
          <p class="rounded-md border border-border bg-muted/20 px-3 py-2 text-sm text-muted-foreground">No enabled agent tools. Add or enable one in Settings.</p>
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
