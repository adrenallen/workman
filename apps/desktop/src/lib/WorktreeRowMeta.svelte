<script lang="ts">
  import CircleAlertIcon from '@lucide/svelte/icons/circle-alert';
  import GitPullRequestIcon from '@lucide/svelte/icons/git-pull-request';
  import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';

  import IconButton from '$lib/components/ds/IconButton.svelte';
  import * as Popover from '$lib/components/ui/popover';
  import { openBrowserUrl } from './openers';
  import PullRequestList from './PullRequestList.svelte';
  import PullRequestStateIcon from './PullRequestStateIcon.svelte';
  import type { PullRequestCache, PullRequestStatus, WorktreeEntry } from './worktrees';
  import { pullRequestsForWorktree } from './worktrees';

  interface Props {
    entry: WorktreeEntry | null;
    pullRequestCache: PullRequestCache | null;
    projectId: number;
    repositoryName: string;
    openPopoverKey: string | null;
    onOpenPopoverChange: (key: string | null) => void;
    refreshing?: boolean;
    showNoPullRequest?: boolean;
    showRefresh?: boolean;
    compact?: boolean;
    onRefresh: () => void;
  }

  let {
    entry,
    pullRequestCache,
    projectId,
    repositoryName,
    openPopoverKey,
    onOpenPopoverChange,
    refreshing = false,
    showNoPullRequest = true,
    showRefresh = false,
    compact = false,
    onRefresh
  }: Props = $props();

  let pullRequests = $derived(pullRequestsForWorktree(entry));
  let pullRequest = $derived(pullRequests[0] ?? null);
  let pullRequestUnavailable = $derived(entry !== null && pullRequestCache?.available === false);
  let pullRequestUnavailableLabel = $derived(
    `PR status unavailable — ${pullRequestCache?.error?.trim() || 'lookup failed without a reason'}`
  );
  let openingPullRequest = $state(false);
  let popoverKey = $derived(`${projectId}:pull-request`);
  let popoverOpen = $derived(openPopoverKey === popoverKey);

  async function openPullRequest(target: PullRequestStatus): Promise<void> {
    if (openingPullRequest) return;
    onOpenPopoverChange(null);
    openingPullRequest = true;
    try {
      await openBrowserUrl(target.url);
    } catch (cause) {
      console.warn('Could not open pull request', cause);
    } finally {
      openingPullRequest = false;
    }
  }

  function changeOpen(open: boolean): void {
    if (open) {
      onOpenPopoverChange(popoverKey);
    } else if (popoverOpen) {
      onOpenPopoverChange(null);
    }
  }

  function togglePopover(event: MouseEvent): void {
    event.stopPropagation();
    onOpenPopoverChange(popoverOpen ? null : popoverKey);
  }
</script>

