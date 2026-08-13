<script lang="ts">
  import CircleAlertIcon from '@lucide/svelte/icons/circle-alert';
  import ExternalLinkIcon from '@lucide/svelte/icons/external-link';
  import FolderOpenIcon from '@lucide/svelte/icons/folder-open';
  import GitBranchIcon from '@lucide/svelte/icons/git-branch';
  import GitPullRequestIcon from '@lucide/svelte/icons/git-pull-request';
  import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
  import SquareArrowOutUpRightIcon from '@lucide/svelte/icons/square-arrow-out-up-right';
  import { onMount } from 'svelte';

  import { Button } from '$lib/components/ui/button';
  import * as Popover from '$lib/components/ui/popover';
  import type { Project } from './daemon';
  import {
    editorActionLabel,
    ensureOpenersLoaded,
    openerSettings,
    openBrowserUrl,
    openProjectEditor,
    openProjectFinder
  } from './openers';
  import PullRequestList from './PullRequestList.svelte';
  import PullRequestStateIcon from './PullRequestStateIcon.svelte';
  import SectionOverview from './SectionOverview.svelte';
  import type {
    PullRequestCache,
    WorktreeEntry,
    WorktreeRepository
  } from './worktrees';
  import {
    projectDisplayName,
    pullRequestDetail,
    pullRequestInteraction,
    pullRequestLabel,
    pullRequestVisual,
    pullRequestsForWorktree
  } from './worktrees';

  type CountTarget = 'agent' | 'terminal' | 'todo';

  interface Props {
    project: Project;
    repository: WorktreeRepository | null;
    worktree: WorktreeEntry | null;
    pullRequestCache: PullRequestCache | null;
    refreshing?: boolean;
    counts: Record<CountTarget, number>;
    onRefresh: () => void | Promise<void>;
    onBrowse: (target: CountTarget) => void;
  }

  let {
    project,
    repository,
    worktree,
    pullRequestCache,
    refreshing = false,
    counts,
    onRefresh,
    onBrowse
  }: Props = $props();

  let actionBusy = $state<'editor' | 'finder' | 'pull-request' | null>(null);
  let actionError = $state<string | null>(null);
  let pullRequestListOpen = $state(false);
  let projectName = $derived(projectDisplayName(project));
  let editorLabel = $derived.by(() => {
    const label = editorActionLabel($openerSettings.config, $openerSettings.editors);
    return label === 'Open in editor' ? 'Open in IDE' : label;
  });
  let branch = $derived(worktree?.branch.trim() || null);
  let pullRequests = $derived(pullRequestsForWorktree(worktree));
  let pullRequest = $derived(pullRequests[0] ?? null);
  let pullRequestMode = $derived(pullRequestInteraction(pullRequests));
  let isWorktreeCheckout = $derived(
    project.parent_project_id !== null || (worktree !== null && worktree.kind !== 'main')
  );
  let isManagedWorktree = $derived(project.worktree_managed || worktree?.managed === true);
  let checkoutLabel = $derived(
    branch
      ? isWorktreeCheckout && repository
        ? `${repository.name}: ${branch}`
        : branch
      : repository
        ? 'Git state unavailable'
        : 'Not a Git repository'
  );
  let checkoutKind = $derived(
    isManagedWorktree
      ? 'Managed worktree'
      : isWorktreeCheckout
        ? 'Worktree checkout'
      : repository
        ? 'Repository checkout'
        : 'Local project'
  );

  onMount(() => {
    void ensureOpenersLoaded();
  });

  async function runAction(
    kind: NonNullable<typeof actionBusy>,
    action: () => Promise<void>
  ): Promise<void> {
    if (actionBusy) return;
    actionBusy = kind;
    actionError = null;
    try {
      await action();
    } catch (cause) {
      actionError = cause instanceof Error ? cause.message : String(cause);
    } finally {
      actionBusy = null;
    }
  }

  function statusCopy(status: WorktreeEntry['status']): string {
    if (status === 'clean') return 'Working tree clean';
    if (status === 'dirty') return 'Uncommitted changes';
    if (status === 'missing') return 'Checkout path missing';
    return 'Bare repository';
  }

  function plural(count: number, singular: string): string {
    return `${count} ${singular}${count === 1 ? '' : 's'}`;
  }
</script>

