<script lang="ts">
  import ArchiveIcon from '@lucide/svelte/icons/archive';
  import ArchiveRestoreIcon from '@lucide/svelte/icons/archive-restore';
  import Mic2Icon from '@lucide/svelte/icons/mic-2';
  import SearchIcon from '@lucide/svelte/icons/search';
  import Trash2Icon from '@lucide/svelte/icons/trash-2';
  import XIcon from '@lucide/svelte/icons/x';

  import { Button } from '$lib/components/ui/button';
  import * as Tabs from '$lib/components/ui/tabs';
  import StatusIndicator from './components/ds/StatusIndicator.svelte';
  import type { Project } from './daemon';
  import {
    feedbackDuration,
    feedbackStatusLabel,
    recordedFeedbackForView,
    type RecordedFeedbackSummary,
    type RecordedFeedbackView
  } from './recordedFeedback';
  import SectionOverview from './SectionOverview.svelte';

  interface Props {
    feedback: RecordedFeedbackSummary[];
    view: RecordedFeedbackView;
    busyId: number | null;
    onOpen: (feedback: RecordedFeedbackSummary) => void;
    onViewChange: (view: RecordedFeedbackView) => void;
    onRecord: () => void;
    onArchive: (feedback: RecordedFeedbackSummary, archived: boolean) => void;
    onDelete: (feedback: RecordedFeedbackSummary) => void;
    project?: Project | null;
  }

  let {
    feedback,
    view,
    busyId,
    onOpen,
    onViewChange,
    onRecord,
    onArchive,
    onDelete,
    project = null
  }: Props = $props();

  let query = $state('');
  let activeCount = $derived(feedback.filter((item) => !item.archived).length);
  let archivedCount = $derived(feedback.filter((item) => item.archived).length);
  let sourceCount = $derived(view === 'active' ? activeCount : archivedCount);
  let visibleFeedback = $derived(recordedFeedbackForView(feedback, view, query));

  function formatUpdated(epochMillis: number): string {
    return new Intl.DateTimeFormat(undefined, {
      month: 'short',
      day: 'numeric',
      year: new Date(epochMillis).getFullYear() === new Date().getFullYear() ? undefined : 'numeric',
      hour: 'numeric',
      minute: '2-digit'
    }).format(new Date(epochMillis));
  }

  function statusTone(item: RecordedFeedbackSummary): 'danger' | 'success' | 'neutral' {
    if (item.status === 'failed') return 'danger';
    if (item.status === 'recording' || item.status === 'transcribing') return 'success';
    return 'neutral';
  }
</script>

<SectionOverview
  ariaLabel="Recorded feedback browser"
  eyebrow="Local capture library"
  title="Feedback"
  description="Find active recordings or revisit feedback you archived earlier."
  summaryLayout="split"
  {project}
