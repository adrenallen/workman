<script lang="ts">
  import GitMergeIcon from '@lucide/svelte/icons/git-merge';
  import GitPullRequestIcon from '@lucide/svelte/icons/git-pull-request';
  import GitPullRequestClosedIcon from '@lucide/svelte/icons/git-pull-request-closed';
  import GitPullRequestDraftIcon from '@lucide/svelte/icons/git-pull-request-draft';

  import type { PullRequestState } from './worktrees';
  import { pullRequestVisual } from './worktrees';

  interface Props {
    state: PullRequestState;
    size?: number;
    strokeWidth?: number;
  }

  let { state, size = 14, strokeWidth = 1.9 }: Props = $props();
  let visual = $derived(pullRequestVisual(state));
</script>

<span class="pull-request-state-icon" style:color={visual.color} data-state={state} aria-hidden="true">
  {#if visual.icon === 'git-merge'}
    <GitMergeIcon {size} {strokeWidth} />
  {:else if visual.icon === 'git-pull-request-closed'}
    <GitPullRequestClosedIcon {size} {strokeWidth} />
  {:else if visual.icon === 'git-pull-request-draft'}
    <GitPullRequestDraftIcon {size} {strokeWidth} />
  {:else}
    <GitPullRequestIcon {size} {strokeWidth} />
  {/if}
</span>

<style>
  .pull-request-state-icon { display: inline-flex; flex: none; align-items: center; justify-content: center; }
</style>
