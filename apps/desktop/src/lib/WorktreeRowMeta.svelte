<script lang="ts">
  import GitMergeIcon from '@lucide/svelte/icons/git-merge';
  import GitPullRequestIcon from '@lucide/svelte/icons/git-pull-request';
  import GitPullRequestClosedIcon from '@lucide/svelte/icons/git-pull-request-closed';
  import GitPullRequestDraftIcon from '@lucide/svelte/icons/git-pull-request-draft';
  import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';

  import IconButton from '$lib/components/ds/IconButton.svelte';
  import type { WorktreeEntry } from './worktrees';
  import { pullRequestLabel, pullRequestTone } from './worktrees';

  interface Props {
    entry: WorktreeEntry | null;
    repositoryName: string;
    refreshing?: boolean;
    showRefresh?: boolean;
    onRefresh: () => void;
  }

  let {
    entry,
    repositoryName,
    refreshing = false,
    showRefresh = false,
    onRefresh
  }: Props = $props();

  let pullRequest = $derived(entry?.pull_request ?? null);
  let pullRequestToneName = $derived(pullRequest ? pullRequestTone(pullRequest) : 'neutral');
  let pullRequestClass = $derived(
    pullRequestToneName === 'success'
      ? 'text-success'
      : pullRequestToneName === 'warning'
        ? 'text-warning'
        : pullRequestToneName === 'danger'
          ? 'text-destructive'
          : 'text-muted-foreground'
  );

  function openUrl(url: string): void {
    window.open(url, '_blank', 'noopener,noreferrer');
  }
</script>

<span class="worktree-meta">
  {#if pullRequest}
    <IconButton
      class={`size-6 border border-border bg-card ${pullRequestClass}`}
      label={pullRequestLabel(pullRequest)}
      onclick={(event) => { event.stopPropagation(); openUrl(pullRequest!.url); }}
    >
      {#snippet icon()}
        {#if pullRequest.state === 'merged'}
          <GitMergeIcon size={13} strokeWidth={1.9} />
        {:else if pullRequest.state === 'closed'}
          <GitPullRequestClosedIcon size={13} strokeWidth={1.9} />
        {:else if pullRequest.state === 'draft'}
          <GitPullRequestDraftIcon size={13} strokeWidth={1.9} />
        {:else}
          <GitPullRequestIcon size={13} strokeWidth={1.9} />
        {/if}
      {/snippet}
    </IconButton>
  {/if}
  {#if showRefresh}
    <IconButton
      class="size-6 opacity-0 group-hover/repository:opacity-100 focus-visible:opacity-100"
      label={`Refresh pull request status for ${repositoryName}`}
      disabled={refreshing}
      onclick={(event) => { event.stopPropagation(); onRefresh(); }}
    >
      {#snippet icon()}<RefreshCwIcon class={refreshing ? 'spin' : ''} size={13} strokeWidth={1.8} />{/snippet}
    </IconButton>
  {/if}
</span>

<style>
  .worktree-meta { display: inline-flex; flex: none; align-items: center; gap: var(--space-1); }
  :global(.spin) { animation: worktree-spin 800ms linear infinite; }
  @media (prefers-reduced-motion: reduce) { :global(.spin) { animation: none; } }
  @keyframes worktree-spin { to { transform: rotate(360deg); } }
</style>
