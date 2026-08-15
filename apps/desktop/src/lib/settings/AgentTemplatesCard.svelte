<script lang="ts">
  import ArrowDownIcon from '@lucide/svelte/icons/arrow-down';
  import ArrowUpIcon from '@lucide/svelte/icons/arrow-up';
  import PencilIcon from '@lucide/svelte/icons/pencil';
  import PlusIcon from '@lucide/svelte/icons/plus';
  import Trash2Icon from '@lucide/svelte/icons/trash-2';
  import XIcon from '@lucide/svelte/icons/x';

  import AgentBrandMark from '../AgentBrandMark.svelte';
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
    const tool = toolSnapshot.tools.find((candidate) => candidate.enabled) ?? toolSnapshot.tools[0];
    if (!tool) {
      onError('Add an agent tool before creating an agent template');
      return;
    }
    draft = { name: '', agentToolId: tool.id, extraArgs: '', prompt: '' };
  }

  function beginEdit(template: AgentTemplate): void {
    draft = {
      id: template.id,
      name: template.name,
      agentToolId: template.agent_tool_id,
      extraArgs: formatExtraArgs(template.extra_args),
      prompt: template.prompt
    };
  }

  async function save(): Promise<void> {
    if (!draft || saving || !draft.name.trim()) return;
    let extraArgs: string[];
    try {
      extraArgs = parseExtraArgs(draft.extraArgs);
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

  function message(cause: unknown): string {
    return cause instanceof Error ? cause.message : String(cause);
  }
</script>

<section class="overflow-hidden rounded-md border border-border bg-card" aria-labelledby="agent-templates-title">
  <header class="flex items-start justify-between gap-3 border-b border-border px-4 py-3">
    <div>
      <span class="text-xs font-medium text-muted-foreground">Active profile</span>
      <h2 id="agent-templates-title" class="mt-0.5 text-base font-semibold">Agent templates</h2>
      <p class="mt-1 text-sm text-muted-foreground">Pair an agent tool with launch arguments and a reusable prompt.</p>
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
              <span class="shrink-0 text-xs text-muted-foreground">{tool?.name ?? 'Missing agent tool'}</span>
            </div>
            <p class="mt-0.5 truncate text-xs text-muted-foreground">
              {template.extra_args.length ? formatExtraArgs(template.extra_args) : 'No extra launch args'} · {template.prompt || 'No template prompt'}
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
        <label class="grid gap-1.5 text-sm font-medium" for="template-tool">Agent tool
          <Select.Root type="single" value={String(draft.agentToolId)} disabled={saving} onValueChange={(value) => { if (value) draft!.agentToolId = Number(value); }}>
            <Select.Trigger id="template-tool" class="w-full">
              {@const currentTool = toolSnapshot.tools.find((tool) => tool.id === draft?.agentToolId)}
              {#if currentTool}<AgentBrandMark tool={currentTool} size={16} />{currentTool.name}{:else}Select an agent tool{/if}
            </Select.Trigger>
            <Select.Content>
              {#each toolSnapshot.tools as tool (tool.id)}
                <Select.Item value={String(tool.id)} label={tool.name}>
                  <AgentBrandMark {tool} size={16} />{tool.name}{#if !tool.enabled}<span class="text-xs text-muted-foreground">Disabled</span>{/if}
                </Select.Item>
              {/each}
            </Select.Content>
          </Select.Root>
        </label>
      </div>
      <label class="grid gap-1.5 text-sm font-medium" for="template-args">Extra launch args <span class="font-normal text-muted-foreground">(optional)</span>
        <Input id="template-args" bind:value={draft.extraArgs} placeholder='--model "gpt-5.6-sol"' disabled={saving} autocapitalize="off" autocorrect="off" spellcheck={false} />
        <span class="text-xs font-normal text-muted-foreground">Quotes group one literal argument. These are prepended to any per-launch arguments.</span>
      </label>
      <label class="grid gap-1.5 text-sm font-medium" for="template-prompt">Template prompt <span class="font-normal text-muted-foreground">(optional)</span>
        <Textarea id="template-prompt" bind:value={draft.prompt} rows={6} placeholder="Persistent instructions for agents launched with this template" disabled={saving} />
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
    description="This removes the agent template from the active profile. Agent tools and existing processes are unchanged."
    confirmLabel="Delete template"
    onConfirm={() => void confirmRemove()}
    onClose={() => (removeRequest = null)}
  />
{/if}