<SectionOverview
  ariaLabel={`${projectName} project overview`}
  eyebrow={checkoutKind}
  title={projectName}
  description={project.path}
  {project}
>
  {#snippet icon()}<GitBranchIcon strokeWidth={1.9} />{/snippet}

  {#snippet action()}
    {#if repository}
      <Button
        size="sm"
        variant="outline"
        disabled={refreshing}
        aria-label={`Refresh Git and pull request state for ${projectName}`}
        onclick={() => void onRefresh()}
      >
        <RefreshCwIcon class={refreshing ? 'spin' : ''} size={13} aria-hidden="true" />
        {refreshing ? 'Refreshing…' : 'Refresh'}
      </Button>
    {/if}
  {/snippet}

  {#snippet summary()}
    <span>Project activity</span>
    <span aria-hidden="true">·</span>
    <button type="button" class="count-link" onclick={() => onBrowse('agent')}>
      {plural(counts.agent, 'agent')}
    </button>
    <span aria-hidden="true">·</span>
    <button type="button" class="count-link" onclick={() => onBrowse('terminal')}>
      {plural(counts.terminal, 'terminal')}
    </button>
    <span aria-hidden="true">·</span>
    <button type="button" class="count-link" onclick={() => onBrowse('todo')}>
      {plural(counts.todo, 'todo')}
    </button>
  {/snippet}

  <div class="overview-scroll">
    <div class="overview-grid">
      <section class="checkout-card" aria-labelledby="git-state-heading">
        <div class="section-label">
          <span id="git-state-heading">Git state</span>
          {#if worktree}
            <span
              class:status-clean={worktree.status === 'clean'}
              class:status-attention={worktree.status !== 'clean'}
              class="status-chip"
            >{statusCopy(worktree.status)}</span>
          {/if}
        </div>
        <div class="head-strip" class:unavailable={!branch}>
          <span class="head-prompt">HEAD →</span>
          <strong title={checkoutLabel}>{checkoutLabel}</strong>
        </div>
        <dl class="checkout-details">
          {#if isWorktreeCheckout && repository}
            <div>
              <dt>Parent repository</dt>
              <dd>{repository.name}</dd>
            </div>
            <div>
              <dt>Repository root</dt>
              <dd title={repository.root_path}>{repository.root_path}</dd>
            </div>
          {/if}
          <div>
            <dt>Checkout path</dt>
            <dd title={project.path}>{project.path}</dd>
          </div>
          {#if worktree}
            <div>
              <dt>Commit</dt>
              <dd>{worktree.head.slice(0, 10)}</dd>
            </div>
          {/if}
        </dl>
      </section>

      <section class="pr-card" aria-labelledby="pull-request-heading">
        <div class="section-label">
          <span id="pull-request-heading">Pull request</span>
          {#if pullRequest}
            <PullRequestStateIcon state={pullRequest.state} size={15} strokeWidth={1.8} />
          {:else}
            <GitPullRequestIcon size={15} strokeWidth={1.8} aria-hidden="true" />
          {/if}
        </div>
        {#if pullRequest}
          <div class={`pr-state ${pullRequest.state}`}>
            <span class="pr-number" style:color={pullRequestVisual(pullRequest.state).color}>
              <PullRequestStateIcon state={pullRequest.state} size={18} strokeWidth={1.9} />
              #{pullRequest.number}
            </span>
            <div>
              <strong>{pullRequest.state === 'draft' ? 'Draft pull request' : `${pullRequest.state} pull request`}</strong>
              <small>{pullRequestDetail(pullRequest)}</small>
            </div>
          </div>
          {#if pullRequestMode === 'direct'}
            <Button
              size="sm"
              variant="outline"
              disabled={actionBusy !== null}
              aria-label={`Open ${pullRequestLabel(pullRequest)} on GitHub`}
              onclick={() => void runAction('pull-request', () => openBrowserUrl(pullRequest!.url))}
            >
              Open on GitHub <ExternalLinkIcon size={13} aria-hidden="true" />
            </Button>
          {:else}
            <Popover.Root bind:open={pullRequestListOpen}>
              <Popover.Trigger>
                {#snippet child({ props })}
                  <Button
                    {...props}
                    size="sm"
                    variant="outline"
                    disabled={actionBusy !== null}
                    aria-label={`Show ${pullRequests.length} pull requests for ${worktree?.branch ?? projectName}`}
                  >
                    View {pullRequests.length} pull requests
                  </Button>
                {/snippet}
              </Popover.Trigger>
              <Popover.Content side="bottom" align="start" sideOffset={6} class="w-80 gap-0 p-2">
                <PullRequestList
                  branch={worktree?.branch ?? projectName}
                  {pullRequests}
                  onChoose={(target) => {
                    pullRequestListOpen = false;
                    void runAction('pull-request', () => openBrowserUrl(target.url));
                  }}
                />
              </Popover.Content>
            </Popover.Root>
          {/if}
        {:else if pullRequestCache?.available === false}
          <div class="pr-state unavailable" role="status">
            <CircleAlertIcon size={18} strokeWidth={1.8} aria-hidden="true" />
            <div>
              <strong>PR status unavailable</strong>
              <small>{pullRequestCache.error?.trim() || 'Lookup failed without a reason.'}</small>
            </div>
          </div>
          <Button size="sm" variant="outline" disabled={refreshing} onclick={() => void onRefresh()}>
            Try again
          </Button>
        {:else if worktree && pullRequestCache?.available === true}
          <div class="pr-state none" aria-label={`No pull request for ${worktree.branch}`}>
            <GitPullRequestIcon size={18} strokeWidth={1.8} aria-hidden="true" />
            <div>
              <strong>No pull request</strong>
              <small>No pull request found for {worktree.branch}.</small>
            </div>
          </div>
        {:else if repository}
          <div class="pr-state none" role="status">
            <RefreshCwIcon class={refreshing ? 'spin' : ''} size={18} strokeWidth={1.8} aria-hidden="true" />
            <div>
              <strong>Checking pull request status</strong>
              <small>Waiting for repository metadata.</small>
            </div>
          </div>
        {:else}
          <div class="pr-state none">
            <GitPullRequestIcon size={18} strokeWidth={1.8} aria-hidden="true" />
            <div>
              <strong>Pull requests unavailable</strong>
              <small>This project is not a Git repository.</small>
            </div>
          </div>
        {/if}
      </section>

      <section class="quick-actions" aria-labelledby="quick-actions-heading">
        <div class="section-label"><span id="quick-actions-heading">Quick actions</span></div>
        <div class="action-row">
          <Button
            disabled={actionBusy !== null}
            onclick={() => void runAction('editor', () => openProjectEditor(project.path, $openerSettings))}
          >
            <SquareArrowOutUpRightIcon size={14} aria-hidden="true" />
            {actionBusy === 'editor' ? 'Opening…' : editorLabel}
          </Button>
          <Button
            variant="outline"
            disabled={actionBusy !== null}
            onclick={() => void runAction('finder', () => openProjectFinder(project.path))}
          >
            <FolderOpenIcon size={14} aria-hidden="true" />
            {actionBusy === 'finder' ? 'Opening…' : 'Open in Finder'}
          </Button>
        </div>
        {#if actionError}
          <p class="action-error" role="alert">{actionError}</p>
        {/if}
      </section>
    </div>
  </div>
</SectionOverview>

<style>
  .overview-scroll { height: 100%; min-height: 0; overflow-y: auto; scrollbar-color: var(--border-strong) transparent; scrollbar-width: thin; }
  .overview-grid { display: grid; box-sizing: border-box; width: min(100%, 920px); grid-template-columns: minmax(0, 1.35fr) minmax(260px, .9fr); gap: 12px; margin: 0 auto; padding: 18px clamp(14px, 2.8cqw, 28px) 36px; }
  .checkout-card, .pr-card, .quick-actions { min-width: 0; border: 1px solid var(--border); border-radius: var(--radius); background: var(--card); }
  .checkout-card { grid-row: span 2; padding: 16px; }
  .pr-card, .quick-actions { padding: 14px; }
  .section-label { display: flex; min-height: 22px; align-items: center; justify-content: space-between; gap: var(--space-2); color: var(--muted-foreground); font-family: var(--terminal-font-family); font-size: var(--font-size-xs); font-weight: 650; letter-spacing: .075em; text-transform: uppercase; }
  .head-strip { display: flex; min-width: 0; align-items: baseline; gap: 11px; margin: 14px -4px 16px; border-left: 3px solid var(--ring); padding: 12px 13px; background: color-mix(in srgb, var(--ring) 8%, var(--background)); font-family: var(--terminal-font-family); }
  .head-strip.unavailable { border-left-color: var(--muted-foreground); }
  .head-strip strong { min-width: 0; overflow: hidden; color: var(--foreground); font-size: clamp(19px, 2.1cqw, 25px); font-weight: 640; letter-spacing: -.025em; text-overflow: ellipsis; white-space: nowrap; }
  .head-prompt { flex: none; color: var(--muted-foreground); font-size: var(--font-size-xs); font-weight: 650; letter-spacing: .045em; }
  .status-chip { border: 1px solid var(--border-strong); border-radius: 999px; padding: 2px 7px; color: var(--muted-foreground); font-size: 10px; letter-spacing: .02em; text-transform: none; }
  .status-clean { border-color: color-mix(in srgb, var(--success) 38%, var(--border)); color: var(--success); }
  .status-attention { border-color: color-mix(in srgb, var(--warning-token) 42%, var(--border)); color: var(--warning-token); }
  .checkout-details { display: grid; gap: 0; margin: 0; border-top: 1px solid var(--border); }
  .checkout-details div { display: grid; min-width: 0; grid-template-columns: 126px minmax(0, 1fr); gap: var(--space-3); border-bottom: 1px solid var(--border); padding: 8px 2px; }
  .checkout-details dt { color: var(--muted-foreground); font-size: var(--font-size-xs); }
  .checkout-details dd { min-width: 0; margin: 0; overflow: hidden; color: var(--foreground); font-family: var(--terminal-font-family); font-size: var(--font-size-xs); text-overflow: ellipsis; white-space: nowrap; }
  .pr-card { display: grid; align-content: start; gap: 10px; }
  .pr-card :global(button) { justify-self: start; }
  .pr-state { display: grid; min-height: 47px; grid-template-columns: auto minmax(0, 1fr); align-items: center; gap: 9px; border-left: 2px solid var(--border-strong); padding: 6px 8px; background: var(--background); }
  .pr-state.open { border-left-color: var(--success); }
  .pr-state.merged { border-left-color: var(--pull-request-merged); }
  .pr-state.closed { border-left-color: var(--destructive); }
  .pr-state.draft { border-left-color: var(--muted-foreground); }
  .pr-state.unavailable { border-left-color: var(--warning-token); color: var(--warning-token); }
  .pr-state.none { color: var(--muted-foreground); }
  .pr-state strong, .pr-state small { display: block; }
  .pr-state strong { color: var(--foreground); font-size: var(--font-size-sm); font-weight: 620; text-transform: capitalize; }
  .pr-state small { margin-top: 2px; overflow-wrap: anywhere; color: var(--muted-foreground); font-family: var(--terminal-font-family); font-size: var(--font-size-xs); }
  .pr-number { display: inline-flex; align-items: center; gap: 5px; font-family: var(--terminal-font-family); font-size: var(--font-size-base); font-weight: 700; }
  .quick-actions { display: grid; align-content: start; gap: 10px; }
  .action-row { display: flex; flex-wrap: wrap; gap: 7px; }
  .action-error { margin: 0; color: var(--destructive); font-size: var(--font-size-xs); }
  .count-link { border: 0; padding: 1px 2px; background: transparent; color: var(--muted-foreground); font: inherit; text-decoration: underline; text-decoration-color: transparent; text-underline-offset: 3px; cursor: pointer; }
  .count-link:hover { color: var(--foreground); text-decoration-color: currentColor; }
  .count-link:focus-visible { border-radius: 3px; outline: 2px solid var(--ring); outline-offset: 2px; color: var(--foreground); }
  :global(.spin) { animation: project-overview-spin 800ms linear infinite; }
  @keyframes project-overview-spin { to { transform: rotate(360deg); } }

  @container (max-width: 760px) {
    .overview-grid { grid-template-columns: minmax(0, 1fr); }
    .checkout-card { grid-row: auto; }
  }

  @container (max-width: 430px) {
    .overview-grid { padding: 12px 10px 28px; }
    .checkout-details div { grid-template-columns: 1fr; gap: 3px; }
    .head-strip { align-items: flex-start; flex-direction: column; gap: 4px; }
    .head-strip strong { width: 100%; }
    .action-row :global(button) { width: 100%; }
  }

  @media (prefers-reduced-motion: reduce) {
    :global(.spin) { animation: none; }
  }
</style>
