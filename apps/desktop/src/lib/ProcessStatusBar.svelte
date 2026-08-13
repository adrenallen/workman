<script module lang="ts">
  import type { DaemonClient, ProcessView, Project } from './daemon';
  import type { DaemonLogEntry } from './daemonLog';
  import type { PullRequestStatus } from './worktrees';

  export interface ProcessStatusBarProps {
    client: DaemonClient;
    project: Project;
    process: ProcessView;
    processes: ProcessView[];
    pullRequests: PullRequestStatus[];
    connected: boolean;
    daemonPort?: number | null;
    daemonEvents: DaemonLogEntry[];
    onUnfocus: () => void;
    onSelectProcess: (processId: number) => void;
    onClearDaemonEvents: () => void;
    onError: (message: string) => void;
  }
</script>

<script lang="ts">
  import ArrowLeftIcon from '@lucide/svelte/icons/arrow-left';
  import ArrowRightIcon from '@lucide/svelte/icons/arrow-right';
  import Clock3Icon from '@lucide/svelte/icons/clock-3';
  import GitBranchIcon from '@lucide/svelte/icons/git-branch';
  import MoreHorizontalIcon from '@lucide/svelte/icons/more-horizontal';
  import ServerIcon from '@lucide/svelte/icons/server';
  import XIcon from '@lucide/svelte/icons/x';
  import { onMount } from 'svelte';

  import { agentStatusPresentation } from './agentStatus';
  import AgentStatusIndicator from './components/ds/AgentStatusIndicator.svelte';
  import StatusIndicator from './components/ds/StatusIndicator.svelte';
  import * as Popover from './components/ui/popover';
  import { liveStats, type DescendantProcessStats } from './liveStats';
  import { processActivity } from './processActivity';
  import { openBrowserUrl } from './openers';
  import PullRequestList from './PullRequestList.svelte';
  import PullRequestStateIcon from './PullRequestStateIcon.svelte';
  import { killSubprocess, listSubprocesses } from './subprocesses';
  import TimerCountdown from './TimerCountdown.svelte';
  import { projectDisplayName, pullRequestInteraction, pullRequestLabel } from './worktrees';

  let {
    client,
    project,
    process,
    processes,
    pullRequests,
    connected,
    daemonPort = null,
    daemonEvents,
    onUnfocus,
    onSelectProcess,
    onClearDaemonEvents,
    onError
  }: ProcessStatusBarProps = $props();

  let popoverOpen = $state(false);
  let daemonPopoverOpen = $state(false);
  let overflowRoot = $state<HTMLElement>();
  let overflowOpen = $state(false);
  let freshChildren = $state<DescendantProcessStats[]>([]);
  let loadingChildren = $state(false);
  let confirmingPid = $state<number | null>(null);
  let killingPid = $state<number | null>(null);
  let activeProcessId = $state<number | null>(null);
  let statusWidth = $state(1_200);
  let pullRequestListOpen = $state(false);

  const stats = $derived($liveStats.processes[process.id] ?? null);
  const activity = $derived(processActivity(process, stats ?? undefined));
  const childCount = $derived(popoverOpen ? freshChildren.length : (stats?.descendant_count ?? 0));
  const daemonNoticeCount = $derived(
    daemonEvents.reduce((count, event) => count + (event.tone === 'info' ? 0 : event.count), 0)
  );
  const siblingIndex = $derived(processes.findIndex((candidate) => candidate.id === process.id));
  const timerDensity = $derived(statusWidth <= 680 ? 'hidden' : statusWidth <= 1_040 ? 'compact' : 'full');
  const pullRequest = $derived(pullRequests[0] ?? null);
  const pullRequestMode = $derived(pullRequestInteraction(pullRequests));

  async function openPullRequest(target: PullRequestStatus): Promise<void> {
    pullRequestListOpen = false;
    try {
      await openBrowserUrl(target.url);
    } catch (cause) {
      onError(cause instanceof Error ? cause.message : String(cause));
    }
  }

  onMount(() => {
    const handlePointerDown = (event: PointerEvent) => {
      if (overflowOpen && !overflowRoot?.contains(event.target as Node)) closeOverflow();
    };
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape' && (popoverOpen || daemonPopoverOpen || overflowOpen)) {
        event.preventDefault();
        closePopover();
        daemonPopoverOpen = false;
        closeOverflow();
      }
    };
    document.addEventListener('pointerdown', handlePointerDown);
    document.addEventListener('keydown', handleKeyDown);
    return () => {
      document.removeEventListener('pointerdown', handlePointerDown);
      document.removeEventListener('keydown', handleKeyDown);
    };
  });

  $effect(() => {
    // Status ticks replace `process`; transient UI resets only when selection really changes.
    const nextProcessId = process.id;
    if (activeProcessId === null) {
      activeProcessId = nextProcessId;
      return;
    }
    if (nextProcessId === activeProcessId) return;
    activeProcessId = nextProcessId;
    popoverOpen = false;
    daemonPopoverOpen = false;
    overflowOpen = false;
    freshChildren = [];
    confirmingPid = null;
  });

  $effect(() => {
    if (!popoverOpen || !connected || activeProcessId === null) return;
    const processId = activeProcessId;
    queueMicrotask(() => void refreshChildren(processId));
    const timer = window.setInterval(() => void refreshChildren(processId), 2_000);
    return () => window.clearInterval(timer);
  });

  function reportCause(cause: unknown): void {
    onError(cause instanceof Error ? cause.message : String(cause));
  }

  async function refreshChildren(processId: number): Promise<void> {
    if (loadingChildren) return;
    loadingChildren = true;
    try {
      const result = await listSubprocesses(client, processId);
      if (result.process_id === activeProcessId) freshChildren = result.subprocesses;
    } catch (cause) {
      reportCause(cause);
    } finally {
      loadingChildren = false;
    }
  }

  function togglePopover(): void {
    changePopover(!popoverOpen);
  }

  function changePopover(open: boolean): void {
    popoverOpen = open;
    confirmingPid = null;
    if (open) {
      freshChildren = stats?.descendants ?? [];
    }
  }

  function closePopover(): void {
    popoverOpen = false;
    confirmingPid = null;
  }

  function toggleOverflow(): void {
    overflowOpen = !overflowOpen;
  }

  function closeOverflow(): void {
    overflowOpen = false;
  }

  function showSubprocessesFromOverflow(): void {
    closeOverflow();
    togglePopover();
  }

  async function confirmKill(child: DescendantProcessStats): Promise<void> {
    if (killingPid !== null || activeProcessId === null) return;
    killingPid = child.pid;
    try {
      await killSubprocess(client, activeProcessId, child.pid);
      confirmingPid = null;
      await refreshChildren(activeProcessId);
    } catch (cause) {
      reportCause(cause);
    } finally {
      killingPid = null;
    }
  }

  function cycle(direction: -1 | 1): void {
    if (processes.length < 2) return;
    const current = siblingIndex < 0 ? 0 : siblingIndex;
    const next = (current + direction + processes.length) % processes.length;
    onSelectProcess(processes[next].id);
  }

  function formatDuration(totalSeconds: number | undefined): string {
    const seconds = Math.max(0, Math.floor(totalSeconds ?? 0));
    const hours = Math.floor(seconds / 3_600);
    const minutes = Math.floor((seconds % 3_600) / 60);
    const remainder = seconds % 60;
    return hours > 0
      ? `${hours}:${minutes.toString().padStart(2, '0')}:${remainder.toString().padStart(2, '0')}`
      : `${minutes.toString().padStart(2, '0')}:${remainder.toString().padStart(2, '0')}`;
  }

  function formatMemory(bytes: number | undefined): string {
    if (!bytes) return '0 B';
    const units = ['B', 'KB', 'MB', 'GB'];
    const index = Math.min(Math.floor(Math.log(bytes) / Math.log(1_024)), units.length - 1);
    const value = bytes / 1_024 ** index;
    return `${value >= 10 || index === 0 ? value.toFixed(0) : value.toFixed(1)} ${units[index]}`;
  }

  function childCommand(child: DescendantProcessStats): string {
    return child.command?.trim() || child.name;
  }

  function agentConversationLabel(process: ProcessView): string {
    switch (process.agent_launch_mode) {
      case 'resumed_session':
        return 'Resumed captured session';
      case 'continued_latest':
        return 'Continued latest in folder';
      case 'fresh':
        return 'Fresh session';
      default:
        return '—';
    }
  }

  function formatDaemonEventTime(timestamp: number): string {
    return new Intl.DateTimeFormat(undefined, {
      hour: 'numeric',
      minute: '2-digit',
      second: '2-digit'
    }).format(timestamp);
  }
