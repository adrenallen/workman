<script lang="ts">
  import FileTextIcon from '@lucide/svelte/icons/file-text';
  import { Button } from '$lib/components/ui/button';
  import * as Command from '$lib/components/ui/command';
  import * as Dialog from '$lib/components/ui/dialog';
  import QuickPromptEditor from './QuickPromptEditor.svelte';
  import type { DaemonClient } from './daemon';
  import {
    filterQuickPrompts,
    quickPromptPaletteAction,
    quickPromptPreview
  } from './quickPromptPalette';
  import {
    getQuickPromptsStore,
    type QuickPrompt,
    type QuickPromptsSnapshot
  } from './quickPrompts';

  interface Props {
    client: DaemonClient;
    canInsert: boolean;
    onInsert: (prompt: QuickPrompt, submit: boolean) => boolean;
    onClose: () => void;
    onError: (message: string) => void;
  }

  let { client, canInsert, onInsert, onClose, onError }: Props = $props();
  let store = $derived(getQuickPromptsStore(client));
  let snapshot = $state<QuickPromptsSnapshot>({ prompts: [], loading: false, error: null });
  let query = $state('');
  let selectedValue = $state('');
  let editorOpen = $state(false);
  let insertFailed = $state(false);
  let filtered = $derived(filterQuickPrompts(snapshot.prompts, query));
  let activePrompt = $derived(
    filtered.find((prompt) => String(prompt.id) === selectedValue) ?? filtered[0] ?? null
  );

  $effect(() => {
    snapshot = store.current();
    return store.subscribe((next) => (snapshot = next));
  });

  $effect(() => {
    void store.refresh().catch((cause) => onError(message(cause)));
  });

  $effect(() => {
    if (!filtered.some((prompt) => String(prompt.id) === selectedValue)) {
      selectedValue = filtered[0] ? String(filtered[0].id) : '';
    }
  });

  function choose(prompt: QuickPrompt | null, submit: boolean): void {
    if (!prompt || !canInsert) return;
    insertFailed = !onInsert(prompt, submit);
  }

  function handleKeydown(event: KeyboardEvent): void {
    const action = quickPromptPaletteAction(event);
    if (!action) return;
    event.preventDefault();
    event.stopPropagation();
    if (action === 'new') {
      editorOpen = true;
      return;
    }
    choose(activePrompt, action === 'insert-and-send');
  }

  function message(cause: unknown): string {
    return cause instanceof Error ? cause.message : String(cause);
  }

  function retry(): void {
    void store.refresh(true).catch((cause) => onError(message(cause)));
  }
</script>

{#if !editorOpen}
  <Dialog.Root open onOpenChange={(open) => { if (!open) onClose(); }}>
    <Dialog.Content
      class="w-[min(720px,calc(100vw-24px))] !max-w-none gap-0 overflow-hidden rounded-lg border border-border bg-popover p-0 shadow-2xl"
      showCloseButton={false}
      aria-labelledby="quick-prompt-palette-title"
    >
      <Command.Root
        bind:value={selectedValue}
        shouldFilter={false}
        loop
        class="max-h-[min(560px,calc(100dvh-32px))] rounded-lg p-0"
        label="Quick prompts"
      >
        <header class="flex min-h-12 items-center justify-between gap-3 border-b border-border px-3 py-2">
          <div class="flex min-w-0 items-center gap-2">
            <span class="grid size-7 place-items-center border border-border bg-card text-muted-foreground" aria-hidden="true">
              <FileTextIcon size={14} strokeWidth={1.8} />
            </span>
            <div class="min-w-0">
              <strong id="quick-prompt-palette-title" class="block text-sm font-semibold text-foreground">Quick prompts</strong>
              <small class="block text-xs text-muted-foreground">Insert saved text into the selected agent</small>
            </div>
          </div>
          <kbd class="rounded border border-border bg-accent px-1.5 py-0.5 font-mono text-xs text-muted-foreground">⌘⇧P</kbd>
        </header>

        <div class="border-b border-border p-2">
          <Command.Input
            bind:value={query}
            placeholder="Search prompt names and text"
            aria-label="Search quick prompts"
            onkeydown={handleKeydown}
          />
        </div>

        {#if !canInsert || insertFailed}
          <div class="border-b border-border bg-muted px-3 py-2 text-xs font-medium text-foreground" role="status">
            {insertFailed ? 'The selected agent terminal is no longer available' : 'Select a running agent first'}
          </div>
        {/if}

        <div class="flex min-h-7 items-center justify-between border-b border-border px-3 py-1 text-xs text-muted-foreground" aria-live="polite">
          <span>{query ? `${filtered.length} match${filtered.length === 1 ? '' : 'es'}` : `${snapshot.prompts.length} saved`}</span>
          {#if snapshot.loading}<span>Refreshing…</span>{/if}
        </div>

        <Command.List class="min-h-28 max-h-80 p-1.5">
          {#each filtered as prompt (prompt.id)}
            <Command.Item
              value={String(prompt.id)}
              keywords={[prompt.name, prompt.body]}
              class="min-h-12 items-start gap-2 px-2 py-2"
              onSelect={() => choose(prompt, false)}
            >
              <span class="mt-0.5 grid size-6 shrink-0 place-items-center border border-border bg-card text-muted-foreground" aria-hidden="true">
                <FileTextIcon size={13} strokeWidth={1.8} />
              </span>
              <span class="min-w-0 flex-1">
                <strong class="block truncate text-sm font-semibold text-foreground">{prompt.name}</strong>
                <small class="mt-0.5 block truncate font-mono text-xs text-muted-foreground">{quickPromptPreview(prompt.body)}</small>
              </span>
            </Command.Item>
          {:else}
            {#if snapshot.error}
              <div class="grid min-h-28 place-content-center gap-1 text-center">
                <strong class="text-sm text-foreground">Quick prompts unavailable</strong>
                <span class="text-xs text-muted-foreground">{snapshot.error}</span>
                <Button class="mt-2 justify-self-center" size="sm" onclick={retry}>Retry</Button>
              </div>
            {:else}
              <div class="grid min-h-28 place-content-center gap-1 text-center">
                <strong class="text-sm text-foreground">{query ? 'No matching prompt' : 'No quick prompts saved'}</strong>
                <span class="text-xs text-muted-foreground">Press ⌘N to create one.</span>
                <Button class="mt-2 justify-self-center" size="sm" onclick={() => (editorOpen = true)}>New quick prompt</Button>
              </div>
            {/if}
          {/each}
        </Command.List>

        <footer class="flex min-h-9 flex-wrap items-center gap-x-3 gap-y-1 border-t border-border px-3 py-1.5 text-xs text-muted-foreground">
          <span>Enter · insert</span>
          <span>⌘Enter · insert &amp; send</span>
          <span>⌘N · new quick prompt</span>
          <span class="ml-auto">Esc · close</span>
        </footer>
      </Command.Root>
    </Dialog.Content>
  </Dialog.Root>
{:else}
  <QuickPromptEditor
    {store}
    {onError}
    onSaved={() => (query = '')}
    onClose={() => (editorOpen = false)}
  />
{/if}
