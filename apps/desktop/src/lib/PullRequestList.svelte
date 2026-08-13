<script lang="ts">
  import ExternalLinkIcon from '@lucide/svelte/icons/external-link';

  import PullRequestStateIcon from './PullRequestStateIcon.svelte';
  import type { PullRequestStatus } from './worktrees';

  interface Props {
    branch: string;
    pullRequests: PullRequestStatus[];
    onChoose: (pullRequest: PullRequestStatus) => void;
  }

  let { branch, pullRequests, onChoose }: Props = $props();

  function stateLabel(pullRequest: PullRequestStatus): string {
    return `${pullRequest.state[0].toUpperCase()}${pullRequest.state.slice(1)}`;
  }

  function title(pullRequest: PullRequestStatus): string {
    return pullRequest.title.trim() || `Pull request #${pullRequest.number}`;
  }
</script>

<section class="pull-request-list" aria-label={`${pullRequests.length} pull requests for ${branch}`}>
  <header>
    <strong>Pull requests</strong>
    <span>{pullRequests.length} on {branch}</span>
  </header>
  <div>
    {#each pullRequests as pullRequest (pullRequest.number)}
      <button
        type="button"
        aria-label={`Open pull request #${pullRequest.number}, ${title(pullRequest)}, ${pullRequest.state}`}
        onclick={(event) => { event.stopPropagation(); onChoose(pullRequest); }}
      >
        <PullRequestStateIcon state={pullRequest.state} size={15} strokeWidth={1.9} />
        <span class="copy">
          <strong><span>#{pullRequest.number}</span> {title(pullRequest)}</strong>
          <small>{stateLabel(pullRequest)}</small>
        </span>
        <ExternalLinkIcon size={13} strokeWidth={1.8} aria-hidden="true" />
      </button>
    {/each}
  </div>
</section>

<style>
  .pull-request-list { min-width: 0; }
  header { display: flex; min-width: 0; align-items: baseline; justify-content: space-between; gap: var(--space-3); border-bottom: 1px solid var(--border); padding: 2px 3px 7px; }
  header strong { color: var(--popover-foreground); font-size: var(--font-size-sm); font-weight: 680; }
  header span { max-width: 190px; overflow: hidden; color: var(--muted-foreground); font-family: var(--terminal-font-family); font-size: var(--font-size-xs); text-overflow: ellipsis; white-space: nowrap; }
  div { display: grid; padding-top: 3px; }
  button { display: grid; min-width: 0; min-height: 42px; grid-template-columns: 18px minmax(0, 1fr) 14px; align-items: center; gap: 7px; border: 0; border-radius: var(--radius); padding: 5px 6px; background: transparent; color: var(--muted-foreground); text-align: left; cursor: pointer; }
  button:hover, button:focus-visible { outline: none; background: var(--accent); color: var(--foreground); }
  .copy { min-width: 0; }
  .copy strong, .copy small { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .copy strong { color: var(--popover-foreground); font-size: var(--font-size-sm); font-weight: 620; }
  .copy strong span { color: var(--muted-foreground); font-family: var(--terminal-font-family); font-size: var(--font-size-xs); }
  .copy small { margin-top: 1px; color: var(--muted-foreground); font-family: var(--terminal-font-family); font-size: var(--font-size-xs); }
</style>
