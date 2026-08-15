<script lang="ts">
  import ArrowDownIcon from '@lucide/svelte/icons/arrow-down';
  import ArrowUpIcon from '@lucide/svelte/icons/arrow-up';
  import PencilIcon from '@lucide/svelte/icons/pencil';
  import PlusIcon from '@lucide/svelte/icons/plus';
  import Trash2Icon from '@lucide/svelte/icons/trash-2';
  import { Button } from '$lib/components/ui/button';
  import IconButton from '$lib/components/ds/IconButton.svelte';
  import ConfirmationDialog from '../ConfirmationDialog.svelte';
  import QuickPromptEditor from '../QuickPromptEditor.svelte';
  import type { DaemonClient } from '../daemon';
  import { quickPromptPreview } from '../quickPromptPalette';
  import {
    getQuickPromptsStore,
    type QuickPrompt,
    type QuickPromptsSnapshot
  } from '../quickPrompts';

  interface Props {
    client: DaemonClient;
    connected: boolean;
    onError: (message: string) => void;
  }

  let { client, connected, onError }: Props = $props();
  let store = $derived(getQuickPromptsStore(client));
  let snapshot = $state<QuickPromptsSnapshot>({ prompts: [], loading: false, error: null });
  let editing = $state<QuickPrompt | 'new' | null>(null);
  let removeRequest = $state<QuickPrompt | null>(null);
  let busyId = $state<number | null>(null);

  $effect(() => {
    snapshot = store.current();
    return store.subscribe((next) => (snapshot = next));
  });

  $effect(() => {
    if (connected) void store.refresh().catch((cause) => onError(message(cause)));
  });

  async function move(prompt: QuickPrompt, direction: -1 | 1): Promise<void> {
    const index = snapshot.prompts.findIndex((candidate) => candidate.id === prompt.id);
    const target = index + direction;
    if (index < 0 || target < 0 || target >= snapshot.prompts.length) return;
    busyId = prompt.id;
    try {
      const reordered = [...snapshot.prompts];
      [reordered[index], reordered[target]] = [reordered[target], reordered[index]];
      await store.reorder(reordered.map((candidate) => candidate.id));
    } catch (cause) {
      onError(message(cause));
    } finally {
      busyId = null;
    }
  }

  async function remove(): Promise<void> {
    const prompt = removeRequest;
    if (!prompt) return;
    removeRequest = null;
    busyId = prompt.id;
    try {
      await store.remove(prompt.id);
      if (editing !== 'new' && editing?.id === prompt.id) editing = null;
    } catch (cause) {
      onError(message(cause));
    } finally {
      busyId = null;
    }
  }

  function message(cause: unknown): string {
    return cause instanceof Error ? cause.message : String(cause);
  }
</script>

<section class="overflow-hidden rounded-lg border border-border bg-card" aria-labelledby="quick-prompts-settings-title">
  <header class="flex min-h-14 items-center justify-between gap-3 border-b border-border px-3 py-2">
    <div class="min-w-0">
      <span class="block text-xs font-semibold uppercase tracking-wider text-muted-foreground">Active profile</span>
      <h2 id="quick-prompts-settings-title" class="mt-0.5 text-base font-semibold text-foreground">Quick prompts</h2>
      <p class="mt-1 text-xs text-muted-foreground">Save reusable text for insertion into the selected agent terminal.</p>
    </div>
    <Button size="sm" disabled={!connected} onclick={() => (editing = 'new')}>
      <PlusIcon size={14} strokeWidth={1.8} />New prompt
    </Button>
  </header>

  {#if snapshot.loading && snapshot.prompts.length === 0}
    <div class="grid min-h-28 place-content-center text-xs text-muted-foreground" aria-live="polite">Loading quick prompts…</div>
  {:else if snapshot.prompts.length === 0}
    <div class="flex min-h-28 items-center justify-between gap-4 p-4">
      <div>
        <strong class="text-sm text-foreground">No quick prompts saved</strong>
        <p class="mt-1 text-xs text-muted-foreground">Create one, then open the palette with ⌘⇧P.</p>
      </div>
      <Button variant="outline" size="sm" disabled={!connected} onclick={() => (editing = 'new')}>Add the first prompt</Button>
    </div>
  {:else}
    <div class="divide-y divide-border">
      {#each snapshot.prompts as prompt, index (prompt.id)}
        <article class="grid min-h-12 grid-cols-[auto_minmax(0,1fr)_auto] items-center gap-2 px-2 py-1.5">
          <div class="flex" aria-label={`Reorder ${prompt.name}`}>
            <IconButton
              label={`Move ${prompt.name} up`}
              disabled={!connected || busyId !== null || index === 0}
              onclick={() => void move(prompt, -1)}
            >
              {#snippet icon()}<ArrowUpIcon size={14} strokeWidth={1.8} />{/snippet}
            </IconButton>
            <IconButton
              label={`Move ${prompt.name} down`}
              disabled={!connected || busyId !== null || index === snapshot.prompts.length - 1}
              onclick={() => void move(prompt, 1)}
            >
              {#snippet icon()}<ArrowDownIcon size={14} strokeWidth={1.8} />{/snippet}
            </IconButton>
          </div>
          <div class="min-w-0">
            <strong class="block truncate text-sm font-semibold text-foreground">{prompt.name}</strong>
            <span class="mt-0.5 block truncate font-mono text-xs text-muted-foreground">{quickPromptPreview(prompt.body)}</span>
          </div>
          <div class="flex gap-0.5">
            <IconButton label={`Edit ${prompt.name}`} disabled={!connected || busyId !== null} onclick={() => (editing = prompt)}>
              {#snippet icon()}<PencilIcon size={14} strokeWidth={1.8} />{/snippet}
            </IconButton>
            <IconButton label={`Delete ${prompt.name}`} disabled={!connected || busyId !== null} onclick={() => (removeRequest = prompt)}>
              {#snippet icon()}<Trash2Icon size={14} strokeWidth={1.8} />{/snippet}
            </IconButton>
          </div>
        </article>
      {/each}
    </div>
  {/if}
</section>

{#if editing}
  <QuickPromptEditor
    {store}
    prompt={editing === 'new' ? null : editing}
    {onError}
    onClose={() => (editing = null)}
  />
{/if}

{#if removeRequest}
  <ConfirmationDialog
    title={`Delete ${removeRequest.name}?`}
    description="This quick prompt will be removed from the active profile."
    confirmLabel="Delete prompt"
    onConfirm={() => void remove()}
    onClose={() => (removeRequest = null)}
  />
{/if}
