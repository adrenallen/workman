<script lang="ts">
  import ArchiveIcon from '@lucide/svelte/icons/archive';
  import NotebookTextIcon from '@lucide/svelte/icons/notebook-text';
  import SearchIcon from '@lucide/svelte/icons/search';
  import XIcon from '@lucide/svelte/icons/x';

  import { Button } from '$lib/components/ui/button';
  import * as Tabs from '$lib/components/ui/tabs';
  import SectionOverview from './SectionOverview.svelte';
  import type { ScratchpadSummary } from './coordination';
  import type { Project } from './daemon';

  interface Props {
    scratchpads: ScratchpadSummary[];
    archivedScratchpads: ScratchpadSummary[];
    busyId: number | null;
    onOpen: (scratchpad: ScratchpadSummary) => void;
    onCreate: () => void;
    onRename: (scratchpad: ScratchpadSummary, name: string) => void;
    onArchive: (scratchpad: ScratchpadSummary) => void;
    onDelete: (scratchpad: ScratchpadSummary) => void;
    project?: Project | null;
  }

  let {
    scratchpads,
    archivedScratchpads,
    busyId,
    onOpen,
    onCreate,
    onRename,
    onArchive,
    onDelete,
    project = null
  }: Props = $props();

  let view = $state<'open' | 'archived'>('open');
  let query = $state('');
  let renameId = $state<number | null>(null);
  let renameValue = $state('');

  let source = $derived(view === 'open' ? scratchpads : archivedScratchpads);
  let visibleScratchpads = $derived.by(() => {
    const needle = query.trim().toLowerCase();
    return source.filter((scratchpad) =>
      !needle || `${scratchpad.id} ${scratchpad.name} ${scratchpad.tags.join(' ')}`
        .toLowerCase()
        .includes(needle)
    );
  });

  function beginRename(scratchpad: ScratchpadSummary): void {
    renameId = scratchpad.id;
    renameValue = scratchpad.name;
  }

  function cancelRename(): void {
    renameId = null;
    renameValue = '';
  }

  function submitRename(event: SubmitEvent, scratchpad: ScratchpadSummary): void {
    event.preventDefault();
    const name = renameValue.trim();
    if (!name || name === scratchpad.name) {
      cancelRename();
      return;
    }
    cancelRename();
    onRename(scratchpad, name);
  }

  function focusRename(node: HTMLInputElement): void {
    requestAnimationFrame(() => {
      node.focus();
      node.select();
    });
  }
</script>

<SectionOverview
  ariaLabel="Scratchpads browser"
  eyebrow="Project notebook"
  title="Scratchpads"
  description="Open working notes or revisit archived handoffs."
  summaryLayout="split"
  {project}
