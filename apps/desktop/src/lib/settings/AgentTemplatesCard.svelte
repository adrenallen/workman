<script lang="ts">
  import ArrowDownIcon from '@lucide/svelte/icons/arrow-down';
  import ArrowUpIcon from '@lucide/svelte/icons/arrow-up';
  import PencilIcon from '@lucide/svelte/icons/pencil';
  import PlusIcon from '@lucide/svelte/icons/plus';
  import Trash2Icon from '@lucide/svelte/icons/trash-2';
  import XIcon from '@lucide/svelte/icons/x';

  import AgentBrandMark from '../AgentBrandMark.svelte';
  import {
    AGENT_EFFORT_LEVELS,
    agentModelSuggestions,
    agentSupportsEffort,
    agentSupportsModel,
    configuredAgentLaunchOptions,
    splitAgentLaunchOptions,
    withAgentLaunchOptions
  } from '../agentLaunchOptions';
  import type { AgentTool, AgentToolsSnapshot } from '../agentTools';
  import { formatExtraArgs, getAgentToolsStore, parseExtraArgs } from '../agentTools';
  import {
    getAgentTemplatesStore,
    type AgentTemplate,
    type AgentTemplatesSnapshot
  } from '../agentTemplates';
  import ConfirmationDialog from '../ConfirmationDialog.svelte';
  import IconButton from '../components/ds/IconButton.svelte';
  import { Button } from '../components/ui/button';
  import { Input } from '../components/ui/input';
  import * as Select from '../components/ui/select';
  import { Textarea } from '../components/ui/textarea';
  import type { DaemonClient } from '../daemon';

  interface Props {
    client: DaemonClient;
    connected: boolean;
    onError: (message: string) => void;
  }

  interface Draft {
    id?: number;
    name: string;
    agentToolId: number;
    model: string;
    effort: string;
    extraArgs: string;
    prompt: string;
  }

  let { client, connected, onError }: Props = $props();
  // The Settings panel owns one DaemonClient for this component's lifetime.
  // svelte-ignore state_referenced_locally
  const toolStore = getAgentToolsStore(client);
  // svelte-ignore state_referenced_locally
  const templateStore = getAgentTemplatesStore(client);
  let toolSnapshot = $state<AgentToolsSnapshot>(toolStore.current());
  let templateSnapshot = $state<AgentTemplatesSnapshot>(templateStore.current());
  let draft = $state<Draft | null>(null);
  let saving = $state(false);
  let busyId = $state<number | null>(null);
  let removeRequest = $state<AgentTemplate | null>(null);
  let draftTool = $derived(
    toolSnapshot.tools.find((tool) => tool.id === draft?.agentToolId) ?? null
  );
  let draftAgentDefaults = $derived(configuredAgentLaunchOptions(draftTool));
  let draftModelSuggestions = $derived(agentModelSuggestions(draftTool?.tool_type));

  $effect(() => {
    const unsubscribeTools = toolStore.subscribe((snapshot) => (toolSnapshot = snapshot));
    const unsubscribeTemplates = templateStore.subscribe((snapshot) => (templateSnapshot = snapshot));
    return () => {
      unsubscribeTools();
      unsubscribeTemplates();
    };
  });

  $effect(() => {
    if (connected) {
      void Promise.all([toolStore.refresh(), templateStore.refresh()]).catch((cause) =>
        onError(message(cause))
      );
    }
  });

  function beginNew(): void {
    const tool = toolSnapshot.tools.find((candidate) => candidate.enabled);
    if (!tool) {
      onError('Add or enable an agent before creating an agent template');
      return;
    }
    draft = { name: '', agentToolId: tool.id, model: '', effort: '', extraArgs: '', prompt: '' };
  }

  function beginEdit(template: AgentTemplate): void {
    const tool = toolFor(template);
    const launch = splitAgentLaunchOptions(template.extra_args, tool?.tool_type);
    draft = {
      id: template.id,
      name: template.name,
      agentToolId: template.agent_tool_id,
      model: launch.model ?? '',
      effort: launch.effort ?? '',
      extraArgs: formatExtraArgs(launch.extraArgs),
      prompt: template.prompt
    };
  }

  async function save(): Promise<void> {
    if (!draft || saving || !draft.name.trim()) return;
    let extraArgs: string[];
    try {
      const parsed = splitAgentLaunchOptions(parseExtraArgs(draft.extraArgs), draftTool?.tool_type);
      extraArgs = withAgentLaunchOptions(
        parsed.extraArgs,
        draftTool?.tool_type,
        draft.model.trim() || parsed.model,
        draft.effort || parsed.effort
      );
    } catch (cause) {
      onError(message(cause));
      return;
    }
    saving = true;
    try {
      await templateStore.save({
        id: draft.id,
        name: draft.name.trim(),
        agent_tool_id: draft.agentToolId,
        extra_args: extraArgs,
        prompt: draft.prompt
      });
      draft = null;
    } catch (cause) {
      onError(message(cause));
    } finally {
      saving = false;
    }
  }

  async function move(template: AgentTemplate, direction: -1 | 1): Promise<void> {
    const index = templateSnapshot.templates.findIndex((candidate) => candidate.id === template.id);
    const target = index + direction;
    if (index < 0 || target < 0 || target >= templateSnapshot.templates.length) return;
    busyId = template.id;
    try {
      const reordered = [...templateSnapshot.templates];
      [reordered[index], reordered[target]] = [reordered[target], reordered[index]];
      await templateStore.reorder(reordered.map((candidate) => candidate.id));
    } catch (cause) {
      onError(message(cause));
    } finally {
      busyId = null;
    }
  }

  async function confirmRemove(): Promise<void> {
    const template = removeRequest;
    if (!template) return;
    removeRequest = null;
    busyId = template.id;
    try {
      await templateStore.remove(template.id);
      if (draft?.id === template.id) draft = null;
    } catch (cause) {
      onError(message(cause));
    } finally {
      busyId = null;
    }
  }

  function toolFor(template: AgentTemplate): AgentTool | null {
    return toolSnapshot.tools.find((tool) => tool.id === template.agent_tool_id) ?? null;
  }

  function selectDraftTool(value: string | undefined): void {
    if (!draft || !value) return;
    const tool = toolSnapshot.tools.find((candidate) => candidate.id === Number(value));
    if (!tool) return;
    if (tool.id !== draft.agentToolId) {
      draft.model = '';
      draft.effort = '';
    }
    draft.agentToolId = tool.id;
    if (!agentSupportsModel(tool.tool_type)) draft.model = '';
    if (!agentSupportsEffort(tool.tool_type)) draft.effort = '';
  }

  function templateLaunchSummary(template: AgentTemplate, tool: AgentTool | null): string {
    const launch = splitAgentLaunchOptions(template.extra_args, tool?.tool_type);
    const parts = [
      launch.model,
      launch.effort ? `${launch.effort} effort` : null,
      launch.extraArgs.length ? formatExtraArgs(launch.extraArgs) : null
    ].filter((part): part is string => Boolean(part));
    return parts.length > 0 ? parts.join(' · ') : 'Agent launch defaults';
  }

  function message(cause: unknown): string {
    return cause instanceof Error ? cause.message : String(cause);
  }
