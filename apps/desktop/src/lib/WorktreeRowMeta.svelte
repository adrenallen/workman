<script lang="ts">
  import GitMergeIcon from '@lucide/svelte/icons/git-merge';
  import CircleAlertIcon from '@lucide/svelte/icons/circle-alert';
  import GitPullRequestIcon from '@lucide/svelte/icons/git-pull-request';
  import GitPullRequestClosedIcon from '@lucide/svelte/icons/git-pull-request-closed';
  import GitPullRequestDraftIcon from '@lucide/svelte/icons/git-pull-request-draft';
  import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';

  import IconButton from '$lib/components/ds/IconButton.svelte';
  import TooltipLabel from '$lib/components/ds/TooltipLabel.svelte';
  import { openBrowserUrl } from './openers';
  import type { PullRequestCache, WorktreeEntry } from './worktrees';
  import { pullRequestLabel, pullRequestTone } from './worktrees';

  interface Props {
    entry: WorktreeEntry | null;
    pullRequestCache: PullRequestCache | null;
    repositoryName: string;
    refreshing?: boolean;
    showRefresh?: boolean;
    onRefresh: () => void;
  }

  let {
    entry,
    pullRequestCache,
    repositoryName,
    refreshing = false,
    showRefresh = false,
    onRefresh
  }: Props = $props();

  let pullRequest = $derived(entry?.pull_request ?? null);
  let pullRequestUnavailable = $derived(entry !== null && pullRequestCache?.available === false);
  let pullRequestUnavailableLabel = $derived(
    `PR status unavailable — ${pullRequestCache?.error?.trim() || 'lookup failed without a reason'}`
  );
  let openingPullRequest = $state(false);
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

  async function openPullRequest(): Promise<void> {
    if (!pullRequest || openingPullRequest) return;
    openingPullRequest = true;
    try {
      await openBrowserUrl(pullRequest.url);
    } catch (cause) {
      console.warn('Could not open pull request', cause);
    } finally {
      openingPullRequest = false;
    }
  }
</script>

<span class="worktree-meta">
  {#if pullRequest}
    <IconButton
      class={`size-6 border border-border bg-card ${pullRequestClass}`}
      label={`Open ${pullRequestLabel(pullRequest)}`}
      disabled={openingPullRequest}
      onclick={(event) => { event.stopPropagation(); void openPullRequest(); }}
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
  {:else if pullRequestUnavailable}
    <IconButton
      class="size-6 text-warning"
      label={pullRequestUnavailableLabel}
      disabled={refreshing}
      onclick={(event) => { event.stopPropagation(); onRefresh(); }}
    >
      {#snippet icon()}<CircleAlertIcon size={13} strokeWidth={1.9} />{/snippet}
    </IconButton>
  {:else if entry && pullRequestCache?.available === true}
    {@const noPullRequestLabel = `No pull request for ${entry.branch}`}
    <TooltipLabel label={noPullRequestLabel}>
      <span class="no-pull-request" aria-label={noPullRequestLabel}>
        <GitPullRequestIcon size={13} strokeWidth={1.8} aria-hidden="true" />
      </span>
    </TooltipLabel>
  {/if}
  {#if showRefresh && !pullRequestUnavailable}
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
  .no-pull-request { display: inline-flex; width: 24px; height: 24px; cursor: default; align-items: center; justify-content: center; color: var(--muted-foreground); opacity: .72; }
  :global(.spin) { animation: worktree-spin 800ms linear infinite; }
  @media (prefers-reduced-motion: reduce) { :global(.spin) { animation: none; } }
  @keyframes worktree-spin { to { transform: rotate(360deg); } }
</style>