</script>

<footer
  class="status-bar"
  class:stage-medium={statusWidth <= 1_040}
  class:stage-narrow={statusWidth <= 680}
  class:stage-tiny={statusWidth <= 480}
  bind:clientWidth={statusWidth}
  aria-label={`${projectDisplayName(project)} selected process status`}
>
  <nav class="navigation" aria-label="Process selection">
    <button
      type="button"
      class="unfocus"
      title="Unfocus terminal (⌘U)"
      aria-keyshortcuts="Meta+U"
      onclick={onUnfocus}
    >
      <XIcon size={13} aria-hidden="true" /><span class="nav-label">Unfocus</span>
    </button>
    <span class="rule" aria-hidden="true"></span>
    <button
      type="button"
      title="Previous process"
      disabled={processes.length < 2}
      onclick={() => cycle(-1)}
    >
      <ArrowLeftIcon size={13} aria-hidden="true" /><span class="nav-label">Prev</span>
    </button>
    <button
      type="button"
      title="Next process"
      disabled={processes.length < 2}
      onclick={() => cycle(1)}
    >
      <span class="nav-label">Next</span><ArrowRightIcon size={13} aria-hidden="true" />
    </button>
  </nav>

  <div class="telemetry">
    <span class="metric uptime" title={`Process uptime: ${formatDuration(stats?.uptime_seconds)}`}>
      <Clock3Icon class="clock" size={13} strokeWidth={1.8} aria-hidden="true" />
      <span class="uptime-prefix">up</span>
      <span class="uptime-value">{formatDuration(stats?.uptime_seconds)}</span>
    </span>
    <TimerCountdown processId={process.id} density={timerDensity} />
    <strong class="process-name" title={process.name}>{process.name}</strong>
    {#if pullRequest}
      {#if pullRequestMode === 'direct'}
        <button
          type="button"
          class="pull-request-link"
          title={`Open ${pullRequestLabel(pullRequest)} on GitHub`}
          aria-label={`Open ${pullRequestLabel(pullRequest)} on GitHub`}
          onclick={() => void openPullRequest(pullRequest)}
        >
          <PullRequestStateIcon state={pullRequest.state} size={13} strokeWidth={1.9} />
          <span class="pull-request-number">#{pullRequest.number}</span>
        </button>
      {:else}
        <Popover.Root bind:open={pullRequestListOpen}>
          <Popover.Trigger>
            {#snippet child({ props })}
              <button
                {...props}
                type="button"
                class="pull-request-link"
                title={`Show ${pullRequests.length} pull requests for ${project.branch ?? project.name}`}
                aria-label={`Show ${pullRequests.length} pull requests for ${project.branch ?? project.name}`}
              >
                <PullRequestStateIcon state={pullRequest.state} size={13} strokeWidth={1.9} />
                <span class="pull-request-number">{pullRequests.length} PRs</span>
              </button>
            {/snippet}
          </Popover.Trigger>
          <Popover.Content side="top" align="end" sideOffset={7} class="w-80 gap-0 p-2">
            <PullRequestList
              branch={project.branch ?? project.name}
              {pullRequests}
              onChoose={(target) => void openPullRequest(target)}
            />
          </Popover.Content>
        </Popover.Root>
      {/if}
    {/if}

    <Popover.Root open={popoverOpen} onOpenChange={changePopover}>
      <Popover.Trigger>
        {#snippet child({ props })}
          <button
            {...props}
            type="button"
            class="subprocess-trigger"
            class:active={popoverOpen}
            aria-haspopup="dialog"
            aria-expanded={popoverOpen}
            title={`${childCount} live ${childCount === 1 ? 'subprocess' : 'subprocesses'}`}
            disabled={!connected || process.pid === null}
          >
            <GitBranchIcon class="branch" size={13} strokeWidth={1.8} aria-hidden="true" />
            <span>+{childCount}</span>
            <span class="subprocess-label">{childCount === 1 ? 'subprocess' : 'subprocesses'}</span>
          </button>
        {/snippet}
      </Popover.Trigger>

      {#if popoverOpen}
        <Popover.Content side="top" align="end" sideOffset={7} class="w-auto gap-0 bg-transparent p-0 shadow-none ring-0">
        <div class="subprocess-popover" role="dialog" aria-label={`${process.name} subprocesses`}>
          <header>
            <div>
              <span class="eyebrow">Live process tree</span>
              <h2>{process.name}</h2>
            </div>
            <span class="root-pid">ROOT {process.pid}</span>
          </header>

          {#if loadingChildren && freshChildren.length === 0}
            <div class="empty-tree"><span aria-hidden="true"></span>Sampling descendants…</div>
          {:else if freshChildren.length === 0}
            <div class="empty-tree"><span aria-hidden="true"></span>No live subprocesses</div>
          {:else}
            <div class="child-list">
              {#each freshChildren as child (child.pid)}
                <article class="child-row">
                  <span class="lineage" aria-hidden="true"><i></i></span>
                  <div class="child-copy">
                    <div class="child-heading">
                      <strong>{child.name}</strong>
                      <span>PID {child.pid}</span>
                    </div>
                    <code title={childCommand(child)}>{childCommand(child)}</code>
                  </div>
                  <div class="child-stats" aria-label={`PID ${child.pid} resource usage`}>
                    <span>{child.cpu_percent.toFixed(1)}%</span>
                    <span>{formatMemory(child.memory_bytes)}</span>
                  </div>
                  <div class="kill-action">
                    {#if confirmingPid === child.pid}
                      <button type="button" class="cancel" onclick={() => (confirmingPid = null)}>
                        Cancel
                      </button>
                      <button
                        type="button"
                        class="confirm"
                        disabled={killingPid !== null}
                        onclick={() => confirmKill(child)}
                      >
                        {killingPid === child.pid ? 'Stopping…' : 'Confirm'}
                      </button>
                    {:else}
                      <button
                        type="button"
                        class="kill"
                        disabled={killingPid !== null}
                        onclick={() => (confirmingPid = child.pid)}
                      >
                        Kill
                      </button>
                    {/if}
                  </div>
                </article>
              {/each}
            </div>
          {/if}

          <footer>
            <span>Only descendants of this live root can be signaled.</span>
            <button type="button" onclick={closePopover}>Close</button>
          </footer>
        </div>
        </Popover.Content>
      {/if}
    </Popover.Root>

    <span class="metric secondary-metric" title={`CPU usage: ${(stats?.cpu_percent ?? 0).toFixed(1)}%`}>
      <span class="metric-label">CPU</span><span>{(stats?.cpu_percent ?? 0).toFixed(1)}%</span>
    </span>
    <span class="metric secondary-metric" title={`Memory usage: ${formatMemory(stats?.memory_bytes)}`}>
      <span class="metric-label">MEM</span><span>{formatMemory(stats?.memory_bytes)}</span>
    </span>
    {#if process.kind === 'agent'}
      <span class="agent-attention"><AgentStatusIndicator {process} showLabel={statusWidth > 1_040} /></span>
    {:else}
      <span
        class:active-run={activity.state === 'working'}
        class:fault={activity.state === 'crashed'}
        class="run-state"
        title={activity.label}
      >
        <i aria-hidden="true"></i><span class="state-word">{activity.shortLabel}</span>
      </span>
    {/if}

    <Popover.Root bind:open={daemonPopoverOpen}>
      <Popover.Trigger>
        {#snippet child({ props })}
          <button
            {...props}
            type="button"
            class="daemon-connection"
            class:active={daemonPopoverOpen}
            aria-label={`${connected ? 'Daemon connected' : 'Daemon disconnected'}${daemonNoticeCount > 0 ? `, ${daemonNoticeCount} logged ${daemonNoticeCount === 1 ? 'notice' : 'notices'}` : ''}`}
            title="Open daemon log"
          >
            <ServerIcon size={13} strokeWidth={1.8} aria-hidden="true" />
            <StatusIndicator
              tone={connected ? 'success' : 'danger'}
              label={connected ? `Daemon connected · port ${daemonPort ?? 'unknown'}` : 'Daemon disconnected'}
            />
            <span class="daemon-port">{connected ? `:${daemonPort ?? '—'}` : 'offline'}</span>
            {#if daemonNoticeCount > 0}
              <span class="daemon-notice-count" aria-hidden="true">{daemonNoticeCount}</span>
            {/if}
          </button>
        {/snippet}
      </Popover.Trigger>

      {#if daemonPopoverOpen}
        <Popover.Content side="top" align="end" sideOffset={7} class="w-auto gap-0 bg-transparent p-0 shadow-none ring-0">
          <section class="daemon-log-popover" aria-label="Daemon activity log">
            <header>
              <div>
                <span class="eyebrow">Local runtime</span>
                <h2>Daemon log</h2>
              </div>
              <span class:connected class="daemon-log-state">
                <i aria-hidden="true"></i>{connected ? `Connected · :${daemonPort ?? '—'}` : 'Disconnected'}
              </span>
            </header>

            <div class="daemon-log-list" aria-live="polite">
              {#each daemonEvents as event (event.id)}
                <article data-tone={event.tone}>
                  <i class="daemon-log-dot" aria-hidden="true"></i>
                  <div>
                    <strong>{event.title}</strong>
                    {#if event.detail}<p>{event.detail}</p>{/if}
                  </div>
                  <time datetime={new Date(event.occurredAt).toISOString()} title={new Date(event.occurredAt).toLocaleString()}>
                    {formatDaemonEventTime(event.occurredAt)}{#if event.count > 1}<span> ×{event.count}</span>{/if}
                  </time>
                </article>
              {:else}
                <div class="daemon-log-empty">
                  <ServerIcon size={15} strokeWidth={1.7} aria-hidden="true" />
                  <span>No daemon notices this session</span>
                </div>
              {/each}
            </div>

            {#if daemonEvents.length > 0}
              <footer>
                <span>Newest first · session only</span>
                <button type="button" onclick={onClearDaemonEvents}>Clear log</button>
              </footer>
            {/if}
          </section>
        </Popover.Content>
      {/if}
    </Popover.Root>

    <div class="status-overflow-control" bind:this={overflowRoot}>
      <button
        type="button"
        class="status-overflow-trigger"
        class:active={overflowOpen}
        title="Show full process status"
        aria-label="Show full process status"
        aria-haspopup="dialog"
        aria-expanded={overflowOpen}
        onclick={toggleOverflow}
      ><MoreHorizontalIcon size={15} aria-hidden="true" /></button>

      {#if overflowOpen}
        <dialog open class="status-overflow-popover" aria-label={`${process.name} full status`}>
          <header>
            <div><span class="eyebrow">Full status</span><h2>{process.name}</h2></div>
            <button type="button" title="Close full status" onclick={closeOverflow}><XIcon size={14} /></button>
          </header>

          <nav class="overflow-actions" aria-label="Process actions">
            <button type="button" onclick={() => { closeOverflow(); onUnfocus(); }}>Unfocus</button>
            <button type="button" disabled={processes.length < 2} onclick={() => cycle(-1)}><ArrowLeftIcon size={13} /> Previous</button>
            <button type="button" disabled={processes.length < 2} onclick={() => cycle(1)}>Next <ArrowRightIcon size={13} /></button>
          </nav>

          <dl>
            <div><dt>Uptime</dt><dd>{formatDuration(stats?.uptime_seconds)}</dd></div>
            <div><dt>CPU</dt><dd>{(stats?.cpu_percent ?? 0).toFixed(1)}%</dd></div>
            <div><dt>Memory</dt><dd>{formatMemory(stats?.memory_bytes)}</dd></div>
            {#if process.kind === 'agent'}<div><dt>Agent state</dt><dd>{agentStatusPresentation(process).shortLabel}</dd></div>{/if}
            {#if process.kind === 'agent'}
              <div>
                <dt>Conversation</dt>
                <dd title={process.agent_session_id ?? undefined}>{agentConversationLabel(process)}</dd>
              </div>
            {/if}
            <div><dt>State</dt><dd>{process.status}</dd></div>
            <div><dt>PID</dt><dd>{process.pid ?? '—'}</dd></div>
          </dl>

          <section class="overflow-timers" aria-label="Active timers">
            <span class="eyebrow">Timers</span>
            <TimerCountdown processId={process.id} variant="menu" />
          </section>

          <button
            type="button"
            class="overflow-subprocesses"
            disabled={!connected || process.pid === null}
            onclick={showSubprocessesFromOverflow}
          >
            <span><GitBranchIcon size={13} /> +{childCount}</span>
            <strong>{childCount === 1 ? 'Subprocess' : 'Subprocesses'}</strong>
          </button>
        </dialog>
      {/if}
    </div>
  </div>
</footer>

<style>
  .status-bar {
    container-name: process-status;
    container-type: inline-size;
    position: relative;
    z-index: 20;
    display: flex;
    min-width: 0;
    height: 30px;
    align-items: stretch;
    justify-content: space-between;
    border-top: 1px solid var(--border);
    background: var(--card);
    color: var(--text-soft);
    font-family: 'JetBrains Mono Variable', monospace;
    font-size: var(--font-size-sm);
    letter-spacing: 0.02em;
    white-space: nowrap;
  }

  button {
    border: 0;
    background: transparent;
    color: inherit;
    cursor: pointer;
  }

  button:disabled {
    opacity: 0.36;
    cursor: default;
  }

  .navigation,
  .telemetry {
    display: flex;
    min-width: 0;
    align-items: stretch;
  }

  .navigation {
    flex: 0 0 auto;
  }

  .navigation button {
    display: flex;
    align-items: center;
    gap: 5px;
    padding: 0 8px;
    color: var(--muted-foreground);
    font-size: var(--font-size-xs);
    font-weight: 600;
    text-transform: uppercase;
  }

  .navigation button:not(:disabled):hover,
  .navigation button:focus-visible {
    background: var(--popover);
    color: var(--fog);
  }

  .navigation .unfocus {
    color: var(--text-soft);
  }

  .navigation .unfocus span {
    color: var(--fault);
    font-size: 12px;
  }

  .navigation .unfocus .nav-label {
    color: inherit;
    font-size: inherit;
  }

  .rule {
    width: 1px;
    height: 14px;
    align-self: center;
    background: var(--border);
  }

  .telemetry {
    flex: 0 1 auto;
    overflow: visible;
    justify-content: flex-end;
  }

  .telemetry > span,
  .process-name,
  .pull-request-link,
  .subprocess-trigger {
    display: flex;
    min-width: 0;
    align-items: center;
    border-left: 1px solid var(--border);
    padding: 0 9px;
    white-space: nowrap;
  }

  .metric {
    gap: 4px;
    color: var(--muted-foreground);
    font-variant-numeric: tabular-nums;
  }

  .uptime {
    gap: 5px;
    color: var(--text-soft);
  }

  .clock {
    color: var(--muted-foreground);
    font-size: var(--font-size-sm);
  }

  .process-name {
    max-width: 220px;
    overflow: hidden;
    color: var(--foreground);
    font-family: 'Archivo Variable', sans-serif;
    font-size: var(--font-size-sm);
    font-weight: 650;
    letter-spacing: 0;
    text-overflow: ellipsis;
  }

  .pull-request-link {
    gap: 4px;
    color: var(--muted-foreground);
    font-size: var(--font-size-xs);
    font-weight: 650;
  }

  .pull-request-link:not(:disabled):hover,
  .pull-request-link:focus-visible {
    background: var(--popover);
    color: var(--foreground);
  }

  .subprocess-trigger {
    gap: 5px;
    color: var(--text-soft);
    font-size: var(--font-size-xs);
    font-weight: 600;
    text-transform: uppercase;
  }

  .subprocess-trigger:hover,
  .subprocess-trigger.active {
    background: var(--popover);
    color: var(--foreground);
  }

  .branch {
    color: var(--signal);
    font-size: 12px;
  }

  .run-state {
    gap: 6px;
    color: var(--muted-foreground);
    font-weight: 650;
    text-transform: uppercase;
  }

  .run-state i {
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: currentColor;
  }

  .run-state.active-run {
    color: var(--signal);
  }

  .run-state.fault {
    color: var(--fault);
  }

  .daemon-connection {
    display: flex;
    min-width: 0;
    align-items: center;
    gap: 3px;
    border-left: 1px solid var(--border);
    padding: 0 9px;
    color: var(--muted-foreground);
    font-size: var(--font-size-xs);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }

  .daemon-connection:hover,
  .daemon-connection.active,
  .daemon-connection:focus-visible {
    background: var(--popover);
    color: var(--foreground);
  }

  .daemon-connection > :global(svg) {
    flex: none;
  }

  .daemon-port {
    color: var(--muted-foreground);
  }

  .daemon-notice-count {
    display: grid;
    min-width: 15px;
    height: 15px;
    place-items: center;
    border: 1px solid color-mix(in srgb, var(--warning-token) 55%, var(--border));
    border-radius: 999px;
    padding: 0 3px;
    background: color-mix(in srgb, var(--warning-token) 12%, var(--card));
    color: var(--warning-token);
    font-size: 9px;
    font-weight: 750;
    line-height: 1;
  }

  .daemon-log-popover {
    width: min(440px, calc(100vw - 28px));
    overflow: hidden;
    border: 1px solid var(--border-strong);
    border-radius: 3px;
    background: var(--popover);
    box-shadow: 0 18px 48px rgb(0 0 0 / 48%);
    color: var(--text-soft);
    font: inherit;
  }

  .daemon-log-popover > header,
  .daemon-log-popover > footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .daemon-log-popover > header {
    min-height: 48px;
    gap: 12px;
    border-bottom: 1px solid var(--border);
    padding: 8px 11px 7px 14px;
  }

  .daemon-log-state {
    display: flex;
    align-items: center;
    gap: 5px;
    color: var(--fault);
    font-size: var(--font-size-xs);
    font-weight: 650;
  }

  .daemon-log-state i,
  .daemon-log-dot {
    width: 6px;
    height: 6px;
    flex: none;
    border-radius: 999px;
    background: currentColor;
  }

  .daemon-log-state.connected {
    color: var(--success);
  }

  .daemon-log-list {
    max-height: 260px;
    overflow-y: auto;
  }

  .daemon-log-list article {
    display: grid;
    min-height: 48px;
    grid-template-columns: 8px minmax(0, 1fr) auto;
    align-items: start;
    gap: 8px;
    border-bottom: 1px solid var(--border);
    padding: 8px 11px;
  }

  .daemon-log-list article:last-child {
    border-bottom: 0;
  }

  .daemon-log-dot {
    margin-top: 5px;
    color: var(--muted-foreground);
  }

  .daemon-log-list article[data-tone='info'] .daemon-log-dot { color: var(--success); }
  .daemon-log-list article[data-tone='warning'] .daemon-log-dot { color: var(--warning-token); }
  .daemon-log-list article[data-tone='error'] .daemon-log-dot { color: var(--fault); }

  .daemon-log-list article > div {
    min-width: 0;
  }

  .daemon-log-list strong,
  .daemon-log-list p {
    display: block;
    overflow-wrap: anywhere;
    white-space: normal;
  }

  .daemon-log-list strong {
    color: var(--foreground);
    font-family: 'Archivo Variable', sans-serif;
    font-size: var(--font-size-sm);
    font-weight: 650;
  }

  .daemon-log-list p {
    margin: 3px 0 0;
    color: var(--muted-foreground);
    font-size: var(--font-size-xs);
    line-height: 1.35;
  }

  .daemon-log-list time {
    color: var(--muted-foreground);
    font-size: var(--font-size-xs);
    font-variant-numeric: tabular-nums;
  }

  .daemon-log-list time span {
    color: var(--warning-token);
    font-weight: 700;
  }

  .daemon-log-empty {
    display: flex;
    min-height: 82px;
    align-items: center;
    justify-content: center;
    gap: 7px;
    color: var(--muted-foreground);
    font-size: var(--font-size-xs);
  }

  .daemon-log-popover > footer {
    min-height: 34px;
    border-top: 1px solid var(--border);
    padding: 5px 8px 5px 12px;
    color: var(--muted-foreground);
    font-size: var(--font-size-xs);
  }

  .daemon-log-popover > footer button {
    border: 1px solid var(--border-strong);
    border-radius: 2px;
    padding: 4px 6px;
    background: var(--accent);
    color: var(--text-soft);
    font-size: var(--font-size-xs);
    font-weight: 650;
    text-transform: uppercase;
  }

  .daemon-log-popover > footer button:hover {
    color: var(--foreground);
  }

  .status-overflow-control {
    position: relative;
    display: none;
    min-width: 0;
  }

  .status-overflow-trigger {
    display: flex;
    width: 34px;
    align-items: center;
    justify-content: center;
    border-left: 1px solid var(--border);
    color: var(--text-soft);
    font-size: var(--font-size-sm);
    font-weight: 700;
    letter-spacing: 0.08em;
  }

  .status-overflow-trigger:hover,
  .status-overflow-trigger.active,
  .status-overflow-trigger:focus-visible {
    background: var(--popover);
    color: var(--fog);
  }

  .status-overflow-popover {
    position: absolute;
    right: 0;
    bottom: calc(100% + 7px);
    width: min(310px, calc(100cqw - 12px), calc(100vw - 28px));
    margin: 0;
    padding: 0;
    overflow: hidden;
    border: 1px solid var(--border-strong);
    border-radius: 3px;
    background: var(--popover);
    box-shadow: 0 18px 48px rgb(0 0 0 / 48%);
    color: var(--text-soft);
    font: inherit;
  }

  .status-overflow-popover > header {
    display: flex;
    min-height: 46px;
    align-items: center;
    justify-content: space-between;
    padding: 7px 8px 7px 12px;
    border-bottom: 1px solid var(--border);
    background: var(--popover);
  }

  .status-overflow-popover > header button {
    display: grid;
    width: 28px;
    height: 28px;
    place-items: center;
    border: 1px solid var(--border-strong);
    border-radius: 2px;
    color: var(--text-soft);
    font-size: 15px;
  }

  .overflow-actions {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    border-bottom: 1px solid var(--border);
  }

  .overflow-actions button {
    min-height: 30px;
    border-right: 1px solid var(--border);
    color: var(--text-soft);
    font-size: var(--font-size-xs);
    font-weight: 650;
    text-transform: uppercase;
  }

  .overflow-actions button:last-child {
    border-right: 0;
  }

  .overflow-actions button:not(:disabled):hover {
    background: var(--accent);
    color: var(--fog);
  }

  .status-overflow-popover dl {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    margin: 0;
    border-bottom: 1px solid var(--border);
  }

  .status-overflow-popover dl div {
    display: flex;
    min-width: 0;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 7px 9px;
    border-right: 1px solid var(--border);
    border-bottom: 1px solid var(--border);
  }

  .status-overflow-popover dl div:nth-child(even) {
    border-right: 0;
  }

  .status-overflow-popover dl div:nth-last-child(-n + 2) {
    border-bottom: 0;
  }

  .status-overflow-popover dt {
    color: var(--muted-foreground);
    font-size: var(--font-size-xs);
    text-transform: uppercase;
  }

  .status-overflow-popover dd {
    margin: 0;
    overflow: hidden;
    color: var(--foreground);
    font-weight: 650;
    text-overflow: ellipsis;
    text-transform: uppercase;
  }

  .overflow-timers {
    display: grid;
    gap: 3px;
    padding: 8px 9px;
    border-bottom: 1px solid var(--border);
  }

  .overflow-subprocesses {
    display: flex;
    width: 100%;
    min-height: 34px;
    align-items: center;
    justify-content: space-between;
    padding: 0 10px;
    color: var(--text-soft);
  }

  .overflow-subprocesses span {
    color: var(--signal);
    font-size: var(--font-size-sm);
  }

  .overflow-subprocesses strong {
    font-size: var(--font-size-xs);
    text-transform: uppercase;
  }

  .overflow-subprocesses:not(:disabled):hover {
    background: var(--accent);
    color: var(--fog);
  }

  .subprocess-popover {
    position: relative;
    width: min(620px, calc(100vw - 42px));
    margin: 0;
    padding: 0;
    overflow: hidden;
    border: 1px solid var(--border-strong);
    border-radius: 3px;
    background: var(--popover);
    box-shadow: 0 18px 48px rgb(0 0 0 / 48%);
    color: var(--text-soft);
    font: inherit;
  }

  .subprocess-popover::after {
    position: absolute;
    right: 32px;
    bottom: -5px;
    width: 8px;
    height: 8px;
    border-right: 1px solid var(--border-strong);
    border-bottom: 1px solid var(--border-strong);
    background: var(--popover);
    content: '';
    transform: rotate(45deg);
  }

  .subprocess-popover > header,
  .subprocess-popover > footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .subprocess-popover > header {
    min-height: 48px;
    padding: 8px 11px 7px 15px;
    border-bottom: 1px solid var(--border);
    background: var(--popover);
  }

  .eyebrow {
    display: block;
    margin-bottom: 3px;
    color: var(--signal);
    font-size: var(--font-size-xs);
    font-weight: 700;
    letter-spacing: 0.12em;
    text-transform: uppercase;
  }

  h2 {
    margin: 0;
    color: var(--fog);
    font-family: 'Archivo Variable', sans-serif;
    font-size: 13px;
    font-weight: 650;
  }

  .root-pid {
    border: 1px solid var(--border-strong);
    border-radius: 2px;
    padding: 4px 6px;
    color: var(--muted-foreground);
    font-size: var(--font-size-xs);
  }

  .child-list {
    max-height: 300px;
    overflow-y: auto;
    padding: 5px 0;
  }

  .child-row {
    display: grid;
    min-height: 54px;
    grid-template-columns: 18px minmax(150px, 1fr) auto auto;
    align-items: center;
    padding: 4px 9px 4px 7px;
  }

  .child-row + .child-row {
    border-top: 1px solid var(--border);
  }

  .lineage {
    position: relative;
    width: 12px;
    height: 100%;
  }

  .lineage::before {
    position: absolute;
    top: -9px;
    bottom: -9px;
    left: 4px;
    width: 1px;
    background: var(--border-strong);
    content: '';
  }

  .lineage i {
    position: absolute;
    top: 50%;
    left: 4px;
    width: 8px;
    height: 1px;
    background: var(--signal);
  }

  .child-copy {
    min-width: 0;
    padding-right: 12px;
  }

  .child-heading {
    display: flex;
    min-width: 0;
    align-items: baseline;
    gap: 8px;
  }

  .child-heading strong {
    overflow: hidden;
    color: var(--foreground);
    font-family: 'Archivo Variable', sans-serif;
    font-size: var(--font-size-sm);
    font-weight: 650;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .child-heading span {
    color: var(--muted-foreground);
    font-size: var(--font-size-xs);
    white-space: nowrap;
  }

  code {
    display: block;
    margin-top: 4px;
    overflow: hidden;
    color: var(--muted-foreground);
    font-family: inherit;
    font-size: var(--font-size-xs);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .child-stats {
    display: grid;
    min-width: 74px;
    grid-template-columns: repeat(2, minmax(35px, auto));
    gap: 8px;
    color: var(--text-soft);
    font-size: var(--font-size-xs);
    font-variant-numeric: tabular-nums;
    text-align: right;
  }

  .kill-action {
    display: flex;
    min-width: 70px;
    justify-content: flex-end;
    gap: 4px;
    margin-left: 10px;
  }

  .kill-action button,
  .subprocess-popover > footer button {
    border: 1px solid var(--border-strong);
    border-radius: 2px;
    padding: 4px 6px;
    background: var(--accent);
    color: var(--text-soft);
    font-size: var(--font-size-xs);
    font-weight: 650;
    text-transform: uppercase;
  }

  .kill-action .kill:hover,
  .kill-action .confirm {
    border-color: #714747;
    color: #e58b8b;
  }

  .empty-tree {
    display: flex;
    min-height: 76px;
    align-items: center;
    justify-content: center;
    gap: 8px;
    color: var(--muted-foreground);
    font-size: var(--font-size-xs);
    text-transform: uppercase;
  }

  .empty-tree span {
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: var(--signal);
    box-shadow: 0 0 0 3px rgb(85 185 137 / 12%);
  }

  .subprocess-popover > footer {
    min-height: 34px;
    padding: 5px 8px 5px 14px;
    border-top: 1px solid var(--border);
    color: var(--muted-foreground);
    font-size: var(--font-size-xs);
  }

  .stage-medium .nav-label,
  .stage-medium .uptime-prefix,
  .stage-medium .metric-label,
  .stage-medium .pull-request-number,
  .stage-medium .subprocess-label,
  .stage-medium .process-name,
  .stage-medium .daemon-port {
    display: none;
  }

  .stage-medium .navigation button,
  .stage-medium .telemetry > span,
  .stage-medium .daemon-connection,
  .stage-medium .pull-request-link,
  .stage-medium .subprocess-trigger {
    padding-right: 7px;
    padding-left: 7px;
  }

  .stage-medium .navigation button {
    min-width: 28px;
    justify-content: center;
  }

  .stage-narrow .navigation,
  .stage-narrow .secondary-metric,
  .stage-narrow .process-name {
    display: none;
  }

  .stage-narrow .telemetry {
    margin-left: auto;
  }

  .stage-narrow .agent-attention,
  .stage-narrow .run-state {
    padding-right: 8px;
    padding-left: 8px;
  }

  .stage-tiny .uptime,
  .stage-tiny .secondary-metric,
  .stage-tiny .process-name,
  .stage-tiny .agent-attention,
  .stage-tiny .subprocess-trigger {
    display: none;
  }

  .stage-tiny .state-word {
    display: none;
  }

  .stage-tiny .run-state {
    width: 30px;
    justify-content: center;
    padding: 0;
  }

  .stage-tiny .status-overflow-control {
    display: flex;
  }

  @media (prefers-reduced-motion: no-preference) {
    .daemon-log-popover,
    .subprocess-popover,
    .status-overflow-popover {
      animation: reveal 120ms ease-out;
      transform-origin: right bottom;
    }

    @keyframes reveal {
      from {
        opacity: 0;
        transform: translateY(3px) scale(0.99);
      }
    }
  }
</style>