>
  {#snippet icon()}<Mic2Icon strokeWidth={1.8} />{/snippet}
  {#snippet action()}
    <Button size="sm" onclick={onRecord}><Mic2Icon size={14} />Record feedback</Button>
  {/snippet}

  {#snippet controls()}
    <div class="browser-controls">
      <Tabs.Root value={view} onValueChange={(value) => onViewChange(value as RecordedFeedbackView)}>
        <Tabs.List variant="line" class="feedback-tabs" aria-label="Feedback views">
          <Tabs.Trigger value="active">Active <span>{activeCount}</span></Tabs.Trigger>
          <Tabs.Trigger value="archived">Archived <span>{archivedCount}</span></Tabs.Trigger>
        </Tabs.List>
      </Tabs.Root>

      <label class="search-field">
        <SearchIcon size={14} strokeWidth={1.8} aria-hidden="true" />
        <input bind:value={query} aria-label="Search feedback" placeholder="Search title, status, or ID" />
        {#if query}
          <button type="button" aria-label="Clear feedback search" title="Clear search" onclick={() => (query = '')}>
            <XIcon size={13} aria-hidden="true" />
          </button>
        {/if}
      </label>
    </div>
  {/snippet}

  {#snippet summary()}
    <span>{visibleFeedback.length} of {sourceCount} {view}</span>
    {#if query}<button class="summary-reset" type="button" onclick={() => (query = '')}>Clear search</button>{/if}
  {/snippet}

  <div class="feedback-ledger" aria-live="polite">
    {#each visibleFeedback as item (item.id)}
      <article class="feedback-row" class:busy={busyId === item.id}>
        <button
          class="feedback-link"
          type="button"
          disabled={busyId !== null}
          title={`Open feedback #${item.id}: ${item.title}`}
          onclick={() => onOpen(item)}
        >
          <StatusIndicator
            tone={statusTone(item)}
            state={item.status === 'recording' || item.status === 'transcribing' ? 'working' : item.status === 'failed' ? 'crashed' : 'idle'}
            label={feedbackStatusLabel(item.status)}
          />
          <span class="feedback-copy">
            <strong>{item.title}</strong>
            <small>
              <span>#{item.id} · {feedbackStatusLabel(item.status)} · {feedbackDuration(item.duration_ms)}</span>
              <span>{item.snapshot_count} snap{item.snapshot_count === 1 ? '' : 's'} · {formatUpdated(item.updated_at)}</span>
            </small>
          </span>
        </button>

        <div class="row-actions" aria-label={`Actions for feedback #${item.id}`}>
          <Button size="sm" variant="ghost" disabled={busyId !== null} onclick={() => onOpen(item)}>Open</Button>
          <Button
            size="sm"
            variant="ghost"
            disabled={busyId !== null || item.status === 'recording'}
            onclick={() => onArchive(item, !item.archived)}
          >
            {#if item.archived}<ArchiveRestoreIcon size={14} />Restore{:else}<ArchiveIcon size={14} />Archive{/if}
          </Button>
          <Button
            class="delete-action"
            size="sm"
            variant="ghost"
            disabled={busyId !== null || item.status === 'recording'}
            onclick={() => onDelete(item)}
          ><Trash2Icon size={14} />Delete</Button>
        </div>
      </article>
    {:else}
      <div class="empty-results">
        <span class="empty-icon" aria-hidden="true">
          {#if view === 'archived'}<ArchiveIcon size={22} strokeWidth={1.6} />{:else}<Mic2Icon size={22} strokeWidth={1.6} />{/if}
        </span>
        <strong>{query ? 'No feedback matches this search' : view === 'active' ? 'No active feedback' : 'No archived feedback'}</strong>
        <p>{query ? 'Try a different title, status, or reference number.' : view === 'active' ? 'Record feedback while you walk through the screen and explain what should change.' : 'Archived feedback stays available here when you need it again.'}</p>
        {#if query}
          <Button size="sm" variant="outline" onclick={() => (query = '')}>Clear search</Button>
        {:else if view === 'active'}
          <Button size="sm" onclick={onRecord}>Record feedback</Button>
        {:else}
          <Button size="sm" variant="outline" onclick={() => onViewChange('active')}>View active feedback</Button>
        {/if}
      </div>
    {/each}
  </div>
</SectionOverview>

<style>
  .summary-reset, .feedback-copy small { font-family: var(--terminal-font-family); }
  .browser-controls { display: flex; min-height: 42px; align-items: center; justify-content: space-between; gap: var(--space-4); padding: 5px 10px; }
  .browser-controls :global(.feedback-tabs) { min-width: 240px; }
  .browser-controls :global([data-slot='tabs-trigger'] span) { margin-left: 5px; color: var(--muted-foreground); font-family: var(--terminal-font-family); font-size: var(--font-size-xs); }
  .search-field { display: flex; width: min(320px, 44%); height: 28px; align-items: center; gap: 5px; border: 1px solid var(--input); border-radius: var(--radius); padding: 0 7px; background: var(--background); color: var(--muted-foreground); }
  .search-field input { min-width: 0; flex: 1; border: 0; outline: 0; padding: 0; background: transparent; color: var(--foreground); font-size: var(--font-size-sm); }
  .search-field button { display: grid; width: 20px; height: 20px; place-items: center; border: 0; padding: 0; background: transparent; color: var(--muted-foreground); cursor: pointer; }
  .search-field:focus-within { border-color: var(--ring); box-shadow: 0 0 0 1px var(--ring); }
  .summary-reset { border: 0; padding: 3px 0; background: transparent; color: var(--ring); font-size: inherit; cursor: pointer; }
  .feedback-ledger { min-height: 0; overflow-y: auto; padding: 4px 7px max(14px, env(safe-area-inset-bottom)); scrollbar-color: var(--border-strong) transparent; scrollbar-width: thin; }
  .feedback-row { display: grid; min-height: 54px; grid-template-columns: minmax(220px, 1fr) auto; align-items: center; gap: var(--space-2); border-bottom: 1px solid var(--border); padding: 5px 4px 5px 9px; }
  .feedback-row:hover { background: var(--popover); }
  .feedback-row.busy { opacity: 0.64; }
  .feedback-link { display: grid; min-width: 0; grid-template-columns: 16px minmax(0, 1fr); align-items: center; gap: 9px; border: 0; padding: 4px 0; background: transparent; color: var(--foreground); text-align: left; cursor: pointer; }
  .feedback-link:disabled { cursor: default; }
  .feedback-link:focus-visible { border-radius: 4px; outline: 2px solid var(--ring); outline-offset: 2px; }
  .feedback-copy { min-width: 0; }
  .feedback-copy strong, .feedback-copy small { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .feedback-copy strong { font-size: var(--font-size-sm); font-weight: 590; }
  .feedback-copy small { display: flex; gap: var(--space-3); margin-top: 2px; color: var(--muted-foreground); font-size: var(--font-size-xs); }
  .row-actions { display: flex; align-items: center; gap: 2px; }
  .row-actions :global(button) { min-width: auto; }
  .row-actions :global(.delete-action) { color: var(--destructive); }
  .empty-results { display: grid; min-height: 240px; place-content: center; justify-items: center; text-align: center; }
  .empty-icon { display: grid; width: 38px; height: 38px; place-items: center; margin-bottom: var(--space-2); border: 1px solid var(--border-strong); border-radius: var(--radius); background: var(--card); color: var(--muted-foreground); }
  .empty-results strong { font-size: var(--font-size-base); }
  .empty-results p { max-width: 420px; margin: 5px 0 11px; color: var(--muted-foreground); font-size: var(--font-size-sm); }

  @container (max-width: 760px) {
    .browser-controls { align-items: stretch; flex-direction: column; gap: 5px; }
    .search-field { width: 100%; }
    .feedback-row { grid-template-columns: minmax(0, 1fr); padding-block: 7px; }
    .row-actions { justify-content: flex-start; padding-left: 25px; }
    .feedback-copy small { align-items: flex-start; flex-direction: column; gap: 1px; }
  }
</style>