</script>

<section class="overflow-hidden rounded-md border border-border bg-card" aria-labelledby="agent-templates-title">
  <header class="flex items-start justify-between gap-3 border-b border-border px-4 py-3">
    <div>
      <span class="text-xs font-medium text-muted-foreground">Active profile</span>
      <h2 id="agent-templates-title" class="mt-0.5 text-base font-semibold">Agent templates</h2>
      <p class="mt-1 text-sm text-muted-foreground">Pair a default agent with launch arguments and a reusable prompt.</p>
    </div>
    <Button size="sm" disabled={!connected || draft !== null} onclick={beginNew}>
      <PlusIcon size={14} />New template
    </Button>
  </header>

  {#if templateSnapshot.loading && templateSnapshot.templates.length === 0}
    <p class="px-4 py-6 text-sm text-muted-foreground">Loading agent templates…</p>
  {:else if templateSnapshot.templates.length === 0}
    <div class="grid justify-items-start gap-2 px-4 py-6">
      <strong class="text-sm font-medium">No agent templates yet</strong>
      <p class="max-w-xl text-sm text-muted-foreground">Templates are optional. Create one when a model, launch flag, or starting instruction is worth reusing.</p>
      <Button size="sm" variant="outline" disabled={!connected} onclick={beginNew}>Create the first template</Button>
    </div>
  {:else}
    <div class="divide-y divide-border">
      {#each templateSnapshot.templates as template, index (template.id)}
        {@const tool = toolFor(template)}
        <article class="grid min-h-14 grid-cols-[auto_auto_minmax(0,1fr)_auto] items-center gap-3 px-3 py-2">
          <div class="grid grid-cols-2 gap-0.5" aria-label={`Reorder ${template.name}`}>
            <IconButton label={`Move ${template.name} up`} disabled={!connected || busyId !== null || index === 0} onclick={() => void move(template, -1)}>
              {#snippet icon()}<ArrowUpIcon size={13} />{/snippet}
            </IconButton>
            <IconButton label={`Move ${template.name} down`} disabled={!connected || busyId !== null || index === templateSnapshot.templates.length - 1} onclick={() => void move(template, 1)}>
              {#snippet icon()}<ArrowDownIcon size={13} />{/snippet}
            </IconButton>
          </div>
          <AgentBrandMark {tool} fallbackName={template.name} size={20} />
          <div class="min-w-0">
            <div class="flex min-w-0 items-center gap-2">
              <strong class="truncate text-sm font-medium">{template.name}</strong>
              <span class="shrink-0 text-xs text-muted-foreground">{tool?.name ?? 'Missing default agent'}</span>
            </div>
            <p class="mt-0.5 truncate text-xs text-muted-foreground">
              {templateLaunchSummary(template, tool)} · {template.prompt || 'No template prompt'}
            </p>
          </div>
          <div class="flex gap-1">
            <IconButton label={`Edit ${template.name}`} disabled={!connected || draft !== null || busyId !== null} onclick={() => beginEdit(template)}>
              {#snippet icon()}<PencilIcon size={14} />{/snippet}
            </IconButton>
            <IconButton label={`Delete ${template.name}`} variant="destructive" disabled={!connected || busyId !== null} onclick={() => (removeRequest = template)}>
              {#snippet icon()}<Trash2Icon size={14} />{/snippet}
            </IconButton>
          </div>
        </article>
      {/each}
    </div>
  {/if}

  {#if draft}
    <form class="grid gap-3 border-t border-border bg-muted/10 p-4" onsubmit={(event) => { event.preventDefault(); void save(); }}>
      <header class="flex items-start justify-between gap-3">
        <div><span class="text-xs text-muted-foreground">Template editor</span><h3 class="text-sm font-semibold">{draft.id ? `Edit ${draft.name}` : 'New agent template'}</h3></div>
        <IconButton label="Close template editor" disabled={saving} onclick={() => (draft = null)}>{#snippet icon()}<XIcon size={14} />{/snippet}</IconButton>
      </header>
      <div class="grid gap-3 sm:grid-cols-2">
        <label class="grid gap-1.5 text-sm font-medium" for="template-name">Name
          <Input id="template-name" bind:value={draft.name} placeholder="Implementation worker" required disabled={saving} />
        </label>
        <label class="grid gap-1.5 text-sm font-medium" for="template-tool">Default agent
          <Select.Root type="single" value={String(draft.agentToolId)} disabled={saving} onValueChange={selectDraftTool}>
            <Select.Trigger id="template-tool" class="w-full">
              {@const currentTool = toolSnapshot.tools.find((tool) => tool.id === draft?.agentToolId)}
              {#if currentTool}<AgentBrandMark tool={currentTool} size={16} />{currentTool.name}{:else}Select a default agent{/if}
            </Select.Trigger>
            <Select.Content>
              {#each toolSnapshot.tools as tool (tool.id)}
                <Select.Item
                  value={String(tool.id)}
                  label={`${tool.name}${tool.enabled ? '' : ' (agent disabled)'}`}
                  disabled={!tool.enabled}
                >
                  <AgentBrandMark {tool} size={16} />{tool.name}{#if !tool.enabled}<span class="text-xs text-muted-foreground">Agent disabled</span>{/if}
                </Select.Item>
              {/each}
            </Select.Content>
          </Select.Root>
        </label>
      </div>
      {#if draftTool && (agentSupportsModel(draftTool.tool_type) || agentSupportsEffort(draftTool.tool_type))}
        <section class="grid gap-3 rounded-md border border-border bg-background/55 p-3" aria-labelledby="template-launch-tuning">
          <div class="flex flex-wrap items-baseline justify-between gap-2">
            <strong id="template-launch-tuning" class="text-sm font-medium">Default model &amp; effort</strong>
            <small class="text-xs text-muted-foreground">Shown as inherited values in New Agent · Advanced</small>
          </div>
          <div class="grid gap-3 sm:grid-cols-2">
            {#if agentSupportsModel(draftTool.tool_type)}
              <label class="grid gap-1.5 text-sm font-medium" for="template-model">Model <span class="font-normal text-muted-foreground">(optional)</span>
                <Input
                  id="template-model"
                  bind:value={draft.model}
                  list={draftModelSuggestions.length > 0 ? 'template-models' : undefined}
                  placeholder={draftAgentDefaults.model ?? 'Agent default'}
                  disabled={saving}
                  autocapitalize="off"
                  autocorrect="off"
                  spellcheck={false}
                />
                {#if draftModelSuggestions.length > 0}
                  <datalist id="template-models">
                    {#each draftModelSuggestions as model}<option value={model}></option>{/each}
                  </datalist>
                {/if}
              </label>
            {/if}
            {#if agentSupportsEffort(draftTool.tool_type)}
              <label class="grid gap-1.5 text-sm font-medium" for="template-effort">Effort <span class="font-normal text-muted-foreground">(optional)</span>
                <Select.Root type="single" value={draft.effort || 'inherit'} disabled={saving} onValueChange={(value) => { if (draft) draft.effort = value === 'inherit' ? '' : value; }}>
                  <Select.Trigger id="template-effort" class="w-full">
                    {draft.effort || (draftAgentDefaults.effort ? `Agent default · ${draftAgentDefaults.effort}` : 'Agent default')}
                  </Select.Trigger>
                  <Select.Content>
                    <Select.Item value="inherit" label={draftAgentDefaults.effort ? `Agent default · ${draftAgentDefaults.effort}` : 'Agent default'} />
                    {#each AGENT_EFFORT_LEVELS as effort}<Select.Item value={effort} label={effort} />{/each}
                  </Select.Content>
                </Select.Root>
              </label>
            {/if}
          </div>
        </section>
      {/if}
      <label class="grid gap-1.5 text-sm font-medium" for="template-args">Other launch args <span class="font-normal text-muted-foreground">(optional)</span>
        <Input id="template-args" bind:value={draft.extraArgs} placeholder="--permission-mode plan" disabled={saving} autocapitalize="off" autocorrect="off" spellcheck={false} />
        <span class="text-xs font-normal text-muted-foreground">Quotes group one literal argument. These run before any per-launch overrides.</span>
      </label>
      <label class="grid gap-1.5 text-sm font-medium" for="template-prompt">Template prompt <span class="font-normal text-muted-foreground">(optional)</span>
        <Textarea id="template-prompt" bind:value={draft.prompt} rows={6} placeholder="Persistent instructions for agents launched with this template" disabled={saving} />
        <span class="text-xs font-normal text-muted-foreground">Combined with any New Agent instructions and sent as one starting prompt.</span>
      </label>
      <footer class="flex justify-end gap-2">
        <Button type="button" variant="ghost" disabled={saving} onclick={() => (draft = null)}>Cancel</Button>
        <Button type="submit" disabled={saving || !draft.name.trim() || !draft.agentToolId}>{saving ? 'Saving…' : 'Save template'}</Button>
      </footer>
    </form>
  {/if}
</section>

{#if removeRequest}
  <ConfirmationDialog
    title={`Delete ${removeRequest.name}?`}
    description="This removes the agent template from the active profile. Agents and existing processes are unchanged."
    confirmLabel="Delete template"
    onConfirm={() => void confirmRemove()}
    onClose={() => (removeRequest = null)}
  />
{/if}