<span class:compact class="worktree-meta" data-project-pr-compact={compact ? 'true' : undefined}>
  {#if pullRequest}
    <span class="pull-request-picker">
      <Popover.Root open={popoverOpen} onOpenChange={changeOpen}>
        <Popover.Trigger>
          {#snippet child({ props })}
            <IconButton
              {...props}
              class={compact ? 'project-pr-pip' : 'size-5 border border-border bg-card'}
              label={`Show ${pullRequests.length} pull request${pullRequests.length === 1 ? '' : 's'} for ${entry?.branch ?? 'this branch'}`}
              tooltip={false}
              disabled={openingPullRequest}
              aria-expanded={popoverOpen}
              data-project-pr-trigger
              onclick={togglePopover}
            >
              {#snippet icon()}
                {#if compact}
                  <span class="pr-pip-mark" data-state={pullRequest.state} aria-hidden="true"></span>
                {:else if pullRequests.length === 1}
                  <PullRequestStateIcon state={pullRequest.state} size={13} strokeWidth={1.9} />
                {:else}
                  <span class="multi-pr-icon">
                    <PullRequestStateIcon state={pullRequest.state} size={13} strokeWidth={1.9} />
                    <span>{pullRequests.length}</span>
                  </span>
                {/if}
              {/snippet}
            </IconButton>
          {/snippet}
        </Popover.Trigger>
        <Popover.Content
          side="right"
          align="start"
          sideOffset={6}
          class="w-80 gap-0 p-2"
          data-project-pr-popover
        >
          <PullRequestList
            branch={entry?.branch ?? ''}
            {pullRequests}
            onChoose={(target) => void openPullRequest(target)}
          />
        </Popover.Content>
      </Popover.Root>
    </span>
  {:else if pullRequestUnavailable}
    <IconButton
      class={compact ? 'project-pr-pip' : 'size-5 text-warning'}
      label={pullRequestUnavailableLabel}
      tooltip={false}
      disabled={refreshing}
      onclick={(event) => { event.stopPropagation(); onRefresh(); }}
    >
      {#snippet icon()}
        {#if compact}<span class="pr-pip-mark unavailable" aria-hidden="true"></span>{:else}<CircleAlertIcon size={13} strokeWidth={1.9} />{/if}
      {/snippet}
    </IconButton>
  {:else if showNoPullRequest && entry && pullRequestCache?.available === true}
    {@const noPullRequestLabel = `No pull request for ${entry.branch}`}
    <span class="no-pull-request" role="img" aria-label={noPullRequestLabel}>
      <GitPullRequestIcon size={13} strokeWidth={1.8} aria-hidden="true" />
    </span>
  {/if}
  {#if showRefresh && !pullRequestUnavailable}
    <IconButton
      class="size-5 opacity-0 group-hover/repository:opacity-100 focus-visible:opacity-100"
      label={`Refresh pull request status for ${repositoryName}`}
      tooltip={false}
      disabled={refreshing}
      onclick={(event) => { event.stopPropagation(); onRefresh(); }}
    >
      {#snippet icon()}<RefreshCwIcon class={refreshing ? 'spin' : ''} size={13} strokeWidth={1.8} />{/snippet}
    </IconButton>
  {/if}
</span>

<style>
  .worktree-meta { display: inline-flex; flex: none; align-items: center; gap: var(--space-1); }
  .worktree-meta.compact { height: 8px; gap: 0; pointer-events: none; }
  .worktree-meta.compact :global(.project-pr-pip) { display: grid; width: 8px; min-width: 8px; height: 8px; place-items: center; border: 0; border-radius: 0; padding: 0; background: transparent; pointer-events: auto; }
  .worktree-meta.compact :global(.project-pr-pip:focus-visible) { outline: 1px solid var(--ring); outline-offset: 1px; box-shadow: none; }
  .pr-pip-mark { display: block; width: 5px; height: 5px; border: 1px solid var(--card); border-radius: 999px; background: var(--success); }
  .pr-pip-mark[data-state='merged'] { background: var(--pull-request-merged); }
  .pr-pip-mark[data-state='draft'] { background: var(--muted-foreground); opacity: .7; }
  .pr-pip-mark[data-state='closed'] { background: var(--destructive); }
  .pr-pip-mark.unavailable { background: var(--warning-token); }
  .pull-request-picker { display: inline-flex; }
  .multi-pr-icon { position: relative; display: inline-flex; }
  .multi-pr-icon > span:last-child { position: absolute; top: -6px; right: -7px; display: grid; min-width: 12px; height: 12px; place-items: center; border: 1px solid var(--card); border-radius: 999px; padding: 0 2px; background: var(--foreground); color: var(--background); font-family: var(--terminal-font-family); font-size: 8px; font-weight: 750; line-height: 1; }
  .no-pull-request { display: inline-flex; width: 20px; height: 20px; cursor: default; align-items: center; justify-content: center; color: var(--muted-foreground); opacity: .72; }
  :global(.spin) { animation: worktree-spin 800ms linear infinite; }
  @media (prefers-reduced-motion: reduce) { :global(.spin) { animation: none; } }
  @keyframes worktree-spin { to { transform: rotate(360deg); } }
</style>
