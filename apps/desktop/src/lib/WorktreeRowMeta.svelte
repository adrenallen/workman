<script lang="ts">
  import CircleAlertIcon from '@lucide/svelte/icons/circle-alert';
  import GitPullRequestIcon from '@lucide/svelte/icons/git-pull-request';
  import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';

  import IconButton from '$lib/components/ds/IconButton.svelte';
  import TooltipLabel from '$lib/components/ds/TooltipLabel.svelte';
  import * as Popover from '$lib/components/ui/popover';
  import { openBrowserUrl } from './openers';
  import PullRequestList from './PullRequestList.svelte';
  import PullRequestStateIcon from './PullRequestStateIcon.svelte';
  import type { PullRequestCache, PullRequestStatus, WorktreeEntry } from './worktrees';
  import { pullRequestInteraction, pullRequestLabel, pullRequestsForWorktree } from './worktrees';

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

  let pullRequests = $derived(pullRequestsForWorktree(entry));
  let pullRequest = $derived(pullRequests[0] ?? null);
  let pullRequestMode = $derived(pullRequestInteraction(pullRequests));
  let pullRequestUnavailable = $derived(entry !== null && pullRequestCache?.available === false);
  let pullRequestUnavailableLabel = $derived(
    `PR status unavailable — ${pullRequestCache?.error?.trim() || 'lookup failed without a reason'}`
  );
  let openingPullRequest = $state(false);
  let pullRequestListOpen = $state(false);

  async function openPullRequest(target: PullRequestStatus): Promise<void> {
    if (openingPullRequest) return;
    pullRequestListOpen = false;
    openingPullRequest = true;
    try {
      await openBrowserUrl(target.url);
    } catch (cause) {
      console.warn('Could not open pull request', cause);
    } finally {
      openingPullRequest = false;
    }
  }
</script>

<span class="worktree-meta">
  {#if pullRequest}
    {#if pullRequestMode === 'direct'}
      <IconButton
        class="size-6 border border-border bg-card"
        label={`Open ${pullRequestLabel(pullRequest)}`}
        disabled={openingPullRequest}
        onclick={(event) => { event.stopPropagation(); void openPullRequest(pullRequest); }}
      >
        {#snippet icon()}
          <PullRequestStateIcon state={pullRequest.state} size={13} strokeWidth={1.9} />
        {/snippet}
      </IconButton>
    {:else}
      <span class="pull-request-picker">
        <Popover.Root bind:open={pullRequestListOpen}>
          <Popover.Trigger>
            {#snippet child({ props })}
              <IconButton
                {...props}
                class="size-6 border border-border bg-card"
                label={`Show ${pullRequests.length} pull requests for ${entry?.branch ?? 'this branch'}`}
              >
                {#snippet icon()}
                  <span class="multi-pr-icon">
                    <PullRequestStateIcon state={pullRequest.state} size={13} strokeWidth={1.9} />
                    <span>{pullRequests.length}</span>
                  </span>
                {/snippet}
              </IconButton>
            {/snippet}
          </Popover.Trigger>
          <Popover.Content side="right" align="start" sideOffset={6} class="w-80 gap-0 p-2">
            <PullRequestList
              branch={entry?.branch ?? ''}
              {pullRequests}
              onChoose={(target) => void openPullRequest(target)}
            />
          </Popover.Content>
        </Popover.Root>
      </span>
    {/if}
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
  .pull-request-picker { display: inline-flex; }
  .multi-pr-icon { position: relative; display: inline-flex; }
  .multi-pr-icon > span:last-child { position: absolute; right: -7px; bottom: -6px; display: grid; min-width: 12px; height: 12px; place-items: center; border: 1px solid var(--card); border-radius: 999px; padding: 0 2px; background: var(--foreground); color: var(--background); font-family: var(--terminal-font-family); font-size: 8px; font-weight: 750; line-height: 1; }
  .no-pull-request { display: inline-flex; width: 24px; height: 24px; cursor: default; align-items: center; justify-content: center; color: var(--muted-foreground); opacity: .72; }
  :global(.spin) { animation: worktree-spin 800ms linear infinite; }
  @media (prefers-reduced-motion: reduce) { :global(.spin) { animation: none; } }
  @keyframes worktree-spin { to { transform: rotate(360deg); } }
</style>