>
  {#snippet icon()}<NotebookTextIcon strokeWidth={1.8} />{/snippet}
  {#snippet action()}
    <Button size="sm" onclick={onCreate}>New scratchpad</Button>
  {/snippet}

  {#snippet controls()}
    <div class="browser-controls">
      <Tabs.Root value={view} onValueChange={(value) => (view = value as typeof view)}>
        <Tabs.List variant="line" class="scratchpad-tabs" aria-label="Scratchpad views">
          <Tabs.Trigger value="open">Open <span>{scratchpads.length}</span></Tabs.Trigger>
          <Tabs.Trigger value="archived">Archived <span>{archivedScratchpads.length}</span></Tabs.Trigger>
        </Tabs.List>
      </Tabs.Root>

      <label class="search-field">
        <SearchIcon size={14} strokeWidth={1.8} aria-hidden="true" />
        <input bind:value={query} aria-label="Search scratchpads" placeholder="Search name, tag, or ID" />
        {#if query}
          <button type="button" aria-label="Clear scratchpad search" title="Clear search" onclick={() => (query = '')}>
            <XIcon size={13} aria-hidden="true" />
          </button>
        {/if}
      </label>
    </div>
  {/snippet}

  {#snippet summary()}
    <span>{visibleScratchpads.length} of {source.length} {view}</span>
    {#if query}<button class="summary-reset" type="button" onclick={() => (query = '')}>Clear search</button>{/if}
  {/snippet}

  <div class="scratchpad-ledger" aria-live="polite">
    {#each visibleScratchpads as scratchpad (scratchpad.id)}
      <article class="scratchpad-row" class:busy={busyId === scratchpad.id}>
        <button
          class="scratchpad-link"
          type="button"
          disabled={busyId !== null}
          title={`Open scratchpad #${scratchpad.id}: ${scratchpad.name}`}
          onclick={() => onOpen(scratchpad)}
        >
          <span class="scratchpad-id">#{scratchpad.id}</span>
          <span class="scratchpad-copy">
            <strong>{scratchpad.name}</strong>
            <small>
              <span>revision {scratchpad.revision} · {scratchpad.updated_by}</span>
              {#if scratchpad.tags.length > 0}<span>{scratchpad.tags.join(' · ')}</span>{/if}
            </small>
          </span>
        </button>

        {#if renameId === scratchpad.id}
          <form class="rename-form" onsubmit={(event) => submitRename(event, scratchpad)}>
            <input
              bind:value={renameValue}
              aria-label={`Rename scratchpad #${scratchpad.id}`}
              maxlength="120"
              use:focusRename
            />
            <Button size="sm" type="submit" disabled={!renameValue.trim()}>Save</Button>
            <Button size="sm" variant="ghost" type="button" onclick={cancelRename}>Cancel</Button>
          </form>
        {:else}
          <div class="row-actions" aria-label={`Actions for scratchpad #${scratchpad.id}`}>
            <Button size="sm" variant="ghost" disabled={busyId !== null} onclick={() => onOpen(scratchpad)}>Open</Button>
            <Button size="sm" variant="ghost" disabled={busyId !== null} onclick={() => beginRename(scratchpad)}>Rename</Button>
            {#if !scratchpad.archived}
              <Button size="sm" variant="ghost" disabled={busyId !== null} onclick={() => onArchive(scratchpad)}>Archive</Button>
            {/if}
            <Button class="delete-action" size="sm" variant="ghost" disabled={busyId !== null} onclick={() => onDelete(scratchpad)}>Delete</Button>
          </div>
        {/if}
      </article>
    {:else}
      <div class="empty-results">
        <span class="empty-icon" aria-hidden="true">
          {#if view === 'archived'}<ArchiveIcon size={22} strokeWidth={1.6} />{:else}<NotebookTextIcon size={22} strokeWidth={1.6} />{/if}
        </span>
        <strong>{query ? 'No scratchpads match this search' : view === 'open' ? 'No open scratchpads' : 'No archived scratchpads'}</strong>
        <p>{query ? 'Search by a different name, tag, or reference number.' : view === 'open' ? 'Create a scratchpad for durable notes, plans, and handoffs.' : 'Archived scratchpads will stay available here.'}</p>
        {#if query}
          <Button size="sm" variant="outline" onclick={() => (query = '')}>Clear search</Button>
        {:else if view === 'open'}
          <Button size="sm" onclick={onCreate}>New scratchpad</Button>
        {:else}
          <Button size="sm" variant="outline" onclick={() => (view = 'open')}>View open scratchpads</Button>
        {/if}
      </div>
    {/each}
  </div>
</SectionOverview>

<style>
  .summary-reset, .scratchpad-id, .scratchpad-copy small { font-family: var(--terminal-font-family); }

  .browser-controls { display: flex; min-height: 42px; align-items: center; justify-content: space-between; gap: var(--space-4); padding: 5px 10px; }
  .browser-controls :global(.scratchpad-tabs) { min-width: 240px; }
  .browser-controls :global([data-slot='tabs-trigger'] span) { margin-left: 5px; color: var(--muted-foreground); font-family: var(--terminal-font-family); font-size: var(--font-size-xs); }
  .search-field { display: flex; width: min(300px, 42%); height: 28px; align-items: center; gap: 5px; border: 1px solid var(--input); border-radius: var(--radius); padding: 0 7px; background: var(--background); color: var(--muted-foreground); }
  .search-field input { min-width: 0; flex: 1; border: 0; outline: 0; padding: 0; background: transparent; color: var(--foreground); font-size: var(--font-size-sm); }
  .search-field button { display: grid; width: 20px; height: 20px; place-items: center; border: 0; padding: 0; background: transparent; color: var(--muted-foreground); cursor: pointer; }

  .summary-reset { border: 0; padding: 3px 0; background: transparent; color: var(--ring); font-size: inherit; cursor: pointer; }
  .scratchpad-ledger { min-height: 0; overflow-y: auto; padding: 4px 7px 10px; scrollbar-color: var(--border-strong) transparent; scrollbar-width: thin; }
  .scratchpad-row { display: grid; min-height: 44px; grid-template-columns: minmax(180px, 1fr) auto; align-items: center; gap: var(--space-2); border-bottom: 1px solid var(--border); padding: 3px 4px 3px 8px; }
  .scratchpad-row:hover { background: var(--popover); }
  .scratchpad-row.busy { opacity: 0.64; }
  .scratchpad-link { display: grid; min-width: 0; grid-template-columns: 48px minmax(0, 1fr); align-items: center; gap: var(--space-2); border: 0; padding: 4px 0; background: transparent; color: var(--foreground); text-align: left; cursor: pointer; }
  .scratchpad-link:disabled { cursor: default; }
  .scratchpad-id { color: var(--muted-foreground); font-size: var(--font-size-xs); }
  .scratchpad-copy { min-width: 0; }
  .scratchpad-copy strong, .scratchpad-copy small { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .scratchpad-copy strong { font-size: var(--font-size-sm); font-weight: 590; }
  .scratchpad-copy small { display: flex; gap: var(--space-2); margin-top: 1px; color: var(--muted-foreground); font-size: var(--font-size-xs); }
  .row-actions { display: flex; align-items: center; gap: 2px; }
  .row-actions :global(button) { min-width: auto; }
  .row-actions :global(.delete-action) { color: var(--destructive); }
  .rename-form { display: flex; align-items: center; justify-content: flex-end; gap: var(--space-1); }
  .rename-form input { width: min(260px, 26vw); height: 28px; border: 1px solid var(--input); border-radius: var(--radius); outline: 0; padding: 0 7px; background: var(--background); color: var(--foreground); font-size: var(--font-size-sm); }
  .rename-form input:focus { border-color: var(--ring); box-shadow: 0 0 0 1px var(--ring); }
  .empty-results { display: grid; min-height: 220px; place-content: center; justify-items: center; text-align: center; }
  .empty-icon { display: grid; width: 36px; height: 36px; place-items: center; margin-bottom: var(--space-2); border: 1px solid var(--border-strong); border-radius: var(--radius); color: var(--muted-foreground); background: var(--card); }
  .empty-results strong { font-size: var(--font-size-base); }
  .empty-results p { max-width: 380px; margin: 5px 0 10px; color: var(--muted-foreground); font-size: var(--font-size-sm); }

  @container (max-width: 760px) {
    .browser-controls { align-items: stretch; flex-direction: column; gap: 5px; }
    .search-field { width: 100%; }
    .scratchpad-row { grid-template-columns: minmax(0, 1fr); padding-block: 5px; }
    .row-actions, .rename-form { justify-content: flex-start; padding-left: 56px; }
  }
</style>
